import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { copyFile, link, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

// Only the test-only Rust example is run. No ahaX application or installer is launched.
// Usage: node scripts/updater-smoke.mjs <signed-installer.exe> [probe.exe]
//        node scripts/updater-smoke.mjs --public [probe.exe]
const root = process.cwd();
const publicRelease = process.argv[2] === '--public';
const appVersion = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).version;
const installer = path.resolve(process.argv[2] ?? `release/ahaX_${appVersion}_x64-setup.exe`);
const executable = path.resolve(process.argv[3] ?? 'src-tauri/target/debug/examples/updater-smoke.exe');
await mkdir(path.join(root, 'artifacts'), { recursive: true });
const sandbox = await mkdtemp(path.join(root, 'artifacts', 'updater-smoke-'));
const probeExecutable = path.join(sandbox, 'updater-smoke.exe');
// The probe only downloads to memory and never modifies or installs itself.
// Reuse its read-only executable on the same volume instead of duplicating a
// potentially large debug binary for every isolated verification run.
try { await link(executable, probeExecutable); }
catch (error) {
  if (!['EXDEV', 'EPERM', 'ENOTSUP'].includes(error.code)) throw error;
  await copyFile(executable, probeExecutable);
}
try {
  await copyFile(path.join(path.dirname(executable), 'WebView2Loader.dll'), path.join(sandbox, 'WebView2Loader.dll'));
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
  await copyFile(path.join(path.dirname(executable), '..', 'WebView2Loader.dll'), path.join(sandbox, 'WebView2Loader.dll'));
}
const systemRoot = process.env.SystemRoot ?? 'C:\\Windows';
const env = {
  ...process.env,
  CODEX_HOME: path.join(sandbox, 'unused-config'),
  AHAX_DATA_DIR: path.join(sandbox, 'unused-data'),
  LOCALAPPDATA: path.join(sandbox, 'local'),
  APPDATA: path.join(sandbox, 'roaming'),
  WEBVIEW2_USER_DATA_FOLDER: path.join(sandbox, 'webview'),
  PATH: [systemRoot, path.join(systemRoot, 'System32'), path.join(systemRoot, 'System32', 'Wbem')].join(';'),
};
// Avoid duplicate case variants selecting an inherited real directory on Windows.
const overriddenVariables = ['PATH', 'CODEX_HOME', 'AHAX_DATA_DIR', 'LOCALAPPDATA', 'APPDATA', 'WEBVIEW2_USER_DATA_FOLDER'];
for (const key of Object.keys(env)) {
  const canonical = overriddenVariables.find((name) => name.toLowerCase() === key.toLowerCase());
  if (canonical && key !== canonical) delete env[key];
}
for (const directory of [env.LOCALAPPDATA, env.APPDATA]) await mkdir(directory, { recursive: true });

async function probe(name, endpoint) {
  const reportPath = path.join(sandbox, `${name}.json`);
  const exitCode = await new Promise((resolve, reject) => {
    const child = spawn(probeExecutable, [endpoint, reportPath], { cwd: sandbox, env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', (chunk) => { output += chunk; });
    child.stderr.on('data', (chunk) => { output += chunk; });
    const timer = setTimeout(() => { child.kill(); reject(new Error(`Updater probe timed out: ${name}`)); }, publicRelease ? 660_000 : 60_000);
    child.once('error', (error) => { clearTimeout(timer); reject(error); });
    child.once('exit', (code) => {
      clearTimeout(timer);
      if (code !== 0 && code !== 1) reject(new Error(`Updater probe crashed (${code}): ${output}`));
      else resolve(code);
    });
  });
  const result = JSON.parse(await readFile(reportPath, 'utf8'));
  assert.equal(result.installed, false);
  assert.equal(exitCode, result.ok ? 0 : 1);
  return result;
}

const results = {};
if (publicRelease) {
  results.publicRelease = await probe('public', 'https://github.com/diaomin66/ahaX/releases/latest/download/latest.json');
  assert.equal(results.publicRelease.ok, true, 'Published release must download and verify with the packaged public key.');
} else {
  const bytes = await readFile(installer);
  const signature = (await readFile(`${installer}.sig`, 'utf8')).trim();
  const { version } = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  const tampered = Buffer.from(bytes);
  tampered[tampered.length - 1] ^= 0xff;
  let port;
  const server = createServer((request, response) => {
    if (request.url.endsWith('.exe')) {
      const payload = request.url.includes('tampered') ? tampered : bytes;
      response.writeHead(200, { 'Content-Type': 'application/octet-stream', 'Content-Length': payload.length });
      response.end(payload);
      return;
    }
    const scenario = request.url.slice(1).replace('.json', '');
    const platform = { signature, url: `http://127.0.0.1:${port}/${scenario}.exe` };
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ version: scenario === 'mismatch' ? '99.0.0' : version, notes: 'Local signed-update smoke fixture', platforms: { 'windows-x86_64': platform, 'windows-x86_64-nsis': platform } }));
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => { port = server.address().port; resolve(); });
  });
  try {
    results.valid = await probe('valid', `http://127.0.0.1:${port}/valid.json`);
    assert.equal(results.valid.ok, true);
    assert.equal(results.valid.sha256, createHash('sha256').update(bytes).digest('hex'));
    assert.equal(results.valid.downloadedBytes, bytes.length);
    for (const scenario of ['tampered', 'mismatch']) {
      results[scenario] = await probe(scenario, `http://127.0.0.1:${port}/${scenario}.json`);
      assert.equal(results[scenario].ok, false);
      assert.equal(results[scenario].errorKind, 'signature');
    }
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}
const reportPath = path.join(sandbox, 'report.json');
await writeFile(reportPath, JSON.stringify({ passed: true, installed: false, results }, null, 2));
console.log(JSON.stringify({ passed: true, installed: false, reportPath, results }, null, 2));
