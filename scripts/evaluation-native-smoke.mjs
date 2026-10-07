import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

// Run only an explicitly built application. Never launch an installer, inspect
// existing ahaX data, or read a real Codex configuration / Windows credential.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const executable = path.resolve(process.argv[2] ?? 'src-tauri/target/release/ahax.exe');
assert.equal(path.basename(executable).toLowerCase(), 'ahax.exe', 'Pass the built ahaX application, never an installer.');
const { version } = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
await mkdir(path.join(root, 'artifacts'), { recursive: true });
const sandbox = await mkdtemp(path.join(root, 'artifacts', 'evaluation-native-smoke-'));
const codexHome = path.join(sandbox, 'codex');
const dataDirectory = path.join(sandbox, 'ahax-data');
const configPath = path.join(codexHome, 'config.toml');
const evaluationIndex = path.join(dataDirectory, 'evaluations', 'index.json');
const originalConfig = '# Isolated evaluation smoke only.\nmodel = "untouched-model"\n';
await mkdir(codexHome);
await mkdir(dataDirectory);
await writeFile(configPath, originalConfig);
await writeFile(path.join(dataDirectory, 'update-preferences.json'), '{"autoDownload":false}');

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
  const server = createServer();
  const port = await listen(server);
  await closeServer(server);
  return port;
}
const json = (response, status, value) => {
  response.writeHead(status, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify(value));
};
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
function within(base, target) {
  const relative = path.relative(path.resolve(base), path.resolve(target));
  assert(relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative), 'Test paths must remain inside the isolated directory.');
}

const svg = '<svg viewBox="0 0 240 140"><title>Pelican bicycle fixture</title><circle cx="50" cy="100" r="28"/><circle cx="180" cy="100" r="28"/><path d="M50 100L100 55L180 100Z" fill="none" stroke="black"/><path d="M100 55Q120 10 150 40L125 52Z" fill="white" stroke="black"/></svg>';
const unsafeSvg = '<svg viewBox="0 0 20 20" onload="globalThis.fixtureExecuted=true"><script>globalThis.fixtureExecuted=true</script><circle cx="10" cy="10" r="8"/></svg>';
const animation = `<!doctype html><html><head><style>@keyframes spin{to{transform:rotate(360deg)}}#wheel{transform-box:fill-box;transform-origin:center;animation:spin 2s linear infinite}</style></head><body><h1>Pelican cycling fixture</h1>${svg.replace('<circle cx="50"', '<circle id="wheel" cx="50"')}<script>globalThis.fixtureExecuted=true</script></body></html>`;
const answers = {
  candy: '{"answer":21}',
  pelican: animation,
  judgment: '{"J1":true,"J2":false,"J3":false,"J4":true,"J5":false,"J6":true}',
};
const channels = [
  { name: 'Evaluation subject fixture', model: 'evaluation-subject', key: `test-evaluation-${randomUUID()}`, judge: false },
  { name: 'Evaluation judge fixture', model: 'evaluation-judge', key: `test-judge-${randomUUID()}`, judge: true },
];
const report = { version, executable, sandbox, passed: false, requests: [], checks: [], cleanupErrors: [] };
const servers = [];
const createdProfiles = new Set();
const pageErrors = [];
let fixtureError;
let mode = 'normal';
let heldResponse;
let child;
let browser;
let page;
let environment;
let startupCount = 0;
const invoke = (command, args = {}) => page.evaluate(
  ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args },
);
const check = (name) => { report.checks.push(name); console.log(`PASS ${name}`); };
function assertNoSecrets(value) {
  const content = typeof value === 'string' ? value : JSON.stringify(value);
  for (const channel of channels) assert(!content.includes(channel.key), 'A synthetic credential leaked into persisted data or IPC results.');
}
function safeError(error) {
  let message = String(error?.stack ?? error);
  for (const channel of channels) message = message.replaceAll(channel.key, '[REDACTED]');
  return message;
}
async function until(predicate, label, timeout = 20000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (fixtureError) throw fixtureError;
    const result = await predicate();
    if (result) return result;
    await delay(50);
  }
  throw new Error(`Timed out: ${label}`);
}
async function finishedRun(id, timeout = 20000) {
  const dashboard = await until(async () => {
    const current = await invoke('get_evaluation_dashboard');
    if (!current.active && current.error) throw new Error(`Evaluation storage failed: ${current.error}`);
    return !current.active && current.history.some((item) => item.id === id) ? current : null;
  }, `evaluation completion ${id}`, timeout);
  const run = await invoke('get_evaluation_run', { runId: id });
  assert.equal(dashboard.history.find((item) => item.id === id).status, run.status);
  assertNoSecrets(run);
  return run;
}
async function startRun(plan) {
  const dashboard = await invoke('start_evaluation', { plan });
  const id = dashboard.active?.id ?? dashboard.history[0]?.id;
  assert(uuid.test(id), 'Start must return a native run ID.');
  return id;
}

