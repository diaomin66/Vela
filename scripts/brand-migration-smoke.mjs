import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const executable = path.resolve(process.argv[2] ?? path.join(root, 'src-tauri/target/release/ahax.exe'));
const expectedVersion = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).version;
assert.equal(path.basename(executable).toLowerCase(), 'ahax.exe');
await mkdir(path.join(root, 'artifacts'), { recursive: true });
const sandbox = await mkdtemp(path.join(root, 'artifacts/brand-migration-'));
const home = path.join(sandbox, 'home');
const data = path.join(sandbox, 'data');
await mkdir(home); await mkdir(data);
const profileId = randomUUID();
const model = 'synthetic-brand-model';
const channelKey = `isolated-brand-key-${randomUUID()}`;
const gatewayToken = `isolated-gateway-${randomUUID()}-${randomUUID()}`;
const gatewayDigest = createHash('sha256').update(data.toLowerCase()).digest('hex').slice(0, 32);
const gatewayId = `${gatewayDigest.slice(0, 8)}-${gatewayDigest.slice(8, 12)}-${gatewayDigest.slice(12, 16)}-${gatewayDigest.slice(16, 20)}-${gatewayDigest.slice(20)}`;
const routeDigest = createHash('sha256').update(profileId).update(Buffer.from([0])).update(model).digest('hex');
const legacyRoute = `vela-${routeDigest}`;
const currentRoute = `ahax-${routeDigest}`;
const credentialTool = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/cmdkey.exe');
const report = { executable, sandbox, version: expectedVersion, checks: [], requests: [], passed: false, cleanupErrors: [] };
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const pass = (name) => { report.checks.push(name); console.log(`PASS ${name}`); };
let child; let browser; let page; let generation = 0; let upstreamFailure;

async function command(program, args, env, timeout = 15000, input) {
  return new Promise((resolve, reject) => {
    const process = spawn(program, args, { cwd: sandbox, env, windowsHide: true, stdio: [input === undefined ? 'ignore' : 'pipe', 'pipe', 'pipe'] });
    if (input !== undefined) process.stdin.end(input);
    const output = [];
    process.stdout.on('data', (chunk) => output.push(chunk));
    process.stderr.resume();
    const timer = setTimeout(() => { process.kill(); reject(new Error('Isolated command timed out.')); }, timeout);
    process.once('error', (error) => { clearTimeout(timer); reject(error); });
    process.once('close', (code) => { clearTimeout(timer); resolve({ code, output: Buffer.concat(output).toString().trim() }); });
  });
}

