// Exercise the real Codex app-server with an isolated home and a local, unbilled fixture.
// No installed ahaX state, Windows credentials, user Codex config, or remote API is used.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const executable = path.resolve(process.argv[2] ?? process.env.AHAX_CODEX_SMOKE_EXECUTABLE ?? 'C:/nvm4w/nodejs/node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe');
const runDirectory = path.join(root, 'artifacts', `official-codex-smoke-${Date.now()}`);
const codexHome = path.join(runDirectory, 'codex-home');
const workspace = path.join(runDirectory, 'workspace');
await mkdir(codexHome, { recursive: true });
await mkdir(workspace, { recursive: true });

const report = { executable, runDirectory, fixtureKind: 'Schema-equivalent ahaX catalog and provider fixture; local credential helper and upstream, not an end-to-end ahaX process test.', version: '', passed: false, models: [], requests: [], helperInvocations: 0 };
const environment = { ...process.env, CODEX_HOME: codexHome };
for (const key of Object.keys(environment)) {
  if (/(?:API[_-]?KEY|TOKEN|PASSWORD|SECRET)/i.test(key)) delete environment[key];
}
delete environment.OPENAI_BASE_URL;
const version = spawnSync(executable, ['--version'], { env: environment, encoding: 'utf8', windowsHide: true });
if (version.error || version.status !== 0) throw version.error ?? new Error('Cannot run the selected Codex binary.');
report.version = version.stdout.trim();

const localToken = `ahax-smoke-${randomUUID()}`;
const helper = path.join(runDirectory, 'fixture-credential.cjs');
const helperLog = path.join(runDirectory, 'credential-invocations.txt');
await writeFile(helper, `require('fs').appendFileSync(${JSON.stringify(helperLog)}, 'invoked\\n');process.stdout.write(${JSON.stringify(localToken)});\n`);

