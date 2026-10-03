import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '@playwright/test';

// This test starts only the explicitly named Vela executable. All configuration,
// WebView2 state, channel keys, and HTTP services are generated for this run.
const root = process.cwd();
const { version } = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
const executable = path.resolve(process.argv[2] ?? 'src-tauri/target/release/vela.exe');
await mkdir(path.join(root, 'artifacts', 'screenshots'), { recursive: true });
const sandbox = await mkdtemp(path.join(root, 'artifacts', 'native-smoke-v2-'));
const codexHome = path.join(sandbox, 'codex');
const dataDirectory = path.join(sandbox, 'vela-data');
await mkdir(codexHome);
const configPath = path.join(codexHome, 'config.toml');
const original = '# Vela v0.3 native smoke only.\nmodel = "original-model"\nprofile = "work"\nmodel_reasoning_effort = "minimal"\nmodel_reasoning_summary = "detailed"\nmodel_supports_reasoning_summaries = true\n\n[features]\nexample = true\n\n[profiles.work]\nmodel_reasoning_effort = "xhigh"\nmodel_reasoning_summary = "auto"\nmodel_supports_reasoning_summaries = false\nsandbox_mode = "read-only"\n\n[profiles.spare]\nmodel_reasoning_effort = "high"\n';
await writeFile(configPath, original);

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const listen = (server) => new Promise((resolve, reject) => {
  server.once('error', reject);
  server.listen(0, '127.0.0.1', () => resolve(server.address().port));
});
const closeServer = (server) => new Promise((resolve) => {
  server.closeAllConnections();
  server.close(() => resolve());
});
async function unusedPort() {
  const reservation = createServer();
  const port = await listen(reservation);
  await closeServer(reservation);
  return port;
}
const json = (response, status, value) => {
  response.writeHead(status, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify(value));
};
const event = (type, value) => `event: ${type}\ndata: ${JSON.stringify({ type, ...value })}\n\n`;
const channels = [
  { name: 'Smoke Alpha', key: `test-alpha-${randomUUID()}`, unitQuota: 1500000, prefix: '/v1', requests: [], inference: [], efforts: ['low', 'medium', 'high', 'xhigh', 'max'], defaultEffort: 'medium', selectedEffort: 'max' },
  { name: 'Smoke Beta', key: `test-beta-${randomUUID()}`, unitQuota: 2500000, prefix: '/custom/v1', requests: [], inference: [], efforts: ['low', 'high'], defaultEffort: 'high', selectedEffort: 'low' },
];
const servers = [];
let serverFailure;
for (const channel of channels) {
  const server = createServer(async (request, response) => {
    try {
      assert.equal(request.headers.authorization, `Bearer ${channel.key}`, 'Upstream must receive only its own synthetic channel credential.');
      channel.requests.push({ method: request.method, path: request.url });
      if (request.method === 'GET' && request.url === `${channel.prefix}/models`) {
        return json(response, 200, { object: 'list', data: [
          { id: 'shared-coding-model', name: 'Shared coding model' },
          { id: 'disabled-model', name: 'Disabled model' },
        ] });
      }
      if (request.method === 'GET' && request.url === '/api/usage/token/') {
        return json(response, 200, { code: true, data: {
          object: 'token_usage', total_available: channel.unitQuota, total_used: 500000,
          total_granted: channel.unitQuota + 500000, unlimited_quota: false,
        } });
      }
      if (request.method === 'POST' && request.url === `${channel.prefix}/responses`) {
        const chunks = [];
        for await (const chunk of request) chunks.push(chunk);
        const body = JSON.parse(Buffer.concat(chunks).toString());
        assert.equal(body.model, 'shared-coding-model', 'Gateway must replace only its public route ID with the upstream model ID.');
        assert.equal(body.reasoning?.effort, channel.selectedEffort, 'Gateway must preserve the exact selected effort for the selected upstream.');
        assert.deepEqual(body.reasoning, { effort: channel.selectedEffort }, 'Gateway must not inject a summary or silently change reasoning settings.');
        assert.equal(request.headers.origin, undefined, 'Browser Origin must never be forwarded.');
        channel.inference.push(body);
        const id = `resp_${channel.name.replaceAll(' ', '_')}_${channel.inference.length}`;
        const output = [{ type: 'message', id: `msg_${channel.inference.length}`, role: 'assistant', status: 'completed', content: [
          { type: 'output_text', text: `${channel.name} OK`, annotations: [] },
        ] }];
        const result = { id, object: 'response', model: body.model, status: 'completed', output };
        if (!body.stream) return json(response, 200, result);
        response.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
        response.write(event('response.created', { response: { id, object: 'response', model: body.model, status: 'in_progress' } }));
        await delay(20);
        response.write(event('response.output_text.delta', { delta: `${channel.name} OK` }));
        await delay(20);
        response.end(event('response.completed', { response: result }));
        return;
      }
      return json(response, 404, { error: { message: 'Synthetic endpoint unavailable.' } });
    } catch (error) {
      serverFailure ??= error;
      if (!response.headersSent) response.writeHead(500);
      response.end();
    }
  });
  channel.origin = `http://127.0.0.1:${await listen(server)}`;
  servers.push(server);
}