async function writeLegacyCredential(id, secret) {
  const script = `$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class SyntheticCredentialFixture {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct Credential {
        public uint Flags; public uint Type; public string TargetName; public string Comment;
        public long LastWritten; public uint CredentialBlobSize; public IntPtr CredentialBlob;
        public uint Persist; public uint AttributeCount; public IntPtr Attributes;
        public string TargetAlias; public string UserName;
    }
    [DllImport("advapi32.dll", EntryPoint = "CredWriteW", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CredWrite(ref Credential credential, uint flags);
    public static void Save(string id, string secret) {
        Guid parsed = Guid.Parse(id);
        if (parsed.ToString() != id) throw new ArgumentException("Invalid synthetic ID");
        byte[] bytes = Encoding.UTF8.GetBytes(secret);
        IntPtr blob = Marshal.AllocHGlobal(bytes.Length);
        try {
            Marshal.Copy(bytes, 0, blob, bytes.Length);
            Credential credential = new Credential { Type = 1, TargetName = "Vela/connection/" + id,
                CredentialBlobSize = (uint)bytes.Length, CredentialBlob = blob, Persist = 2, UserName = "Vela" };
            if (!CredWrite(ref credential, 0)) throw new Win32Exception(Marshal.GetLastWin32Error());
        } finally {
            for (int index = 0; index < bytes.Length; index++) Marshal.WriteByte(blob, index, 0);
            Array.Clear(bytes, 0, bytes.Length); Marshal.FreeHGlobal(blob);
        }
    }
}
'@
$fixture = [Console]::In.ReadToEnd() | ConvertFrom-Json
[SyntheticCredentialFixture]::Save($fixture.id, $fixture.secret)
`;
  const result = await command('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], environment, 15000, JSON.stringify({ id, secret }));
  assert.equal(result.code, 0, 'Create only this run’s synthetic legacy credentials using the original UTF-8 format.');
}

async function listen(server) {
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  return server.address().port;
}

async function freePort() {
  const server = createServer();
  const port = await listen(server);
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function until(test, label, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const result = await test();
    if (result) return result;
    await delay(100);
  }
  throw new Error(`Timeout: ${label}`);
}

const proxy = createServer((_, response) => { response.writeHead(503); response.end(); });
const proxyPort = await listen(proxy);
const proxyUrl = `http://127.0.0.1:${proxyPort}`;
const environment = {
  ...process.env, CODEX_HOME: home, AHAX_DATA_DIR: data,
  LOCALAPPDATA: path.join(sandbox, 'local'), APPDATA: path.join(sandbox, 'roaming'),
  HTTP_PROXY: proxyUrl, HTTPS_PROXY: proxyUrl, ALL_PROXY: proxyUrl,
  NO_PROXY: '127.0.0.1,localhost',
};
for (const name of Object.keys(environment)) {
  const canonical = ['CODEX_HOME', 'AHAX_DATA_DIR', 'LOCALAPPDATA', 'APPDATA', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY'].find((key) => key.toLowerCase() === name.toLowerCase());
  if ((canonical && name !== canonical) || /(?:API[_-]?KEY|TOKEN|PASSWORD|SECRET)/i.test(name) || ['VELA_DATA_DIR', 'CODEXTOOL_DATA_DIR', 'CODEX_SQLITE_HOME', 'OPENAI_BASE_URL'].includes(name.toUpperCase())) delete environment[name];
}
await mkdir(environment.LOCALAPPDATA); await mkdir(environment.APPDATA);
const upstream = createServer(async (request, response) => {
  try {
    assert.equal(request.method, 'POST');
    assert.equal(request.url, '/v1/responses');
    assert.equal(request.headers.authorization, `Bearer ${channelKey}`);
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks).toString());
    assert.equal(body.model, model, 'Only the real upstream model may leave the local gateway.');
    assert.equal(body.reasoning?.effort, 'high');
    report.requests.push({ model: body.model, effort: body.reasoning.effort });
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ id: `resp_brand_${report.requests.length}`, object: 'response', status: 'completed', model, output: [] }));
  } catch (error) {
    upstreamFailure = error;
    response.writeHead(500, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ error: { message: 'Synthetic upstream assertion failed.' } }));
  }
});
const upstreamPort = await listen(upstream);
const gatewayPort = await freePort();
const timestamp = '2026-10-07T00:00:00Z';
const configPath = path.join(home, 'config.toml');
const oldHelper = path.join(path.dirname(executable), 'vela.exe');
const oldCatalog = path.join(data, 'catalogs', `models-${'a'.repeat(64)}.json`);
const literal = (value) => JSON.stringify(value);
const tomlString = (value) => value.startsWith("'") ? value.slice(1, -1) : JSON.parse(value);
const hasCurrentHelper = (contents) => [...contents.matchAll(/^command\s*=\s*(.+)$/gm)]
  .some((match) => path.resolve(tomlString(match[1].trim())) === executable);
const initialConfig = `# Synthetic 0.13 configuration; preserves unrelated preferences.\nmodel_provider = "Vela"\nmodel = "${legacyRoute}"\nmodel_catalog_json = ${literal(oldCatalog)}\nsandbox_mode = "read-only"\n\n[model_providers.Vela]\nname = "Vela"\nbase_url = "http://127.0.0.1:${gatewayPort}/v1"\nwire_api = "responses"\n[model_providers.Vela.auth]\ncommand = ${literal(oldHelper)}\nargs = ["--gateway-credential"]\n`;
await mkdir(path.dirname(oldCatalog));
await writeFile(oldCatalog, JSON.stringify({ models: [{ slug: legacyRoute, display_name: `Synthetic channel（${model}）` }] }));
await writeFile(configPath, initialConfig);
await writeFile(path.join(data, 'gateway-credential-id'), gatewayId);
await writeFile(path.join(data, 'update-preferences.json'), '{"autoDownload":false}');
await writeFile(path.join(data, 'connections.json'), JSON.stringify({
  profiles: [{ id: profileId, name: 'Synthetic channel', baseUrl: `http://127.0.0.1:${upstreamPort}/v1`, model, keyStored: true, createdAt: timestamp, updatedAt: timestamp, revision: randomUUID(), models: [{ id: model, alias: '', enabled: true, reasoningEfforts: ['low', 'high'], defaultReasoningEffort: 'high' }] }],
  backups: [], settings: { providerName: 'Vela', gatewayPort, autoRefresh: false, refreshMinutes: 15 }, settingsRevision: randomUUID(), defaultRouteId: legacyRoute,
}));

