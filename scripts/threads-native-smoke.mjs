import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomUUID, createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, writeFile, unlink, rename } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

// Only synthetic homes and an explicitly built executable are used. This never
// opens a real thread, launches an installer, or reads account credentials.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const executable = path.resolve(process.argv[2] ?? 'src-tauri/target/release/vela.exe');
const expectedVersion = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).version;
assert.equal(path.basename(executable).toLowerCase(), 'vela.exe');
await mkdir(path.join(root, 'artifacts'), { recursive: true });
const sandbox = await mkdtemp(path.join(root, 'artifacts', 'threads-native-'));
const home = path.join(sandbox, 'home');
const data = path.join(sandbox, 'data');
await mkdir(home); await mkdir(data);
const config = '# Isolated thread protection fixture.\n';
await writeFile(path.join(home, 'config.toml'), config);
await writeFile(path.join(data, 'update-preferences.json'), '{"autoDownload":false}');
const proxy = createServer((_, response) => { response.writeHead(503); response.end(); });
await new Promise((resolve) => proxy.listen(0, '127.0.0.1', resolve));
const proxyUrl = `http://127.0.0.1:${proxy.address().port}`;
const environment = {
  ...process.env, CODEX_HOME: home, VELA_DATA_DIR: data,
  HTTP_PROXY: proxyUrl, HTTPS_PROXY: proxyUrl, ALL_PROXY: proxyUrl,
  http_proxy: proxyUrl, https_proxy: proxyUrl, all_proxy: proxyUrl,
  NO_PROXY: '127.0.0.1,localhost', no_proxy: '127.0.0.1,localhost',
};
for (const key of Object.keys(environment)) if (/(?:API[_-]?KEY|TOKEN|PASSWORD|SECRET)/i.test(key)) delete environment[key];
delete environment.CODEX_SQLITE_HOME; delete environment.OPENAI_BASE_URL;
const report = { sandbox, executable, checks: [], passed: false };
let child; let browser; let page; let generation = 0;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
const check = (name) => { report.checks.push(name); console.log(`PASS ${name}`); };
async function until(test, label, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { const value = await test(); if (value) return value; await delay(80); }
  throw new Error(`Timeout: ${label}`);
}
async function freePort() {
  const server = createServer(); await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port; await new Promise((resolve) => server.close(resolve)); return port;
}
async function start() {
  const port = await freePort(); generation++;
  child = spawn(executable, [], { cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore', env: { ...environment, WEBVIEW2_USER_DATA_FOLDER: path.join(sandbox, `webview-${generation}`), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --no-proxy-server` } });
  let failed; child.once('error', (error) => { failed = error; });
  await until(async () => {
    if (failed) throw failed;
    assert.equal(child.exitCode, null);
    try { return (await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(400) })).ok; } catch { return false; }
  }, 'native startup');
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  page = browser.contexts()[0].pages()[0];
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__?.invoke);
  assert(!page.url().includes('127.0.0.1:1420'));
  const environmentInfo = (await invoke('get_dashboard')).environment;
  assert.equal(environmentInfo.appVersion, expectedVersion, 'The desktop binary must match the release version');
  assert.equal(path.resolve(environmentInfo.configPath), path.join(home, 'config.toml'));
}
async function stop() {
  if (browser) { await browser.close(); browser = undefined; }
  if (child && child.exitCode === null) {
    const exited = new Promise((resolve) => child.once('exit', resolve)); child.kill(); await exited;
  }
  child = undefined;
}
async function scan() {
  await invoke('scan_threads');
  return until(async () => { const overview = await invoke('get_thread_dashboard'); return overview.protection.state !== 'scanning' && overview.scannedAt ? overview : null; }, 'thread scan');
}
const list = (patch = {}) => invoke('list_threads', { query: { search: '', scope: 'all', status: 'all', sourceId: null, offset: 0, limit: 100, ...patch } });
async function fixture(archived = false, tail = '') {
  const id = randomUUID();
  const file = path.join(home, archived ? 'archived_sessions' : 'sessions/2026/10/04', `rollout-2026-10-04T09-00-00-${id}.jsonl`);
  await mkdir(path.dirname(file), { recursive: true });
  const content = [
    { timestamp: '2026-10-04T09:00:00Z', type: 'session_meta', payload: { id, timestamp: '2026-10-04T09:00:00Z', cwd: home, source: 'cli', model_provider: 'original-provider' } },
    { timestamp: '2026-10-04T09:00:01Z', type: 'event_msg', payload: { type: 'user_message', message: 'Synthetic thread history.' } },
  ].map((line) => JSON.stringify(line)).join('\n') + '\n' + tail;
  await writeFile(file, content);
  return { id, file, content, hash: digest(content) };
}