const debugPort = await unusedPort();
const gatewayPort = await unusedPort();
const environment = {
  ...process.env,
  PATH: `${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`,
  CODEX_HOME: codexHome,
  VELA_DATA_DIR: dataDirectory,
  WEBVIEW2_USER_DATA_FOLDER: path.join(sandbox, 'webview'),
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort} --no-proxy-server`,
  HTTP_PROXY: '', HTTPS_PROXY: '', ALL_PROXY: '', NO_PROXY: '127.0.0.1,localhost',
};
const child = spawn(executable, [], {
  cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore', env: environment,
});
let spawnFailure;
child.once('error', (error) => { spawnFailure = error; });
let browser;
let page;
let secondInstance;
let backupId;
let restored = false;
const createdProfiles = new Set();
const invoke = (command, args = {}) => page.evaluate(
  ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args },
);
const fetchLocal = (url, options = {}) => fetch(url, { ...options, signal: AbortSignal.timeout(15000) });

async function waitForWindowVisibility(visible) {
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    assert.equal(child.exitCode, null, 'Closing the window must keep the original native process alive.');
    if (await invoke('plugin:window|is_visible', { label: 'main' }) === visible) return;
    await delay(50);
  }
  assert.fail(`The original native window did not become ${visible ? 'visible' : 'hidden'}.`);
}

async function reopenViaSecondInstance() {
  secondInstance = spawn(executable, [], {
    cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore', env: environment,
  });
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error('A second Vela process did not exit after waking the original window.')), 10000);
    secondInstance.once('error', (error) => { clearTimeout(deadline); reject(error); });
    secondInstance.once('close', (code) => {
      clearTimeout(deadline);
      if (code === 0) resolve();
      else reject(new Error(`The second Vela process exited with status ${code}.`));
    });
  });
  await waitForWindowVisibility(true);
  assert.equal(child.exitCode, null, 'The original process must remain the single application instance.');
  assert.equal(page.isClosed(), false, 'Reopening must keep the original WebView and its state.');
}

async function gatewayCredential() {
  return new Promise((resolve, reject) => {
    const helper = spawn(executable, ['--gateway-credential'], {
      cwd: path.dirname(executable), windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: environment,
    });
    const chunks = [];
    helper.stdout.on('data', (chunk) => chunks.push(chunk));
    // Do not surface helper stderr: credential helper errors must not become logs.
    helper.stderr.resume();
    helper.once('error', reject);
    const timeout = setTimeout(() => { helper.kill(); reject(new Error('Gateway credential helper timed out.')); }, 10000);
    helper.once('close', (code) => {
      clearTimeout(timeout);
      if (code !== 0) return reject(new Error(`Gateway credential helper exited with status ${code}.`));
      const value = Buffer.concat(chunks).toString().trim();
      if (value.length < 32) return reject(new Error('Gateway credential helper did not return a valid local token.'));
      resolve(value);
    });
  });
}

async function removeSyntheticGatewayCredential() {
  let storedId;
  try { storedId = (await readFile(path.join(dataDirectory, 'gateway-credential-id'), 'utf8')).trim(); }
  catch (error) { if (error.code === 'ENOENT') return; throw error; }
  const digest = createHash('sha256').update(dataDirectory.toLowerCase()).digest('hex').slice(0, 32);
  const expectedId = `${digest.slice(0, 8)}-${digest.slice(8, 12)}-${digest.slice(12, 16)}-${digest.slice(16, 20)}-${digest.slice(20)}`;
  assert.equal(storedId, expectedId, 'Only the credential derived from this run\'s temporary data directory may be removed.');
  await new Promise((resolve, reject) => {
    const cleanup = spawn(path.join(process.env.SystemRoot, 'System32', 'cmdkey.exe'), [`/delete:Vela/connection/${storedId}`], {
      windowsHide: true, stdio: 'ignore', env: environment,
    });
    cleanup.once('error', reject);
    cleanup.once('close', (code) => code === 0 ? resolve() : reject(new Error('Synthetic gateway credential cleanup failed.')));
  });
}

try {
  let debuggingReady = false;
  for (let attempt = 0; attempt < 150; attempt++) {
    if (spawnFailure) throw spawnFailure;
    if (child.exitCode !== null) throw new Error(`Isolated native process exited (${child.exitCode}).`);
    try {
      if ((await fetch(`http://127.0.0.1:${debugPort}/json/version`, { signal: AbortSignal.timeout(500) })).ok) {
        debuggingReady = true;
        break;
      }
    } catch { /* Wait for this run's WebView2 instance. */ }
    await delay(200);
  }
  assert(debuggingReady, 'Isolated native WebView2 did not start within the deadline.');
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${debugPort}`);
  page = browser.contexts()[0].pages()[0] ?? await browser.contexts()[0].waitForEvent('page');
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__?.invoke, null, { timeout: 15000 });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  assert(!page.url().includes('127.0.0.1:1420'), 'Native release must use its packaged UI.');
  const initial = await invoke('get_dashboard');
  assert.equal(initial.environment.desktopMode, true);
  assert.equal(path.resolve(initial.environment.configPath), configPath);
  assert.equal(initial.profiles.length, 0, 'The test must never reuse real channel credentials.');
  const updateStatus = await invoke('get_update_status');
  assert.equal(updateStatus.currentVersion, version);
  assert.equal(updateStatus.autoDownload, true);
  // Exercise the packaged IPC and atomic persistence without running an installer.
  const manualDownloads = await invoke('set_update_preferences', { autoDownload: false });
  assert.equal(manualDownloads.autoDownload, false);
  assert.deepEqual(JSON.parse(await readFile(path.join(dataDirectory, 'update-preferences.json'), 'utf8')), { autoDownload: false });
  assert.equal(await readFile(configPath, 'utf8'), original, 'Update preferences must not modify the user configuration.');
  if (process.env.VELA_SMOKE_PUBLIC_UPDATE === '1') {
    let publishedStatus = await invoke('check_for_updates');
    const checkDeadline = Date.now() + 40000;
    while (publishedStatus.phase === 'checking' && Date.now() < checkDeadline) {
      await delay(200);
      publishedStatus = await invoke('get_update_status');
    }
    assert.equal(publishedStatus.phase, 'latest', 'The published application must recognize its own version via the actual GitHub update endpoint.');
    assert.equal(publishedStatus.autoDownload, false);
    assert(publishedStatus.checkedAt);
  }
  await invoke('save_settings', { input: {
    providerName: 'Vela', gatewayPort, autoRefresh: false, refreshMinutes: 15,
  } });

  for (let index = 0; index < channels.length; index++) {
    const channel = channels[index];
    const baseUrl = index === 0 ? channel.origin : `${channel.origin}/custom/v1/responses`;
    const saved = await invoke('save_profile', { input: {
      name: channel.name, baseUrl, apiKey: channel.key, model: '', balanceConfig: { mode: 'auto' },
    } });
    createdProfiles.add(saved.id);
    channel.profileId = saved.id;
    assert.equal(saved.keyStored, true);
    assert(!JSON.stringify(saved).includes(channel.key));
    const synced = await invoke('sync_profile', { id: saved.id, runId: randomUUID() });
    if (serverFailure) throw serverFailure;
    assert.equal(synced.resolvedBaseUrl, `${channel.origin}${channel.prefix}`);
    assert(synced.models.some((model) => model.id === 'shared-coding-model'));
    assert.equal(synced.balance.status, 'available');
    assert.equal(synced.balance.remaining, channel.unitQuota);
    assert.equal(synced.balance.unit, '额度');
    channel.saved = await invoke('save_profile', { input: {
      id: synced.id, name: synced.name, baseUrl: synced.baseUrl, model: 'shared-coding-model',
      models: synced.models.map((model) => ({
        ...model, alias: model.id === 'shared-coding-model' ? `${channel.name} Coding` : model.alias,
        enabled: model.id === 'shared-coding-model',
        reasoningEfforts: model.id === 'shared-coding-model' ? channel.efforts : [],
        defaultReasoningEffort: model.id === 'shared-coding-model' ? channel.defaultEffort : null,
      })), balanceConfig: synced.balanceConfig,
    } });
    const savedModel = channel.saved.models.find((model) => model.id === 'shared-coding-model');
    assert.deepEqual(savedModel.reasoningEfforts, channel.efforts, 'Saved channel models must retain configured native reasoning choices.');
    assert.equal(savedModel.defaultReasoningEffort, channel.defaultEffort);
    const refreshed = await invoke('sync_profile', { id: synced.id, runId: randomUUID() });
    const refreshedModel = refreshed.models.find((model) => model.id === 'shared-coding-model');
    assert.deepEqual(refreshedModel.reasoningEfforts, channel.efforts, 'GET metadata refresh must preserve manually configured reasoning capabilities.');
    assert.equal(refreshedModel.defaultReasoningEffort, channel.defaultEffort);
  }
  assert(channels[0].requests.some((request) => request.path === '/models'));
  assert(channels[0].requests.some((request) => request.path === '/v1/models'));
  assert(channels[1].requests.some((request) => request.path === '/custom/v1/models'));
  const configured = await invoke('get_dashboard');
  const enabled = configured.catalog.filter((model) => model.enabled);
  assert.equal(enabled.length, 2);
  assert.equal(new Set(enabled.map((model) => model.routeId)).size, 2, 'Same upstream model ID in different channels must have distinct public routes.');
  for (const channel of channels) {
    channel.route = enabled.find((model) => model.profileId === channel.profileId);
    assert.equal(channel.route.modelId, 'shared-coding-model');
    assert(channel.route.displayName.includes('shared-coding-model'));
    assert(!channel.route.displayName.includes('vela-'), 'Display labels must not expose opaque generated route names.');
    assert.deepEqual(channel.route.supportedReasoningEfforts, channel.efforts);
    assert.equal(channel.route.defaultReasoningEffort, channel.defaultEffort);
  }
  const preview = await invoke('preview_gateway', { defaultRouteId: channels[0].route.routeId });
  assert(!channels.some((channel) => JSON.stringify(preview).includes(channel.key)));
  const backup = await invoke('apply_gateway', {
    defaultRouteId: channels[0].route.routeId, expectedHash: preview.expectedHash,
  });
  backupId = backup.id;
  const applied = await readFile(configPath, 'utf8');
  assert(applied.includes('name = "Vela"'));
  assert(applied.includes('example = true'));
  assert(!channels.some((channel) => applied.includes(channel.key)));
  const rootSection = applied.split(/^\[/m)[0];
  const workSection = applied.match(/^\[profiles\.work\]\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m)?.[1];
  const spareSection = applied.match(/^\[profiles\.spare\]\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m)?.[1];
  assert(workSection, 'The selected named profile must be preserved.');
  for (const key of ['model_reasoning_effort', 'model_reasoning_summary', 'model_supports_reasoning_summaries']) {
    assert(!rootSection.includes(key), `Root ${key} must be cleared so per-model defaults can work.`);
    assert(!workSection.includes(key), `The selected profile must not override native ${key}.`);
  }
  assert(workSection.includes('sandbox_mode = "read-only"'));
  assert(spareSection?.includes('model_reasoning_effort = "high"'), 'Unrelated named profiles must remain untouched.');
  const catalogLiteral = rootSection.match(/^model_catalog_json\s*=\s*(.+)$/m)?.[1].trim();
  assert(catalogLiteral, 'Applied configuration must reference its actual immutable native catalog.');
  const catalogPath = catalogLiteral.startsWith("'") ? catalogLiteral.slice(1, -1) : JSON.parse(catalogLiteral);
  const catalogRelativePath = path.relative(path.join(dataDirectory, 'catalogs'), catalogPath);
  assert(!catalogRelativePath.startsWith('..') && !path.isAbsolute(catalogRelativePath), 'The actual catalog must remain inside this isolated application data directory.');
  const nativeCatalog = JSON.parse(await readFile(catalogPath, 'utf8'));
  assert.equal(nativeCatalog.models.length, 2);
  for (const channel of channels) {
    const nativeModel = nativeCatalog.models.find((model) => model.slug === channel.route.routeId);
    assert(nativeModel, 'Every enabled channel route must be present in the actual file consumed by Codex.');
    assert.equal(nativeModel.display_name, channel.route.displayName);
    assert.deepEqual(nativeModel.supported_reasoning_levels.map((preset) => preset.effort), channel.efforts);
    assert(nativeModel.supported_reasoning_levels.every((preset) => typeof preset.description === 'string' && preset.description.length > 0));
    assert.equal(nativeModel.default_reasoning_level, channel.defaultEffort);
    assert.equal(nativeModel.supports_reasoning_summaries, true, 'Codex 0.137 requires the legacy flag to forward reasoning effort.');
    assert.equal(nativeModel.supports_reasoning_summary_parameter, false, 'Current clients must not assume third-party summary support.');
    assert.equal(nativeModel.default_reasoning_summary, 'none');
    assert(nativeModel.base_instructions.length > 1000, 'Native reasoning support must preserve the full coding prompt.');
  }
  const active = await invoke('get_dashboard');
  assert.equal(active.gatewayApplied, true);
  assert.equal(active.gateway.running, true);
  assert.equal(active.gateway.port, gatewayPort);

  // Exercise the packaged frontend against the saved native capabilities.
  // Opening selectors must never trigger a billable model validation request.
  const inferenceBeforeMenus = channels.map((channel) => channel.inference.length);
  await page.bringToFront();
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '模型库', exact: true }).click();
  await page.getByRole('heading', { name: '模型库', exact: true }).waitFor({ state: 'visible' });
  const effortLabels = { low: '低', medium: '中', high: '高', xhigh: '超高', max: '最高' };
  for (const [index, channel] of channels.entries()) {
    const label = `${channel.route.displayName} 的推理强度`;
    const picker = page.getByRole('combobox', { name: label, exact: true });
    await picker.waitFor({ state: 'visible', timeout: 15000 });
    assert.equal((await picker.innerText()).trim(), effortLabels[channel.defaultEffort]);
    await picker.click();
    const list = page.getByRole('listbox', { name: label, exact: true });
    await list.waitFor({ state: 'visible' });
    assert.deepEqual((await list.getByRole('option').allTextContents()).map((text) => text.trim()), channel.efforts.map((effort) => effortLabels[effort]));
    assert.equal((await list.getByRole('option', { selected: true }).innerText()).trim(), effortLabels[channel.defaultEffort]);
    if (index === 0) {
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: path.join(root, 'artifacts', 'screenshots', `vela-native-${version}-models.png`) });
    }
    await page.keyboard.press('Escape');
    await list.waitFor({ state: 'hidden' });
  }
  assert.deepEqual(channels.map((channel) => channel.inference.length), inferenceBeforeMenus, 'Viewing native model selectors must never send model requests.');

  const localToken = await gatewayCredential();
  assert(!channels.some((channel) => localToken === channel.key));
  assert(!applied.includes(localToken));
  const gateway = `http://127.0.0.1:${gatewayPort}/v1`;
  const headers = { Authorization: `Bearer ${localToken}`, 'Content-Type': 'application/json' };
  const listed = await fetchLocal(`${gateway}/models`, { headers });
  assert.equal(listed.status, 200);
  const catalog = await listed.json();
  assert.deepEqual(new Set(catalog.data.map((model) => model.id)), new Set(enabled.map((model) => model.routeId)));
  assert(!catalog.data.some((model) => model.name.includes('disabled-model')));

  const resultA = await fetchLocal(`${gateway}/responses`, {
    method: 'POST', headers, body: JSON.stringify({ model: channels[0].route.routeId, input: 'Synthetic alpha routing check.', reasoning: { effort: channels[0].selectedEffort }, stream: false }),
  });
  assert.equal(resultA.status, 200);
  const responseA = await resultA.json();
  assert.equal(responseA.model, channels[0].route.routeId);
  assert.equal(responseA.status, 'completed');
  assert.equal(responseA.output[0].content[0].text, 'Smoke Alpha OK');
  assert.equal(channels[0].inference.length, 1);
  assert.deepEqual(channels[0].inference[0].reasoning, { effort: channels[0].selectedEffort });
  assert.equal(channels[1].inference.length, 0);

  const resultB = await fetchLocal(`${gateway}/responses`, {
    method: 'POST', headers, body: JSON.stringify({ model: channels[1].route.routeId, input: 'Synthetic beta streaming check.', reasoning: { effort: channels[1].selectedEffort }, stream: true }),
  });
  assert.equal(resultB.status, 200);
  assert(resultB.headers.get('content-type').includes('text/event-stream'));
  const streamed = await resultB.text();
  assert(streamed.includes('response.output_text.delta'));
  assert(streamed.includes('response.completed'));
  assert(streamed.includes('Smoke Beta OK'));
  assert(!streamed.includes(localToken));
  assert(!channels.some((channel) => streamed.includes(channel.key)));
  assert.equal(channels[0].inference.length, 1);
  assert.equal(channels[1].inference.length, 1);
  assert.deepEqual(channels[1].inference[0].reasoning, { effort: channels[1].selectedEffort });
  if (serverFailure) throw serverFailure;

  const wrongToken = await fetchLocal(`${gateway}/models`, { headers: { Authorization: 'Bearer intentionally-wrong-smoke-token' } });
  assert.equal(wrongToken.status, 401);
  const foreignOrigin = await fetchLocal(`${gateway}/models`, { headers: { ...headers, Origin: 'https://untrusted.example' } });
  assert.equal(foreignOrigin.status, 403);
  const crossChannel = await fetchLocal(`${gateway}/responses`, {
    method: 'POST', headers, body: JSON.stringify({ model: channels[1].route.routeId, previous_response_id: responseA.id, input: 'Must stay local.' }),
  });
  assert.equal(crossChannel.status, 409);
  assert.equal(channels[1].inference.length, 1, 'Cross-channel continuation must be rejected before forwarding.');

  await waitForWindowVisibility(true);
  await page.getByRole('button', { name: '关闭窗口', exact: true }).click();
  await waitForWindowVisibility(false);
  const hiddenHealth = await fetchLocal(`http://127.0.0.1:${gatewayPort}/health`, { headers });
  assert.equal(hiddenHealth.status, 200, 'Authenticated gateway health must remain available after close-to-tray.');
  const health = await hiddenHealth.json();
  assert.equal(health.service, 'Vela');
  assert.equal(health.running, true);
  const hiddenUnauthorized = await fetchLocal(`http://127.0.0.1:${gatewayPort}/health`);
  assert.equal(hiddenUnauthorized.status, 401, 'Close-to-tray must not relax local gateway authentication.');
  const hiddenModels = await fetchLocal(`${gateway}/models`, { headers });
  assert.equal(hiddenModels.status, 200);
  assert.equal((await hiddenModels.json()).data.length, 2);
  assert.equal(child.exitCode, null, 'Closing the main window must not terminate the background gateway.');
  await reopenViaSecondInstance();
  const reopened = await invoke('get_dashboard');
  assert.equal(reopened.profiles.length, 2);
  assert.equal(reopened.defaultRouteId, channels[0].route.routeId);
  assert.equal(reopened.gateway.port, gatewayPort);

  const stalePreview = await invoke('preview_gateway', { defaultRouteId: channels[0].route.routeId });
  await writeFile(configPath, `${applied}\n# External edit in isolated smoke\n`);
  await assert.rejects(invoke('apply_gateway', {
    defaultRouteId: channels[0].route.routeId, expectedHash: stalePreview.expectedHash,
  }));
  const restore = await invoke('preview_restore', { id: backupId });
  await invoke('restore_backup', { id: backupId, expectedHash: restore.expectedHash });
  assert.equal(await readFile(configPath, 'utf8'), original);
  restored = true;
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: path.join(root, 'artifacts', 'screenshots', 'vela-native-v2.png') });
  for (const id of createdProfiles) {
    await invoke('delete_profile', { id });
    createdProfiles.delete(id);
  }
  assert.equal((await invoke('get_dashboard')).profiles.length, 0);
  assert.deepEqual(errors, []);
  console.log(`Vela ${version} native smoke passed: packaged UI, isolated IPC/configuration, model discovery, original quota units, root/v1 normalization, unique routes for two channels, persisted reasoning choices including max, actual native catalog metadata, per-model reasoning defaults, stale reasoning override cleanup, unchanged reasoning effort through both upstream routes, real credential helper, authenticated Models/Responses, SSE, Origin/token rejection, continuation isolation, close-to-tray background availability, single-instance window reopening, stale preview rejection, and exact backup restore.`);
  console.log('Only temporary local services, synthetic channel keys, and an isolated CODEX_HOME were used.');
} finally {
  if (page) {
    if (backupId && !restored) {
      try {
        const restore = await invoke('preview_restore', { id: backupId });
        await invoke('restore_backup', { id: backupId, expectedHash: restore.expectedHash });
      } catch { console.error('Isolated smoke configuration could not be restored:', configPath); }
    }
    for (const id of createdProfiles) {
      try { await invoke('delete_profile', { id }); }
      catch { console.error('Synthetic channel credential cleanup requires checking for profile:', id); }
    }
  }
  if (browser) await browser.close().catch(() => {});
  if (secondInstance && secondInstance.exitCode === null) secondInstance.kill();
  child.kill();
  await Promise.all(servers.map(closeServer));
  await removeSyntheticGatewayCredential();
}