async function verifyEvaluationUi(run, exported) {
  const screenshots = path.join(root, 'artifacts', 'screenshots');
  await mkdir(screenshots, { recursive: true });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '评测', exact: true }).click();
  await page.getByRole('heading', { name: '单次检测', exact: true }).waitFor({ state: 'visible' });
  assert.equal(await page.getByRole('navigation', { name: '评测子导航' }).getByRole('button', { name: '单次检测', exact: true }).getAttribute('aria-current'), 'page');
  assert.equal(await page.getByRole('button', { name: /暂停动画|播放动画/ }).count(), 0);
  const card = page.getByTestId('pelican-card');
  await card.waitFor({ state: 'visible' });
  assert.equal(await card.count(), 1);
  await card.locator('.artifact-preview[data-ready="true"]').waitFor();
  const drawing = page.frameLocator('iframe').first();
  await drawing.locator('#wheel').waitFor();
  const before = await drawing.locator('#wheel').evaluate((element) => getComputedStyle(element).transform);
  await delay(150);
  assert.notEqual(await drawing.locator('#wheel').evaluate((element) => getComputedStyle(element).transform), before);
  assert.equal(await page.evaluate(() => globalThis.fixtureExecuted), undefined);
  await page.evaluate(() => document.fonts.ready.then(() => true));
  const evaluationScreenshot = path.join(screenshots, `ahax-native-${version}-gallery.png`);
  await page.screenshot({ path: evaluationScreenshot, animations: 'disabled' });
  await card.getByRole('button', { name: '查看 evaluation-subject 鹈鹕动画 结果', exact: true }).click();
  const detail = page.getByRole('dialog', { name: '鹈鹕动画', exact: true });
  await detail.waitFor();
  await detail.getByRole('tab', { name: '原文', exact: true }).click();
  assert.equal(await detail.locator('pre').innerText(), run.results.find((result) => result.caseId === 'pelican').output);
  await detail.getByRole('tab', { name: '题目', exact: true }).click();
  assert.equal(await detail.locator('pre').innerText(), run.results.find((result) => result.caseId === 'pelican').prompt);
  await detail.getByRole('tab', { name: '结果', exact: true }).click();
  await detail.locator('.artifact-preview[data-ready="true"]').waitFor();
  const previousExport = await stat(exported.path);
  await detail.getByRole('button', { name: '导出评测报告', exact: true }).click();
  await detail.getByRole('status').filter({ hasText: '报告已保存' }).waitFor();
  const savePath = detail.getByRole('textbox', { name: '报告保存路径', exact: true });
  assert.equal(await savePath.inputValue(), exported.path);
  assert(await savePath.evaluate((input) => input.readOnly));
  await until(async () => (await stat(exported.path)).mtimeMs > previousExport.mtimeMs, 'native export file replacement');
  assert.deepEqual(JSON.parse(await readFile(exported.path, 'utf8')), run);
  await page.keyboard.press('Escape');
  await detail.waitFor({ state: 'detached' });
  await page.getByRole('tab', { name: '糖果推理', exact: true }).click();
  const manual = page.getByTestId('manual-results');
  await manual.waitFor();
  assert.equal(await manual.locator('.evaluation-answer-row').count(), 1);
  assert.equal(await page.getByTestId('candy-timeline').count(), 0);
  assert((await manual.innerText()).includes('Subject fixture'));
  await manual.locator('.evaluation-answer-row').click();
  const candy = page.getByRole('dialog', { name: '糖果推理', exact: true });
  await candy.waitFor();
  assert.equal(await candy.locator('.evaluation-answer').innerText(), answers.candy);
  await page.keyboard.press('Escape');
  await candy.waitFor({ state: 'detached' });
  const manualScreenshot = path.join(screenshots, `ahax-native-${version}-manual.png`);
  await page.screenshot({ path: manualScreenshot, animations: 'disabled' });
  await page.getByRole('button', { name: '记录', exact: true }).click();
  const history = page.getByRole('dialog', { name: '评测记录', exact: true });
  await history.waitFor();
  assert.equal(await history.locator('.evaluation-history-item').count(), 1);
  await history.getByRole('checkbox', { name: '选择全部评测记录', exact: true }).check();
  assert.equal(await history.getByRole('checkbox', { checked: true }).count(), 2);
  await history.getByRole('button', { name: '删除所选（1）', exact: true }).click();
  const removal = page.getByRole('dialog', { name: '删除这轮评测？', exact: true });
  await removal.waitFor();
  assert((await removal.innerText()).includes('已导出的文件会保留'));
  await removal.getByRole('button', { name: '取消', exact: true }).click();
  await removal.waitFor({ state: 'detached' });
  await page.keyboard.press('Escape');
  await history.waitFor({ state: 'detached' });
  assert.deepEqual(await invoke('get_evaluation_run', { runId: run.id }), run);
  report.ui = { evaluationScreenshot, manualScreenshot, exportPath: exported.path, animationChanged: true, nativeExportFeedback: true, historySelectionAndDeletionCancel: true };
  check('Packaged manual gallery animates HTML, result rows open exact answers, history selects records and native export confirms its path');
}

