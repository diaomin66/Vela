import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '@playwright/test';

const root = process.cwd();
const executable = path.resolve(process.argv[2] ?? 'src-tauri/target/release/vela.exe');
await mkdir(path.join(root, 'artifacts'), { recursive: true });
const sandbox = await mkdtemp(path.join(root, 'artifacts', 'native-smoke-'));
const codexHome = path.join(sandbox, 'codex');
await mkdir(codexHome);
const configPath = path.join(codexHome, 'config.toml');
const original = '# Native smoke test only.\nmodel = "original-model"\n\n[features]\nexample = true\n';
await writeFile(configPath, original);
const syntheticKey = `smoke-only-${crypto.randomUUID()}`;
let requestCount = 0;
let serverFailure;
const event = (type, data) => `event: ${type}\ndata: ${JSON.stringify({ type, ...data })}\n\n`;
const server = createServer(async (request, response) => {
  try {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks).toString());
    requestCount++;
    assert.equal(request.url, '/custom/v1/responses');
    assert.equal(request.headers.authorization, `Bearer ${syntheticKey}`);
    const id = `resp_smoke_${requestCount}`;
    const output = requestCount === 2
      ? [{ type: 'function_call', id: 'fc_smoke', call_id: 'call_smoke', name: body.tools[0].name, arguments: '{"value":"ok"}' }]
      : [{ type: 'message', id: 'msg_smoke', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'OK', annotations: [] }] }];
    if (requestCount === 3) assert(body.input.some((item) => item.type === 'function_call_output' && item.call_id === 'call_smoke'));
    let stream = event('response.created', { response: { id } });
    stream += requestCount === 2
      ? event('response.function_call_arguments.delta', { delta: '{"value":"ok"}', item_id: 'fc_smoke' }) + event('response.function_call_arguments.done', { arguments: '{"value":"ok"}', item_id: 'fc_smoke' })
      : event('response.output_text.delta', { delta: 'OK' });
    stream += event('response.completed', { response: { id, object: 'response', model: body.model, status: 'completed', output } });
    response.writeHead(200, { 'Content-Type': 'text/event-stream' });
    response.end(stream);
  } catch (error) { serverFailure = error; response.writeHead(500); response.end(); }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const endpoint = `http://127.0.0.1:${server.address().port}/custom/v1`;
const port = 9337;
const child = spawn(executable, [], {
  cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore',
  env: { ...process.env, PATH: `${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`, CODEX_HOME: codexHome, VELA_DATA_DIR: path.join(sandbox, 'vela-data'), WEBVIEW2_USER_DATA_FOLDER: path.join(sandbox, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --no-proxy-server` },
});
let browser;
let page;
let profileId;
let originalBackupId;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
try {
  for (let i = 0; i < 100; i++) {
    if (child.exitCode !== null) throw new Error(`Desktop process exited unexpectedly (${child.exitCode}).`);
    try { if ((await fetch(`http://127.0.0.1:${port}/json/version`)).ok) break; } catch { /* WebView2 is starting. */ }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  page = browser.contexts()[0].pages()[0] ?? await browser.contexts()[0].waitForEvent('page');
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.getByRole('heading', { name: '连接管理', exact: true }).waitFor({ timeout: 15000 });
  assert(!page.url().includes('127.0.0.1:1420'), 'Native release must load bundled UI, not the development server.');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.evaluate(() => document.fonts.ready);
  assert.equal(await page.locator('.preview-ribbon').count(), 0);
  const initial = await invoke('get_dashboard');
  assert.equal(initial.environment.desktopMode, true);
  assert.equal(path.resolve(initial.environment.configPath), configPath);
  assert.equal(initial.profiles.length, 0);
  await page.screenshot({ path: path.join(root, 'artifacts/screenshots/vela-native.png') });
  const local = await invoke('run_diagnostics', { runId: crypto.randomUUID(), includeNetwork: false });
  assert(local.items.length > 0);
  assert.equal(requestCount, 0);
  const profile = await invoke('save_profile', { input: { name: 'Native smoke test', baseUrl: endpoint, model: 'smoke-model', apiKey: syntheticKey } });
  profileId = profile.id;
  assert.equal(profile.keyStored, true);
  assert(!JSON.stringify(profile).includes(syntheticKey));
  const validation = await invoke('validate_profile', { id: profileId, runId: crypto.randomUUID() });
  if (serverFailure) throw serverFailure;
  assert.equal(validation.ok, true, JSON.stringify(validation.items));
  assert.equal(requestCount, 3);
  assert(!JSON.stringify(validation).includes(syntheticKey));
  const preview = await invoke('preview_profile', { id: profileId });
  assert(!JSON.stringify(preview).includes(syntheticKey));
  const backup = await invoke('apply_profile', { id: profileId, expectedHash: preview.expectedHash });
  originalBackupId = backup.id;
  const applied = await readFile(configPath, 'utf8');
  assert(applied.includes('vela_'));
  assert(applied.includes('original-model') === false);
  assert(applied.includes('example = true'));
  assert(!applied.includes(syntheticKey));
  const appliedDashboard = await invoke('get_dashboard');
  assert.equal(appliedDashboard.activeProfileId, profileId);
  assert.equal(appliedDashboard.activeProfileMatches, true);
  const stale = await invoke('preview_profile', { id: profileId });
  await writeFile(configPath, `${applied}\n# External edit during preview\n`);
  await assert.rejects(invoke('apply_profile', { id: profileId, expectedHash: stale.expectedHash }));
  const restored = await invoke('preview_restore', { id: originalBackupId });
  await invoke('restore_backup', { id: originalBackupId, expectedHash: restored.expectedHash });
  assert.equal(await readFile(configPath, 'utf8'), original);
  await invoke('delete_profile', { id: profileId });
  profileId = null;
  assert.equal((await invoke('get_dashboard')).profiles.length, 0);
  assert.deepEqual(errors, []);
  console.log('Native desktop smoke passed: real WebView2 UI, Rust IPC, isolated configuration, Windows credential storage, 3-request Responses validation, conflict rejection, encrypted backup and exact restore.');
  console.log('No real Codex configuration or real API key was used.');
} finally {
  if (page && profileId) {
    try {
      if (originalBackupId) { const p = await invoke('preview_restore', { id: originalBackupId }); await invoke('restore_backup', { id: originalBackupId, expectedHash: p.expectedHash }); }
      await invoke('delete_profile', { id: profileId });
    } catch { console.error('Synthetic credential cleanup needs checking for profile:', profileId); }
  }
  if (browser) await browser.close().catch(() => {});
  child.kill();
  server.close();
}