const channels = [
  { id: 'channel-a', label: '渠道甲（gpt-5.4）', model: 'gpt-5.4', efforts: ['none', 'low', 'medium', 'high', 'xhigh'], defaultEffort: 'none' },
  { id: 'channel-b', label: '渠道乙（gpt-5-pro）', model: 'gpt-5-pro', efforts: ['high'], defaultEffort: 'high' },
  { id: 'channel-c', label: '渠道丙（chat-basic）', model: 'chat-basic', efforts: [], defaultEffort: null },
  { id: 'channel-d', label: '渠道丁（gpt-6.1-sol）', model: 'gpt-6.1-sol', efforts: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'], defaultEffort: 'medium', ultraEffort: 'xhigh' },
  { id: 'channel-e', label: '渠道戊（gpt-5.6-sol）', model: 'gpt-5.6-sol', efforts: ['none', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'], defaultEffort: 'medium', ultraEffort: 'max' },
  { id: 'channel-f', label: '渠道己（gpt-6-astra）', model: 'gpt-6-astra', efforts: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'], defaultEffort: 'low', ultraEffort: 'xhigh' },
  { id: 'channel-g', label: '渠道庚（gpt-6-luna）', model: 'gpt-6-luna', efforts: ['none', 'low', 'medium', 'high', 'xhigh', 'max'], defaultEffort: 'medium' },
];
// Test the complete native Ultra runtime against the exact supported release.
// Accepting its enum alone does not prove proactive multi-agent support.
const versionParts = report.version.match(/(\d+)\.(\d+)\.(\d+)/);
assert.ok(versionParts && (Number(versionParts[1]) > 0 || Number(versionParts[2]) >= 160), 'This complete native Ultra fixture requires Codex >=0.160. Pass an isolated official binary as argv[2] or AHAX_CODEX_SMOKE_EXECUTABLE.');
const instructions = await readFile(path.join(root, 'src-tauri/resources/official-codex-fallback-prompt.md'), 'utf8');
const models = channels.map((channel, index) => ({
  slug: `ahax-${createHash('sha256').update(`${channel.id}\0${channel.model}`).digest('hex')}`,
  display_name: channel.label,
  description: `${channel.id} · ${channel.model}`,
  default_reasoning_level: channel.defaultEffort,
  supported_reasoning_levels: channel.efforts.map(effort => ({ effort, description: `Fixture ${effort}` })),
  multi_agent_version: channel.ultraEffort ? 'v2' : null,
  multi_agent_reasoning_effort: channel.ultraEffort ?? null,
  default_reasoning_summary: 'none',
  shell_type: 'unified_exec',
  visibility: 'list',
  supported_in_api: true,
  priority: index,
  availability_nux: null,
  upgrade: null,
  support_verbosity: false,
  default_verbosity: null,
  apply_patch_tool_type: null,
  truncation_policy: { mode: 'bytes', limit: 10000 },
  experimental_supported_tools: [],
  input_modalities: ['text'],
  supports_reasoning_summary_parameter: false,
  supports_reasoning_summaries: channel.efforts.length > 0,
  supports_parallel_tool_calls: false,
  base_instructions: instructions,
}));
const catalogPath = path.join(runDirectory, 'model-catalog.json');
await writeFile(catalogPath, JSON.stringify({ models }, null, 2));

const server = createServer(async (request, response) => {
  let source = '';
  for await (const chunk of request) source += chunk;
  if (request.method !== 'POST' || request.url !== '/v1/responses') {
    response.writeHead(404, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ error: { message: 'Local smoke fixture only accepts Responses.' } }));
    return;
  }
  try {
    assert.equal(request.headers.authorization, `Bearer ${localToken}`);
    const payload = JSON.parse(source);
    assert.ok(models.some(model => model.slug === payload.model));
    const requestText = JSON.stringify({ instructions: payload.instructions, input: payload.input });
    const toolsText = JSON.stringify(payload.tools ?? []);
    report.requests.push({
      model: payload.model, stream: payload.stream, authenticated: true,
      reasoning: payload.reasoning ?? null,
      instructionsLength: typeof payload.instructions === 'string' ? payload.instructions.length : null,
      proactiveDelegation: /Proactive multi-agent delegation is active/i.test(requestText),
      hasSpawnAgent: /"name":"(?:[^"\s]*[._])?spawn_agent"/.test(toolsText),
      hasFollowupTask: /"name":"(?:[^"\s]*[._])?followup_task"/.test(toolsText),
    });
    const id = `resp_${randomUUID().replaceAll('-', '')}`;
    const item = { id: `msg_${randomUUID().replaceAll('-', '')}`, type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'SMOKE_OK', annotations: [] }] };
    const completed = { id, object: 'response', model: payload.model, status: 'completed', output: [item], usage: { input_tokens: 10, output_tokens: 3, total_tokens: 13, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } } };
    response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
    for (const event of [
      { type: 'response.created', sequence_number: 0, response: { ...completed, status: 'in_progress', output: [] } },
      { type: 'response.output_item.done', sequence_number: 1, output_index: 0, item },
      { type: 'response.completed', sequence_number: 2, response: completed },
    ]) response.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
    response.end();
  } catch (error) {
    report.fixtureError = String(error);
    response.writeHead(500, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ error: { message: 'Local fixture assertion failed.' } }));
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const port = server.address().port;
const quote = value => JSON.stringify(value);
await writeFile(path.join(codexHome, 'config.toml'), [
  `model_provider = "ahaX"`,
  `model = ${quote(models[0].slug)}`,
  `model_catalog_json = ${quote(catalogPath)}`,
  `[analytics]`, `enabled = false`,
  `[model_providers.ahaX]`,
  `name = "ahaX"`,
  `base_url = "http://127.0.0.1:${port}/v1"`,
  `wire_api = "responses"`,
  `supports_websockets = false`,
  `request_max_retries = 0`,
  `stream_max_retries = 0`,
  `[model_providers.ahaX.auth]`,
  `command = ${quote(process.execPath)}`,
  `args = [${quote(helper)}]`,
  `timeout_ms = 35000`,
  '',
].join('\n'));