async function verifyScheduledUi(scheduledRun) {
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '评测', exact: true }).click();
  await page.getByRole('navigation', { name: '评测子导航' }).getByRole('button', { name: '定时评测', exact: true }).click();
  await page.getByRole('heading', { name: '定时评测', exact: true }).waitFor();
  await page.getByRole('heading', { name: '计划已暂停', exact: true }).waitFor();
  const timeline = page.getByTestId('candy-timeline');
  await timeline.waitFor();
  assert.equal(await timeline.locator('.evaluation-timeline-card').count(), 1);
  assert.equal(await timeline.locator('.evaluation-time-block').count(), 48);
  assert.equal(await timeline.locator('.evaluation-time-block[data-state-value="passed"]').count(), 1);
  assert.equal(await page.getByTestId('manual-results').count(), 0);
  await timeline.locator('.evaluation-time-block[data-state-value="passed"]').click();
  const candy = page.getByRole('dialog', { name: '糖果推理', exact: true });
  await candy.waitFor();
  assert.equal(await candy.locator('.evaluation-answer').innerText(), scheduledRun.results[0].output);
  await candy.getByRole('button', { name: '删除本轮评测', exact: true }).click();
  const removal = page.getByRole('dialog', { name: '删除这轮评测？', exact: true });
  await removal.waitFor();
  await removal.getByRole('button', { name: '取消', exact: true }).click();
  await removal.waitFor({ state: 'detached' });
  await page.keyboard.press('Escape');
  await candy.waitFor({ state: 'detached' });
  const timelineScreenshot = path.join(root, 'artifacts', 'screenshots', `ahax-native-${version}-timeline.png`);
  await page.screenshot({ path: timelineScreenshot, animations: 'disabled' });
  Object.assign(report.ui, { timelineScreenshot, scheduledScopeAndDeletionCancel: true });
  check('Packaged scheduled page shows only scheduled records in 48 slots and opens the matching native result');
}

async function assertDeleted(ids) {
  const current = await invoke('get_evaluation_dashboard');
  const activity = await invoke('get_evaluation_activity');
  for (const id of ids) {
    assert(!current.history.some((item) => item.id === id));
    assert(!activity.records.some((item) => item.runId === id));
    await assert.rejects(invoke('get_evaluation_run', { runId: id }));
    await assert.rejects(invoke('export_evaluation_run', { runId: id }));
    await assert.rejects(stat(path.join(dataDirectory, 'evaluations', 'runs', `${id}.json`)), { code: 'ENOENT' });
    await assert.rejects(stat(path.join(dataDirectory, 'evaluations', '.deleting', `${id}.json`)), { code: 'ENOENT' });
  }
}