const invoke = (name, args = {}) => page.evaluate(({ name, args }) => window.__TAURI_INTERNALS__.invoke(name, args), { name, args });

async function start(legacyEnvironment = false) {
  const port = await freePort();
  const env = { ...environment, WEBVIEW2_USER_DATA_FOLDER: path.join(sandbox, `webview-${++generation}`), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --no-proxy-server` };
  if (legacyEnvironment) { delete env.AHAX_DATA_DIR; env.VELA_DATA_DIR = data; }
  child = spawn(executable, [], { cwd: path.dirname(executable), env, windowsHide: true, stdio: 'ignore' });
  let failure;
  child.once('error', (error) => { failure = error; });
  await until(async () => {
    if (failure) throw failure;
    assert.equal(child.exitCode, null, 'The isolated application must remain alive.');
    try { return (await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(400) })).ok; } catch { return false; }
  }, 'isolated native startup');
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  page = browser.contexts()[0].pages()[0] ?? await browser.contexts()[0].waitForEvent('page');
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__?.invoke);
  assert(!page.url().includes('127.0.0.1:1420'));
  const dashboard = await invoke('get_dashboard');
  assert.equal(dashboard.environment.appVersion, expectedVersion);
  assert.equal(path.resolve(dashboard.environment.configPath), configPath);
  assert.equal(dashboard.profiles.length, 1);
}

async function stop() {
  if (browser) { await browser.close(); browser = undefined; }
  if (child && child.exitCode === null) {
    const exited = new Promise((resolve) => child.once('exit', resolve));
    child.kill(); await exited;
  }
  child = undefined;
}

try {
  for (const [id, secret] of [[profileId, channelKey], [gatewayId, gatewayToken]]) {
    await writeLegacyCredential(id, secret);
  }
  await start();
  const helper = await command(executable, ['--credential', profileId], environment);
  assert.equal(helper.code, 0);
  assert.equal(helper.output, channelKey);
  pass('Legacy credentials remain readable and migrate to the ahaX namespace');
  const dashboard = await until(async () => {
    const current = await invoke('get_dashboard');
    const contents = await readFile(configPath, 'utf8');
    return current.gateway.running && contents.includes('model_provider = "ahaX"') && hasCurrentHelper(contents) ? current : null;
  }, 'automatic managed configuration upgrade');
  assert.equal(dashboard.settings.providerName, 'ahaX');
  assert.equal(dashboard.catalog[0].routeId, currentRoute);
  const upgradedConfig = await readFile(configPath, 'utf8');
  assert(upgradedConfig.includes('model_provider = "ahaX"'));
  assert(upgradedConfig.includes(currentRoute));
  assert(upgradedConfig.includes('sandbox_mode = "read-only"'));
  assert(hasCurrentHelper(upgradedConfig));
  const catalogLiteral = upgradedConfig.match(/^model_catalog_json\s*=\s*(.+)$/m)?.[1].trim();
  assert(catalogLiteral);
  const migratedCatalogPath = tomlString(catalogLiteral);
  const migratedCatalog = JSON.parse(await readFile(migratedCatalogPath, 'utf8'));
  assert.equal(migratedCatalog.models[0].slug, currentRoute);
  assert.equal(migratedCatalog.models[0].display_name, `Synthetic channel（${model}）`);
  assert(dashboard.backups.length > 0, 'Automatic upgrade must preserve the previous configuration as an encrypted backup.');
  pass('A saved 0.13 provider, catalog and same-directory helper upgrade with a backup');
  const token = await command(executable, ['--gateway-credential'], environment, 20000);
  assert.equal(token.code, 0);
  assert.equal(token.output, gatewayToken);
  pass('The saved gateway credential keeps its identity and token after rebranding');
  for (const route of [legacyRoute, currentRoute]) {
    const response = await fetch(`http://127.0.0.1:${gatewayPort}/v1/responses`, {
      method: 'POST', headers: { Authorization: `Bearer ${token.output}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ model: route, input: 'Local synthetic smoke only.', reasoning: { effort: 'high' }, stream: false }),
      signal: AbortSignal.timeout(15000),
    });
    assert.equal(response.status, 200);
    await response.json();
    if (upstreamFailure) throw upstreamFailure;
  }
  assert.equal(report.requests.length, 2);
  pass('Both old and new route IDs forward the real model and preserve reasoning effort');
  const unknown = await fetch(`http://127.0.0.1:${gatewayPort}/v1/responses`, {
    method: 'POST', headers: { Authorization: `Bearer ${token.output}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ model: `vela-${'f'.repeat(64)}`, input: 'Never forward an unknown internal model.' }), signal: AbortSignal.timeout(5000),
  });
  assert.equal(unknown.status, 404);
  await unknown.text();
  assert.equal(report.requests.length, 2);
  pass('Unknown internal routes fail locally without contacting an upstream');
  await stop();
  await start(true);
  const restarted = await invoke('get_dashboard');
  assert.equal(restarted.profiles[0].id, profileId);
  assert.equal(restarted.settings.providerName, 'ahaX');
  assert.equal(await readFile(path.join(data, 'gateway-credential-id'), 'utf8'), gatewayId);
  pass('The old custom data environment remains compatible without touching default storage');
  await stop();
  const directProvider = `vela_${profileId.replaceAll('-', '')}`;
  const directConfig = `# Synthetic legacy direct connection after a native model change.\nmodel_provider = "${directProvider}"\nmodel = "native-selected-model"\nmodel_reasoning_effort = "high"\nsandbox_mode = "read-only"\n\n[model_providers.${directProvider}]\nname = "My custom channel"\nbase_url = "http://127.0.0.1:${upstreamPort}/v1"\nwire_api = "responses"\n[model_providers.${directProvider}.auth]\ncommand = ${literal(oldHelper)}\nargs = ["--credential", "${profileId}"]\n`;
  await writeFile(configPath, directConfig);
  await start();
  await until(async () => hasCurrentHelper(await readFile(configPath, 'utf8')), 'legacy direct helper upgrade after a native model change');
  const directUpgrade = await readFile(configPath, 'utf8');
  assert.equal(directUpgrade.replace(/^command\s*=\s*.+$/m, `command = ${literal(oldHelper)}`), directConfig);
  const directHelper = await command(executable, ['--credential', profileId], environment);
  assert.equal(directHelper.code, 0);
  assert.equal(directHelper.output, channelKey);
  pass('A legacy direct connection retains a native model change and preferences while rebinding its helper');
  report.passed = true;
} catch (error) {
  report.error = String(error.stack ?? error).replaceAll(channelKey, '[synthetic key]').replaceAll(gatewayToken, '[synthetic token]');
  process.exitCode = 1;
} finally {
  try { await stop(); } catch { report.cleanupErrors.push('Stopping the isolated application failed.'); }
  for (const service of ['ahaX', 'Vela']) {
    for (const id of [profileId, gatewayId]) {
      try { await command(credentialTool, [`/delete:${service}/connection/${id}`], environment); }
      catch { report.cleanupErrors.push(`Synthetic credential cleanup failed for ${service}/${id}.`); }
    }
  }
  upstream.closeAllConnections(); proxy.closeAllConnections();
  await Promise.all([new Promise((resolve) => upstream.close(resolve)), new Promise((resolve) => proxy.close(resolve))]);
  await writeFile(path.join(sandbox, 'report.json'), JSON.stringify(report, null, 2));
  console.log(`Brand migration report: ${path.join(sandbox, 'report.json')}`);
  if (report.error) console.error(report.error);
}