const child = spawn(executable, ['app-server', '--stdio', '--strict-config'], { cwd: workspace, env: environment, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
let outputBuffer = '';
let stderr = '';
let nextId = 0;
const pending = new Map();
const notifications = [];
child.stderr.on('data', chunk => { stderr += chunk; });
child.stdout.setEncoding('utf8');
child.stdout.on('data', chunk => {
  outputBuffer += chunk;
  let end;
  while ((end = outputBuffer.indexOf('\n')) !== -1) {
    const line = outputBuffer.slice(0, end).trim(); outputBuffer = outputBuffer.slice(end + 1);
    if (!line) continue;
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    if (message.id != null && pending.has(message.id)) {
      const waiter = pending.get(message.id); pending.delete(message.id); clearTimeout(waiter.timer);
      if (message.error) waiter.reject(new Error(JSON.stringify(message.error))); else waiter.resolve(message.result);
    } else if (message.method) notifications.push(message);
  }
});
child.on('error', error => { for (const waiter of pending.values()) { clearTimeout(waiter.timer); waiter.reject(error); } pending.clear(); });
child.on('exit', code => { for (const waiter of pending.values()) { clearTimeout(waiter.timer); waiter.reject(new Error(`Codex exited with ${code}: ${stderr.slice(-1500)}`)); } pending.clear(); });
function rpc(method, params) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`RPC timed out: ${method}`)); }, 40000);
    pending.set(id, { resolve, reject, timer });
    child.stdin.write(`${JSON.stringify({ id, method, params })}\n`);
  });
}
async function completedTurn(id) {
  const deadline = Date.now() + 40000;
  while (Date.now() < deadline) {
    const message = notifications.find(event => event.method === 'turn/completed' && event.params.turn?.id === id);
    if (message) return message.params.turn;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  throw new Error(`Turn did not complete: ${id}`);
}

try {
  report.initialize = await rpc('initialize', { clientInfo: { name: 'ahax_native_smoke', title: 'ahaX native integration smoke', version: '0.5.0' }, capabilities: { experimentalApi: true } });
  child.stdin.write(`${JSON.stringify({ method: 'initialized', params: {} })}\n`);
  const available = await rpc('model/list', { limit: 100, includeHidden: true });
  report.models = available.data.map(model => ({ id: model.id, model: model.model, displayName: model.displayName, supportedReasoningEfforts: model.supportedReasoningEfforts, defaultReasoningEffort: model.defaultReasoningEffort }));
  assert.equal(report.models.length, channels.length);
  for (const model of models) {
    const actual = report.models.find(item => item.model === model.slug && item.displayName === model.display_name);
    assert.ok(actual);
    assert.deepEqual(actual.supportedReasoningEfforts.map(preset => preset.reasoningEffort), model.supported_reasoning_levels.map(preset => preset.effort));
    assert.equal(actual.defaultReasoningEffort, model.default_reasoning_level ?? 'none');
  }
  report.account = await rpc('account/read', { refreshToken: false });
  assert.equal(report.account.requiresOpenaiAuth, false);
  assert.equal(report.account.account, null);
  for (const [index, model] of models.entries()) {
    // First prove that no global fixed effort is needed. Then exercise every
    // effort exposed by the native picker, including the highest setting.
    for (const effort of [undefined, ...channels[index].efforts]) {
      const thread = await rpc('thread/start', { model: model.slug, modelProvider: 'ahaX', cwd: workspace, approvalPolicy: 'never', sandbox: 'read-only', ephemeral: true });
      const turn = await rpc('turn/start', { threadId: thread.thread.id, effort, input: [{ type: 'text', text: 'Reply SMOKE_OK without tools.' }] });
      const finished = await completedTurn(turn.turn.id);
      assert.equal(finished.status, 'completed', JSON.stringify(finished));
      const request = report.requests.at(-1);
      assert.equal(request.model, model.slug);
      const expectedEffort = effort === 'ultra' ? channels[index].ultraEffort : effort ?? model.default_reasoning_level;
      assert.equal(request.reasoning?.effort ?? null, expectedEffort);
      assert.equal(request.reasoning?.summary, undefined);
      assert.equal(request.proactiveDelegation, effort === 'ultra', `Unexpected delegation mode for ${channels[index].model}/${effort ?? 'default'}`);
      if (effort === 'ultra') {
        assert.ok(request.hasSpawnAgent, 'Ultra must expose native spawn_agent');
        assert.ok(request.hasFollowupTask, 'Ultra must use the native v2 agent toolset');
      }
    }
  }
  assert.equal(report.requests.length, channels.reduce((count, channel) => count + channel.efforts.length + 1, 0));
  assert.ok(report.requests.every(request => request.authenticated && request.stream === true));
  report.helperInvocations = (await readFile(helperLog, 'utf8')).split('invoked').length - 1;
  assert.ok(report.helperInvocations >= 1);
  assert.equal(report.fixtureError, undefined);
  report.passed = true;
} catch (error) {
  report.error = String(error);
  report.recentNotifications = notifications.slice(-8);
} finally {
  child.stdin.end();
  const timer = setTimeout(() => child.kill(), 2000);
  await new Promise(resolve => { if (child.exitCode != null) resolve(); else child.once('exit', resolve); });
  clearTimeout(timer);
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  await writeFile(path.join(runDirectory, 'report.json'), JSON.stringify(report, null, 2));
  await writeFile(path.join(runDirectory, 'app-server.stderr.txt'), stderr);
  console.log(JSON.stringify({ passed: report.passed, version: report.version, models: report.models, requests: report.requests.length, helperInvocations: report.helperInvocations, error: report.error, report: path.join(runDirectory, 'report.json') }, null, 2));
}
if (!report.passed) process.exitCode = 1;