async function stopApplication() {
  if (browser) await browser.close().catch(() => {});
  browser = undefined;
  page = undefined;
  const instance = child;
  child = undefined;
  if (instance && instance.exitCode === null) {
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Isolated ahaX did not exit.')), 10000);
      instance.once('exit', () => { clearTimeout(timeout); resolve(); });
      instance.kill();
    });
  }
}
async function startApplication(background = false) {
  assert(!child, 'Stop the isolated app before editing or restarting its state.');
  const debugPort = await unusedPort();
  startupCount += 1;
  const processEnvironment = {
    ...environment,
    WEBVIEW2_USER_DATA_FOLDER: path.join(sandbox, `webview-${startupCount}`),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort} --no-proxy-server`,
  };
  const instance = spawn(executable, background ? ['--background'] : [], {
    cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore', env: processEnvironment,
  });
  child = instance;
  let spawnFailure;
  instance.once('error', (error) => { spawnFailure = error; });
  await until(async () => {
    if (spawnFailure) throw spawnFailure;
    assert.equal(instance.exitCode, null, 'The explicitly isolated native process must remain alive.');
    try { return (await fetch(`http://127.0.0.1:${debugPort}/json/version`, { signal: AbortSignal.timeout(400) })).ok; }
    catch { return false; }
  }, 'isolated WebView2 startup', 30000);
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${debugPort}`);
  page = browser.contexts()[0].pages()[0] ?? await browser.contexts()[0].waitForEvent('page');
  page.setDefaultTimeout(15000);
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__?.invoke, null, { timeout: 15000 });
  page.on('pageerror', (error) => pageErrors.push(error.message));
  assert(!page.url().includes('127.0.0.1:1420'), 'Use the packaged UI, not the Vite preview.');
  const dashboard = await invoke('get_dashboard');
  assert.equal(path.resolve(dashboard.environment.configPath), configPath);
  assert.equal(dashboard.environment.desktopMode, true);
  const update = await invoke('get_update_status');
  assert.equal(update.currentVersion, version);
  assert.equal(update.autoDownload, false);
  if (background) assert.equal(await invoke('plugin:window|is_visible', { label: 'main' }), false);
  return dashboard;
}
async function deleteSyntheticCredential(id) {
  assert(uuid.test(id));
  await new Promise((resolve, reject) => {
    const command = spawn(path.join(process.env.SystemRoot, 'System32', 'cmdkey.exe'), [`/delete:ahaX/connection/${id}`], {
      windowsHide: true, stdio: 'ignore', env: environment,
    });
    command.once('error', reject);
    command.once('close', (code) => code === 0 ? resolve() : reject(new Error(`Synthetic credential cleanup failed for ${id}.`)));
  });
}
async function cleanupGatewayCredential() {
  let stored;
  try { stored = (await readFile(path.join(dataDirectory, 'gateway-credential-id'), 'utf8')).trim(); }
  catch (error) { if (error.code === 'ENOENT') return; throw error; }
  const digest = createHash('sha256').update(dataDirectory.toLowerCase()).digest('hex').slice(0, 32);
  const expected = `${digest.slice(0, 8)}-${digest.slice(8, 12)}-${digest.slice(12, 16)}-${digest.slice(16, 20)}-${digest.slice(20)}`;
  assert.equal(stored, expected, 'Delete only the gateway credential derived from this test directory.');
  await deleteSyntheticCredential(stored);
}
async function checkPersistedSecrets(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    within(dataDirectory, file);
    if (entry.isDirectory()) await checkPersistedSecrets(file);
    else if (entry.isFile() && entry.name.endsWith('.json')) assertNoSecrets(await readFile(file, 'utf8'));
  }
}

try {
  // Prevent background updater traffic from leaving localhost. Channel requests
  // bypass this synthetic denial proxy via NO_PROXY; no public API is invoked.
  const denyProxy = createServer((_request, response) => json(response, 502, { error: 'Local-only smoke.' }));
  denyProxy.on('connect', (_request, socket) => socket.end('HTTP/1.1 502 Bad Gateway\r\n\r\n'));
  const proxyUrl = `http://127.0.0.1:${await listen(denyProxy)}`;
  servers.push(denyProxy);
  for (const channel of channels) {
    const server = createServer(async (request, response) => {
      try {
        assert.equal(request.method, 'POST', 'Evaluation must not discover metadata or execute tools.');
        assert.equal(request.url, '/v1/responses');
        assert.equal(request.headers.authorization, `Bearer ${channel.key}`, 'Only this fixture channel may receive its synthetic key.');
        let source = '';
        for await (const chunk of request) source += chunk;
        const body = JSON.parse(source);
        assert.equal(body.model, channel.model);
        assert.equal(body.stream, false);
        assert.equal(body.store, false);
        assert.equal(body.tools, undefined);
        assert.equal(typeof body.input, 'string');
        const isJudge = body.input.startsWith('你是评审');
        assert.equal(isJudge, channel.judge);
        const caseId = body.input.includes('J1') ? 'judgment' : body.input.includes('鹈鹕') ? 'pelican' : 'candy';
        assert.equal(body.max_output_tokens, caseId === 'pelican' && !isJudge ? 32768 : 8192);
        const requestRecord = { phase: mode, judge: isJudge, caseId, model: body.model, effort: body.reasoning?.effort ?? null, disconnected: false };
        report.requests.push(requestRecord);
        assert.deepEqual(body.reasoning, channel.judge ? undefined : { effort: 'high' });
        if ((mode === 'cancel' || mode === 'timeout') && !isJudge) {
          heldResponse = response;
          response.once('close', () => { requestRecord.disconnected = true; });
          return; // Hold the first request until native cancellation aborts it.
        }
        const output = isJudge
          ? JSON.stringify({ score: 92, explanation: `Synthetic text review; credential ${channel.key}` })
          : mode === 'unsafe-svg' ? unsafeSvg : answers[caseId];
        json(response, 200, {
          id: `resp_${randomUUID()}`, object: 'response', status: 'completed', model: body.model,
          output: [{ type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: output }] }],
          usage: { input_tokens: 31, output_tokens: 17, total_tokens: 48 },
        });
      } catch (error) {
        fixtureError ??= error;
        if (!response.headersSent) json(response, 500, { error: 'Local fixture assertion failed.' });
        else response.end();
      }
    });
    channel.baseUrl = `http://127.0.0.1:${await listen(server)}/v1`;
    servers.push(server);
  }
  const gatewayPort = await unusedPort();
  await writeFile(path.join(dataDirectory, 'connections.json'), JSON.stringify({
    profiles: [], backups: [], settings: { providerName: 'ahaX', gatewayPort, autoRefresh: false, refreshMinutes: 15 },
  }));
  environment = {
    ...process.env,
    PATH: `${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`,
    CODEX_HOME: codexHome, AHAX_DATA_DIR: dataDirectory,
    HTTP_PROXY: proxyUrl, HTTPS_PROXY: proxyUrl, ALL_PROXY: proxyUrl,
    http_proxy: proxyUrl, https_proxy: proxyUrl, all_proxy: proxyUrl,
    NO_PROXY: '127.0.0.1,localhost', no_proxy: '127.0.0.1,localhost',
  };
  for (const key of Object.keys(environment)) {
    if (/(?:API[_-]?KEY|TOKEN|PASSWORD|SECRET)/i.test(key)) delete environment[key];
  }
  delete environment.OPENAI_BASE_URL;
  const initial = await startApplication();
  assert.equal(initial.profiles.length, 0);
  const empty = await invoke('get_evaluation_dashboard');
  assert.equal(empty.history.length, 0);
  assert.equal(empty.active, null);
  assert.equal(empty.plan.requestTimeoutSeconds, 300);
  assert.equal(empty.plan.intervalMinutes, 30);
  assert.deepEqual(empty.cases.map((item) => item.id).sort(), ['candy', 'judgment', 'pelican']);
  for (const channel of channels) {
    const saved = await invoke('save_profile', { input: {
      name: channel.name, baseUrl: channel.baseUrl, apiKey: channel.key, model: channel.model,
      models: [{ id: channel.model, alias: channel.judge ? 'Judge fixture' : 'Subject fixture', enabled: true, reasoningEfforts: ['high'], defaultReasoningEffort: 'high' }],
      balanceConfig: { mode: 'disabled' },
    } });
    assert(uuid.test(saved.id));
    createdProfiles.add(saved.id);
    channel.profileId = saved.id;
    assert.equal(saved.keyStored, true);
    assertNoSecrets(saved);
  }
  const subject = { profileId: channels[0].profileId, modelId: channels[0].model, reasoningEffort: 'high' };
  const judge = { profileId: channels[1].profileId, modelId: channels[1].model, reasoningEffort: null };
  const plan = { targets: [subject], cases: ['candy', 'pelican', 'judgment'], judge, scheduleEnabled: false, intervalHours: 1, intervalMinutes: 30, requestTimeoutSeconds: 125 };
  const legacyPlan = { ...plan };
  delete legacyPlan.requestTimeoutSeconds;
  delete legacyPlan.intervalMinutes;
  const migrated = await invoke('save_evaluation_plan', { plan: legacyPlan });
  assert.equal(migrated.plan.requestTimeoutSeconds, 120);
  assert.equal(migrated.plan.intervalMinutes, null);
  assert.equal(migrated.plan.intervalHours, 1);
  const saved = await invoke('save_evaluation_plan', { plan });
  assert.deepEqual(saved.plan, plan);
  assert.deepEqual((await invoke('get_evaluation_dashboard')).plan, plan);
  assert.deepEqual(JSON.parse(await readFile(evaluationIndex, 'utf8')).plan, plan);
  assert.equal(saved.nextRunAt, null);
  check('Packaged IPC saves and reads an isolated evaluation plan');

  const run = await finishedRun(await startRun(plan));
  assert.equal(run.plan.requestTimeoutSeconds, 125);
  assert.equal(run.status, 'completed');
  assert.equal(run.trigger, 'manual');
  assert.equal(run.totalCases, 3);
  assert.equal(run.completedCases, 3);
  assert.equal(run.results.length, 3);
  assert.equal(report.requests.length, 5);
  assert.deepEqual(report.requests.map((item) => item.judge), [false, true, false, false, true]);
  for (const result of run.results) {
    assert.equal(result.status, result.caseId === 'pelican' ? 'generated' : 'passed');
    assert.equal(result.score, result.caseId === 'pelican' ? null : 100);
    assert.equal(result.output, answers[result.caseId]);
    assert(result.checks.every((item) => item.passed));
    assert(result.prompt.length > 20);
    assert.equal(result.inputTokens, 31);
    assert.equal(result.outputTokens, 17);
    assert.equal(result.reasoningEffort, 'high');
    if (result.caseId === 'pelican') {
      assert.equal(result.artifactHtml, animation);
      assert.equal(result.judge, null);
      assert.deepEqual(result.checks, []);
    } else {
      assert.equal(result.safeSvg, null);
      assert.equal(result.judge.score, 92);
      assert.equal(result.judge.error, null);
      assert(result.judge.explanation.includes('[REDACTED]'));
    }
  }
  const activity = await invoke('get_evaluation_activity');
  assert.equal(activity.records.length, 3);
  assert.deepEqual(activity.records.map((record) => record.caseId), ['candy', 'pelican', 'judgment']);
  for (const record of activity.records) {
    assert.equal(record.runId, run.id);
    assert.equal(record.modelId, channels[0].model);
    assert.equal(record.createdAt, run.startedAt);
    assert.equal(record.hasArtifact, record.caseId === 'pelican');
    for (const field of ['output', 'prompt', 'artifactHtml', 'safeSvg', 'judge']) assert(!(field in record));
  }
  assertNoSecrets(activity);
  const exported = await invoke('export_evaluation_run', { runId: run.id });
  within(path.join(dataDirectory, 'evaluations', 'exports'), exported.path);
  assert.equal(path.basename(exported.path), exported.fileName);
  assert.deepEqual(JSON.parse(exported.content), run);
  assert.equal(await readFile(exported.path, 'utf8'), exported.content);
  assertNoSecrets(exported);
  report.manualRun = { id: run.id, status: run.status, cases: run.results.map((item) => ({ id: item.caseId, score: item.score, judgeScore: item.judge?.score ?? null })) };
  check('Three native cases, text-only judge, compact activity, raw outputs, history and JSON export');
  await verifyEvaluationUi(run, exported);

  mode = 'unsafe-svg';
  const unsafeRun = await finishedRun(await startRun({ ...plan, cases: ['pelican'], judge: null }));
  assert.equal(unsafeRun.results[0].status, 'generated');
  assert.equal(unsafeRun.results[0].score, null);
  assert.equal(unsafeRun.results[0].safeSvg, null);
  assert.equal(unsafeRun.results[0].output, unsafeSvg);
  assert.equal(unsafeRun.results[0].artifactHtml, unsafeSvg);
  assert.equal(await page.evaluate(() => globalThis.fixtureExecuted), undefined);
  check('Scripted artifacts remain intact without executing in the application document');

  const beforeInvalid = report.requests.length;
  await assert.rejects(startRun({ ...plan, targets: [{ ...subject, reasoningEffort: 'ultra' }] }));
  await assert.rejects(startRun({ ...plan, targets: [{ ...subject, reasoningEffort: 'max' }] }));
  for (const requestTimeoutSeconds of [29, 3601, 30.5]) {
    await assert.rejects(startRun({ ...plan, requestTimeoutSeconds }));
    await assert.rejects(invoke('save_evaluation_plan', { plan: { ...plan, requestTimeoutSeconds } }));
  }
  await assert.rejects(invoke('get_evaluation_run', { runId: '../outside' }));
  assert.equal(report.requests.length, beforeInvalid);
  check('Invalid API efforts and unsafe run IDs fail before network access');

  mode = 'cancel';
  const beforeCancel = report.requests.length;
  const cancelledId = await startRun({ ...plan, requestTimeoutSeconds: 1800 });
  await until(() => heldResponse, 'first cancellable Responses request');
  const activeIndexBeforeWake = await readFile(evaluationIndex, 'utf8');
  const activeReportPath = path.join(dataDirectory, 'evaluations', 'runs', `${cancelledId}.json`);
  const activeReportBeforeWake = await readFile(activeReportPath, 'utf8');
  for (const args of [[], ['--background']]) {
    await new Promise((resolve, reject) => {
      const waking = spawn(executable, args, { cwd: path.dirname(executable), windowsHide: true, stdio: 'ignore', env: environment });
      const deadline = setTimeout(() => { waking.kill(); reject(new Error('Second instance failed to exit.')); }, 10000);
      waking.once('error', (error) => { clearTimeout(deadline); reject(error); });
      waking.once('close', (code) => { clearTimeout(deadline); code === 0 ? resolve() : reject(new Error(`Second instance exited with ${code}.`)); });
    });
    assert.equal(await readFile(evaluationIndex, 'utf8'), activeIndexBeforeWake, 'A second desktop launch must not recover the live evaluation as interrupted.');
    assert.equal(await readFile(activeReportPath, 'utf8'), activeReportBeforeWake);
    assert.equal((await invoke('get_evaluation_dashboard')).active.id, cancelledId);
    assert.equal(report.requests.length, beforeCancel + 1);
  }
  check('Second foreground and background launches leave the running evaluation and persisted files unchanged');
  await assert.rejects(startRun(plan));
  const activeIndex = await readFile(evaluationIndex);
  const activeFeed = await invoke('get_evaluation_activity');
  await assert.rejects(invoke('delete_evaluation_runs', { runIds: [run.id, cancelledId] }), /正在运行/);
  assert.deepEqual(await readFile(evaluationIndex), activeIndex, 'A batch containing an active run must not remove the completed companion.');
  assert.deepEqual(await invoke('get_evaluation_activity'), activeFeed);
  assert.deepEqual(await invoke('get_evaluation_run', { runId: run.id }), run);
  assert.equal((await invoke('get_evaluation_dashboard')).active.id, cancelledId);
  check('Native deletion rejects an active run and leaves the entire mixed batch unchanged');
  await invoke('cancel_evaluation', { runId: cancelledId });
  const cancelled = await finishedRun(cancelledId);
  assert.equal(cancelled.status, 'cancelled');
  assert.equal(cancelled.completedCases, 0);
  assert.equal(cancelled.results.length, 1);
  assert.equal(cancelled.results[0].status, 'cancelled');
  await until(() => report.requests.at(-1).disconnected, 'cancelled transport disconnect');
  await delay(500);
  assert.equal(report.requests.length, beforeCancel + 1, 'Cancellation must stop judge and later task requests.');
  heldResponse = undefined;
  check('Native cancellation aborts the active request and prevents all subsequent requests');

  mode = 'timeout';
  const beforeTimeout = report.requests.length;
  const timeoutStarted = Date.now();
  const expired = await finishedRun(await startRun({ ...plan, cases: ['candy'], judge: null, requestTimeoutSeconds: 30 }), 45000);
  const timeoutElapsed = Date.now() - timeoutStarted;
  assert.equal(expired.results[0].status, 'error');
  assert.equal(expired.results[0].score, null);
  assert.match(expired.results[0].error, /超时.*30/);
  assert(timeoutElapsed >= 29000 && timeoutElapsed < 45000, 'Use the configured native timeout, not the former fixed 120-second limit.');
  assert(expired.results[0].elapsedMs >= 29000 && expired.results[0].elapsedMs < 45000, 'Timed-out results must report the actual waiting time.');
  await until(() => report.requests.at(-1).disconnected, 'timed-out transport disconnect');
  assert.equal(report.requests.length, beforeTimeout + 1, 'A timed-out request must not be retried.');
  report.timeout = { configuredSeconds: 30, elapsedMs: timeoutElapsed, error: expired.results[0].error };
  heldResponse = undefined;
  check('Custom request deadlines survive IPC and persistence, legacy plans retain 120 seconds, and timed-out requests are not retried');

  mode = 'scheduled';
  const scheduledPlan = { ...plan, cases: ['candy'], judge: null, scheduleEnabled: true };
  const scheduled = await invoke('save_evaluation_plan', { plan: scheduledPlan });
  assert(Date.parse(scheduled.nextRunAt) > Date.now());
  const disabled = await invoke('save_evaluation_plan', { plan: { ...scheduledPlan, scheduleEnabled: false } });
  assert.equal(disabled.nextRunAt, null);
  assert.equal(JSON.parse(await readFile(evaluationIndex, 'utf8')).plan.scheduleEnabled, false);
  await invoke('save_evaluation_plan', { plan: scheduledPlan });
  await stopApplication();
  // The only simulated clock change is to our own persisted index while its
  // owning process is stopped. No clock, user setting or live file is changed.
  within(sandbox, evaluationIndex);
  const due = JSON.parse(await readFile(evaluationIndex, 'utf8'));
  assert.equal(due.activeRunId, null);
  assert.deepEqual(due.plan, scheduledPlan);
  due.nextRunAt = new Date(Date.now() - 86400000).toISOString();
  await writeFile(evaluationIndex, JSON.stringify(due, null, 2));
  const beforeSchedule = report.requests.length;
  await startApplication(true);
  const scheduledHistory = await until(async () => {
    const current = await invoke('get_evaluation_dashboard');
    const completed = current.history.find((item) => item.trigger === 'scheduled' && item.status === 'completed');
    return !current.active && completed ? completed : null;
  }, 'background scheduled evaluation');
  const scheduledRun = await invoke('get_evaluation_run', { runId: scheduledHistory.id });
  assert.equal(scheduledRun.plan.requestTimeoutSeconds, 125);
  assert.equal(scheduledRun.results[0].score, 100);
  assert.equal(report.requests.length, beforeSchedule + 1);
  const afterSchedule = await invoke('get_evaluation_dashboard');
  assert(Date.parse(afterSchedule.nextRunAt) > Date.now());
  assert.deepEqual(afterSchedule.plan, scheduledPlan);
  await stopApplication();
  await startApplication(true);
  await delay(500);
  const resumed = await invoke('get_evaluation_dashboard');
  assert.equal(resumed.nextRunAt, afterSchedule.nextRunAt);
  assert.equal(resumed.history.filter((item) => item.trigger === 'scheduled').length, 1);
  assert.equal(report.requests.length, beforeSchedule + 1, 'Restart must not replay a claimed scheduled occurrence.');
  assert.deepEqual(await invoke('get_evaluation_run', { runId: run.id }), run);
  const stoppedSchedule = await invoke('save_evaluation_plan', { plan: { ...scheduledPlan, scheduleEnabled: false } });
  assert.equal(stoppedSchedule.nextRunAt, null);
  assert.equal(JSON.parse(await readFile(evaluationIndex, 'utf8')).plan.scheduleEnabled, false);
  report.scheduledRun = { id: scheduledRun.id, trigger: scheduledRun.trigger, status: scheduledRun.status };
  check('Persisted schedule runs once in the background, advances its deadline and survives restart without replay');

  await stopApplication();
  await startApplication();
  await verifyScheduledUi(scheduledRun);
  const retainedPlan = (await invoke('get_evaluation_dashboard')).plan;
  const beforeDeletionRequests = report.requests.length;
  const beforeDeletionIndex = await readFile(evaluationIndex);
  const beforeDeletionFeed = await invoke('get_evaluation_activity');
  const retainedExports = await readFile(exported.path);
  for (const invalidBatch of [[run.id, randomUUID()], [run.id, '../outside'], []]) {
    await assert.rejects(invoke('delete_evaluation_runs', { runIds: invalidBatch }));
    assert.deepEqual(await readFile(evaluationIndex), beforeDeletionIndex, 'Invalid batches must leave the persisted index byte-identical.');
    assert.deepEqual(await invoke('get_evaluation_activity'), beforeDeletionFeed);
    assert.deepEqual(await invoke('get_evaluation_run', { runId: run.id }), run);
  }
  check('Unknown, unsafe and empty native deletion batches fail atomically without removing valid records');
  const singleRemoved = await invoke('delete_evaluation_runs', { runIds: [unsafeRun.id] });
  assert.equal(singleRemoved.history.length, 4);
  assert.deepEqual(singleRemoved.plan, retainedPlan);
  await assertDeleted([unsafeRun.id]);
  assert.deepEqual(await invoke('get_evaluation_run', { runId: scheduledRun.id }), scheduledRun);
  check('Single native deletion removes the report, artwork and activity while preserving other runs and the plan');
  const batchIds = [run.id, cancelled.id, expired.id];
  const batchRemoved = await invoke('delete_evaluation_runs', { runIds: batchIds });
  assert.deepEqual(batchRemoved.history.map((item) => item.id), [scheduledRun.id]);
  assert.deepEqual(batchRemoved.plan, retainedPlan);
  await assertDeleted(batchIds);
  const retainedFeed = await invoke('get_evaluation_activity');
  assert.equal(retainedFeed.records.length, 1);
  assert.equal(retainedFeed.records[0].runId, scheduledRun.id);
  assert.deepEqual(await readFile(exported.path), retainedExports, 'An explicit exported JSON file must survive deletion of its source report.');
  assert.equal(report.requests.length, beforeDeletionRequests, 'Deleting records must not call any model or trigger a schedule.');
  check('Batch native deletion clears compact activity and report files while retaining explicit exports and scheduled history');
  await stopApplication();
  await startApplication();
  await assertDeleted([unsafeRun.id, ...batchIds]);
  const afterDeletionRestart = await invoke('get_evaluation_dashboard');
  assert.deepEqual(afterDeletionRestart.history.map((item) => item.id), [scheduledRun.id]);
  assert.deepEqual(afterDeletionRestart.plan, retainedPlan);
  assert.equal(afterDeletionRestart.nextRunAt, null);
  assert.deepEqual(await invoke('get_evaluation_activity'), retainedFeed);
  assert.deepEqual(await readFile(exported.path), retainedExports);
  await delay(500);
  assert.equal(report.requests.length, beforeDeletionRequests);
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '评测', exact: true }).click();
  await page.getByRole('heading', { name: '开始一次鹈鹕动画', exact: true }).waitFor();
  assert.equal(await page.getByTestId('pelican-card').count(), 0);
  await page.getByRole('button', { name: '记录', exact: true }).click();
  const emptyHistory = page.getByRole('dialog', { name: '评测记录', exact: true });
  await emptyHistory.getByText('这里还没有评测记录。', { exact: true }).waitFor();
  assert.equal(await emptyHistory.locator('.evaluation-history-item').count(), 0);
  await page.keyboard.press('Escape');
  await emptyHistory.waitFor({ state: 'detached' });
  report.deletion = { singleId: unsafeRun.id, batchIds, retainedScheduledId: scheduledRun.id, exportedFileRetained: true, atomicInvalidBatch: true, activeRunRejected: true, restartPreserved: true };
  check('Deleted native records stay absent from disk, activity, UI and exports after restart without replay');

  assert.equal(await readFile(configPath, 'utf8'), originalConfig);
  await checkPersistedSecrets(dataDirectory);
  assert.deepEqual(pageErrors, []);
  assert.equal(fixtureError, undefined);
  report.passed = true;
  check('Codex configuration remains byte-identical and no fixture keys escape credential storage');
} catch (error) {
  report.error = safeError(error);
  assertNoSecrets(report.error);
} finally {
  if (page) {
    try {
      const current = await invoke('get_evaluation_dashboard');
      if (current.active) {
        await invoke('cancel_evaluation', { runId: current.active.id });
        await finishedRun(current.active.id);
      }
      await invoke('save_evaluation_plan', { plan: { ...current.plan, scheduleEnabled: false } });
    } catch (error) { report.cleanupErrors.push(`Stop isolated evaluation: ${safeError(error)}`); }
    for (const id of [...createdProfiles]) {
      try { await invoke('delete_profile', { id }); createdProfiles.delete(id); }
      catch { /* Remove only this run's known synthetic credential after exit. */ }
    }
  }
  heldResponse?.destroy();
  await stopApplication().catch((error) => report.cleanupErrors.push(safeError(error)));
  // A save may persist a profile before a later gateway refresh reports an
  // error. Recover only matching fixture IDs from our initially empty store.
  try {
    const isolated = JSON.parse(await readFile(path.join(dataDirectory, 'connections.json'), 'utf8'));
    for (const profile of isolated.profiles) {
      if (uuid.test(profile.id) && channels.some((channel) => channel.name === profile.name && channel.baseUrl === profile.baseUrl && channel.model === profile.model)) {
        createdProfiles.add(profile.id);
      }
    }
  } catch (error) { report.cleanupErrors.push(`Read isolated cleanup metadata: ${safeError(error)}`); }
  for (const id of createdProfiles) {
    await deleteSyntheticCredential(id).catch((error) => report.cleanupErrors.push(safeError(error)));
  }
  await cleanupGatewayCredential().catch((error) => report.cleanupErrors.push(safeError(error)));
  await Promise.all(servers.map(closeServer));
  if (report.cleanupErrors.length) report.passed = false;
  assertNoSecrets(report);
  await writeFile(path.join(sandbox, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ passed: report.passed, requests: report.requests.length, checks: report.checks, error: report.error, cleanupErrors: report.cleanupErrors, report: path.join(sandbox, 'report.json') }, null, 2));
}
if (!report.passed) process.exitCode = 1;