try {
  await start();
  const empty = await scan();
  assert.equal(empty.total, 0); assert.equal(empty.error, null); assert.deepEqual(empty.threads, []);
  check('New user starts with an empty catalog and compact IPC');
  const active = await fixture(); const archived = await fixture(true); const partial = await fixture(false, '{"type":');
  const scanned = await scan();
  assert.equal(scanned.total, 3); assert.equal(scanned.protected, 2);
  const pageOne = await list({ limit: 1 }); assert.equal(pageOne.threads.length, 1); assert.equal(pageOne.total, 3);
  const all = await list();
  const thread = all.threads.find((row) => row.threadId === active.id);
  const pending = all.threads.find((row) => row.threadId === partial.id);
  assert.equal(pending.snapshot, 'pending'); assert.equal(pending.integrity, 'partial');
  for (const item of [active, archived, partial]) assert.equal(digest(await readFile(item.file)), item.hash);
  check('Active and archived discovery, bounded pagination and live partial records preserve source bytes');
  const detail = await invoke('get_thread_detail', { key: thread.key });
  assert.equal(detail.rawAvailable, true); assert(detail.preview.some((item) => item.text.includes('Synthetic thread history')));
  assert.equal(detail.summary.provider, 'original-provider');
  await unlink(active.file);
  assert.equal((await scan()).recoverable, 1);
  assert.equal((await invoke('get_thread_detail', { key: thread.key })).rawAvailable, false);
  const preview = await invoke('preview_thread_restore', { key: thread.key });
  assert.equal(preview.targetExists, false); assert.equal(preview.conflict, false);
  await writeFile(active.file, 'Concurrent replacement must survive.\n');
  await assert.rejects(invoke('restore_thread', { key: thread.key, expectedHash: preview.expectedHash }));
  assert.equal(await readFile(active.file, 'utf8'), 'Concurrent replacement must survive.\n');
  await unlink(active.file);
  const fresh = await invoke('preview_thread_restore', { key: thread.key });
  await invoke('restore_thread', { key: thread.key, expectedHash: fresh.expectedHash });
  assert.equal(digest(await readFile(active.file)), active.hash);
  check('Missing history restores exactly and a target created after preview cannot be overwritten');
  const settings = await invoke('get_thread_settings');
  await invoke('save_thread_settings', { settings: { ...settings, intervalSeconds: 125, enabled: false } });
  await stop(); await start();
  assert.equal((await invoke('get_thread_settings')).intervalSeconds, 125);
  assert.equal((await invoke('get_thread_settings')).enabled, false);
  const persisted = await list();
  assert.equal(persisted.total, 3);
  assert.equal(persisted.threads.find((row) => row.threadId === partial.id).snapshot, 'pending');
  assert.equal(persisted.threads.find((row) => row.threadId === partial.id).integrity, 'partial');
  check('Thread catalog, snapshots and custom scheduling survive application restart');
  await page.getByRole('button', { name: '线程', exact: true }).click();
  await page.getByRole('heading', { name: '线程管理' }).waitFor();
  await page.screenshot({ path: path.join(sandbox, 'native-thread-page.png') });
  await stop();
  const inventory = path.join(data, 'threads', 'inventory.sqlite3');
  await rename(inventory, `${inventory}.fixture-preserved`);
  for (const suffix of ['-wal', '-shm']) {
    try { await rename(`${inventory}${suffix}`, `${inventory}.fixture-preserved${suffix}`); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  await start();
  assert.equal((await list()).total, 3);
  assert.equal(await readFile(path.join(home, 'config.toml'), 'utf8'), config);
  for (const item of [active, archived, partial]) assert.equal(digest(await readFile(item.file)), item.hash);
  check('Missing AhaX inventory rebuilds from encrypted snapshots without changing configuration or source history');
  report.passed = true;
} catch (error) {
  report.error = String(error?.stack ?? error); process.exitCode = 1;
} finally {
  await stop(); proxy.closeAllConnections(); await new Promise((resolve) => proxy.close(resolve));
  await writeFile(path.join(sandbox, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
}
