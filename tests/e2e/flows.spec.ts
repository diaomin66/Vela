import { test, expect, type APIRequestContext, type Page, type Locator } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const nav = (page: Page, name: string) => page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name, exact: true });
const channel = (page: Page, name: string) => page.locator('.channel-card').filter({ has: page.getByRole('heading', { name, exact: true }) });
const modelRow = (page: Page, channelName: string, modelId: string) => page.locator(`.catalog-row[data-model="${modelId}"]`).filter({ has: page.locator('.catalog-model > span').filter({ hasText: new RegExp(`^${channelName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?:\\s*/|$)`) }) });

// Some Windows test environments return an empty 204 for loopback Vite modules.
// Opt into a test-only static GET bridge when needed; it never
// forwards websocket, POST, non-loopback, or third-party traffic.
test.beforeEach(async ({ page, request }: { page: Page; request: APIRequestContext }) => {
  if (process.env.VELA_E2E_LOCAL_PROXY !== '1') return;
  await page.route('http://127.0.0.1:1420/**', async (route) => {
    const input = route.request();
    if (input.method() !== 'GET') return route.continue();
    if (!['document', 'script', 'stylesheet', 'font', 'image'].includes(input.resourceType())) return route.continue();
    const url = new URL(input.url());
    if (url.hostname !== '127.0.0.1' || url.port !== '1420') return route.continue();
    const response = await request.get(input.url(), { maxRedirects: 0, headers: { accept: input.headers().accept ?? '*/*' } });
    const headers = response.headers();
    for (const name of ['connection', 'content-encoding', 'content-length', 'transfer-encoding']) delete headers[name];
    await route.fulfill({ status: response.status(), headers, body: await response.body() });
  });
});

async function selectOption(page: Page, trigger: Locator, name: string) {
  await trigger.click();
  await page.getByRole('option', { name, exact: true }).click();
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
}

test('legacy conversation help explains provider continuity and copies the real model ID', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'ahaX 主页', exact: true })).toBeVisible();
  await expect(page.locator('.brand-symbol img')).toHaveAttribute('src', '/mark.svg');
  await page.getByRole('button', { name: '旧会话报错', exact: true }).click();
  const help = page.getByRole('dialog', { name: '旧会话提示模型不可用', exact: true });
  await expect(help).toContainText('只切换模型，不会同时切换服务商');
  await expect(help).toContainText('再新建会话选择渠道模型');
  await help.getByRole('button', { name: '复制原始模型 ID', exact: true }).click();
  await expect(help.getByRole('status')).toHaveText('模型 ID 已复制');
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('example-code');
  await page.keyboard.press('Escape');
  await expect(help).not.toBeVisible();
  await expect(page.getByRole('button', { name: '旧会话报错', exact: true })).toBeFocused();
});

async function expandModel(editor: Locator, modelId: string) {
  const button = editor.getByRole('button', { name: `${modelId} 的模型设置`, exact: true });
  if (await button.getAttribute('aria-expanded') !== 'true') await button.click();
}

async function openHome(page: Page) {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '渠道管理', exact: true })).toBeVisible();
}
async function checkAccessibility(page: Page, state: string) {
  const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(result.violations.map((v) => ({ id: v.id, description: v.description, targets: v.nodes.map((n) => n.target) })), `Accessibility: ${state}`).toEqual([]);
}
async function checkWidth(page: Page, state: string) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `Page overflow: ${state}`).toBeTruthy();
  const dialogs = await page.locator('dialog[open], .ahax-select-popup:popover-open').evaluateAll((items) => items.map((dialog) => {
    const bounds = dialog.getBoundingClientRect();
    return { left: bounds.left, right: bounds.right, top: bounds.top, bottom: bounds.bottom, scroll: dialog.scrollWidth, client: dialog.clientWidth, width: window.innerWidth, height: window.innerHeight };
  }));
  for (const dialog of dialogs) {
    expect(dialog.left, `Dialog left edge: ${state}`).toBeGreaterThanOrEqual(0);
    expect(dialog.right, `Dialog right edge: ${state}`).toBeLessThanOrEqual(dialog.width);
    expect(dialog.top, `Overlay top edge: ${state}`).toBeGreaterThanOrEqual(0);
    expect(dialog.bottom, `Overlay bottom edge: ${state}`).toBeLessThanOrEqual(dialog.height);
    expect(dialog.scroll, `Dialog overflow: ${state}`).toBeLessThanOrEqual(dialog.client);
  }
}

test('preview is explicit and all four primary pages work without page errors', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await openHome(page);
  await expect(page.getByText('演示模式 · 不修改本机配置')).toBeVisible();
  await expect(page.locator('.channel-card')).toHaveCount(2);
  await nav(page, '模型库').click();
  await expect(page.getByRole('heading', { name: '模型库', exact: true })).toBeVisible();
  await expect(page.locator('.catalog-row')).toHaveCount(4);
  await nav(page, '诊断').click();
  await page.getByRole('button', { name: '开始检查', exact: true }).click();
  await expect(page.getByRole('heading', { name: '检查完成', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '需要在桌面版验证实际环境' })).toBeVisible();
  await nav(page, '恢复').click();
  await expect(page.getByRole('heading', { name: '暂无恢复记录' })).toBeVisible();
  expect(errors).toEqual([]);
});

test('discovery, selection, aliases and unified application preserve key privacy', async ({ page }) => {
  const externalRequests: string[] = [];
  page.on('request', (request) => { if (new URL(request.url()).hostname === 'gateway.example.test') externalRequests.push(request.url()); });
  await openHome(page);
  await page.getByRole('button', { name: '添加渠道', exact: true }).click();
  const editor = page.getByRole('dialog');
  await editor.getByLabel('渠道名称', { exact: false }).fill('测试工作流');
  await editor.getByLabel('API 地址', { exact: true }).fill('https://gateway.example.test/custom');
  await editor.getByLabel('API Key', { exact: true }).fill('demo-only-secret');
  await editor.getByRole('button', { name: '拉取模型与余额' }).click();
  await expect(editor.getByRole('checkbox')).toHaveCount(3);
  await editor.getByRole('button', { name: '余额查询设置', exact: true }).click();
  await expect(editor.getByText('服务路径：https://gateway.example.test/custom/v1')).toBeVisible();
  await expect(editor.getByLabel('API Key', { exact: false })).toHaveValue('');
  await expect(editor.getByRole('checkbox', { name: 'example-code', exact: true })).not.toBeChecked();
  await editor.getByRole('checkbox', { name: 'example-code', exact: true }).check();
  await expandModel(editor, 'example-code');
  await editor.getByLabel('example-code 的显示备注').fill('生产主力');
  await editor.getByRole('checkbox', { name: 'example-pro', exact: true }).check();
  await selectOption(page, editor.getByRole('combobox', { name: '渠道默认模型', exact: true }), '生产主力（example-code）');
  await editor.getByRole('button', { name: '保存渠道', exact: true }).click();
  await expect(editor).toHaveCount(0);
  await expect(channel(page, '测试工作流').locator('.channel-models')).toHaveText('已启用模型2个');
  await expect(channel(page, '测试工作流')).toContainText('生产主力');
  await nav(page, '模型库').click();
  const selected = modelRow(page, '测试工作流', 'example-code');
  await expect(selected).toBeVisible();
  await expect(selected.locator('.catalog-model')).toContainText('生产主力');
  await expect(modelRow(page, '测试工作流', 'example-pro')).toBeVisible();
  await expect(page.locator('.catalog-row')).toHaveCount(6);
  await selected.getByRole('button', { name: '设为默认', exact: true }).click();
  await expect(page.getByRole('heading', { name: '接入统一模型库', exact: true })).toBeVisible();
  await expect(page.getByRole('dialog')).toContainText('6 个模型 · 3 个渠道');
  await expect(page.getByRole('dialog')).toContainText('测试工作流 · 生产主力（example-code）');
  await page.getByRole('button', { name: '确认并应用', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(selected.getByRole('button', { name: '默认模型', exact: true })).toBeDisabled();
  expect(externalRequests).toEqual([]);
  expect(await page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }))).not.toContain('demo-only-secret');
  await expect(page.locator('body')).not.toContainText('demo-only-secret');
});

test('channel search matches names, API addresses and model aliases and recovers from no results', async ({ page }) => {
  await openHome(page);
  const search = page.getByRole('textbox', { name: '搜索渠道', exact: true });
  const names = page.locator('.channel-card .channel-heading h2');
  await expect(names).toHaveText(['主力渠道', '备用渠道']);
  await search.fill(' 备用 ');
  await expect(names).toHaveText(['备用渠道']);
  await search.fill('API.EXAMPLE.COM');
  await expect(names).toHaveText(['主力渠道']);
  await search.fill('编程主力');
  await expect(names).toHaveText(['主力渠道']);
  await search.fill('EXAMPLE-CODE');
  await expect(names).toHaveText(['主力渠道', '备用渠道']);
  await page.getByRole('button', { name: '清空渠道搜索', exact: true }).click();
  await expect(search).toHaveValue('');
  await expect(names).toHaveText(['主力渠道', '备用渠道']);
  await expect(page.getByRole('button', { name: '清空渠道搜索', exact: true })).toHaveCount(0);
  await search.fill('channel-that-does-not-exist');
  await expect(page.locator('.channel-card')).toHaveCount(0);
  await expect(page.getByRole('heading', { name: '没有匹配的渠道', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '添加渠道', exact: true })).toBeEnabled();
  await expect(page.locator('.channel-empty')).toHaveCount(0);
  await page.getByRole('button', { name: '清除筛选', exact: true }).click();
  await expect(search).toHaveValue('');
  await expect(names).toHaveText(['主力渠道', '备用渠道']);
  await expect(page.getByRole('heading', { name: '没有匹配的渠道', exact: true })).toHaveCount(0);
});

test('model search and channel filter retain distinct same-ID entries', async ({ page }) => {
  await openHome(page);
  await nav(page, '模型库').click();
  await page.getByRole('textbox', { name: '搜索全部模型' }).fill('example-code');
  await expect(page.locator('.catalog-row')).toHaveCount(2);
  await expect(modelRow(page, '主力渠道', 'example-code')).toBeVisible();
  await expect(modelRow(page, '备用渠道', 'example-code')).toBeVisible();
  await expect(page.locator('.catalog-row[data-model="example-code"] .catalog-model > span')).toHaveText(['主力渠道/example-code', '备用渠道']);
  await selectOption(page, page.getByRole('combobox', { name: '按渠道筛选', exact: true }), '备用渠道');
  await expect(page.locator('.catalog-row')).toHaveCount(1);
  await expect(modelRow(page, '备用渠道', 'example-code')).toBeVisible();
  await page.getByRole('textbox', { name: '搜索全部模型' }).fill('missing-model-id');
  await expect(page.getByRole('heading', { name: '没有匹配的模型' })).toBeVisible();
  await page.getByRole('textbox', { name: '搜索全部模型' }).clear();
  await expect(page.locator('.catalog-row')).toHaveCount(2);
  await selectOption(page, page.getByRole('combobox', { name: '按渠道筛选', exact: true }), '全部渠道');
  await expect(page.locator('.catalog-row')).toHaveCount(4);
});

test('verification names the chosen model and cancellation is recoverable', async ({ page }) => {
  await openHome(page);
  await nav(page, '模型库').click();
  const verify = page.getByRole('button', { name: '验证 备用渠道（example-reasoning）', exact: true });
  await verify.click();
  await expect(page.locator('.validation-profile')).toContainText('example-reasoning');
  await expect(page.getByRole('dialog')).toContainText('结果仅适用于此模型');
  await page.getByRole('button', { name: '开始验证', exact: true }).click();
  await page.getByRole('button', { name: '取消验证', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await verify.click();
  await page.getByRole('button', { name: '开始验证', exact: true }).click();
  await expect(page.getByRole('heading', { name: '验证通过', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '工具调用与结果回传', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '返回模型库', exact: true }).click();
  await expect(modelRow(page, '主力渠道', 'example-code').getByRole('button', { name: '默认模型' })).toBeDisabled();
});

test('unsupported balance is explicitly unavailable rather than zero', async ({ page }) => {
  await openHome(page);
  const fallback = channel(page, '备用渠道').locator('.channel-balance');
  await expect(fallback).toContainText('暂不支持查询');
  await expect(fallback.locator('strong')).toHaveCount(0);
  await expect(fallback).not.toContainText('0.00');
  await expect(channel(page, '主力渠道').locator('.channel-balance')).toContainText('128.5');
  await expect(channel(page, '主力渠道').locator('.channel-balance')).toContainText('站点计费单位');
});

test('saved endpoints are immutable and resync preserves selected models and aliases', async ({ page }) => {
  await openHome(page);
  await channel(page, '主力渠道').getByRole('button', { name: '管理模型', exact: true }).click();
  const editor = page.getByRole('dialog');
  await expect(editor.getByLabel('API 地址', { exact: true })).toHaveAttribute('readonly', '');
  await editor.getByRole('checkbox', { name: 'example-pro', exact: true }).uncheck();
  await expandModel(editor, 'example-code');
  await editor.getByLabel('example-code 的显示备注').fill('稳定编程');
  await editor.getByRole('button', { name: '拉取模型与余额' }).click();
  await expect(editor.getByRole('button', { name: '拉取模型与余额' })).toBeEnabled();
  await expect(editor.getByRole('checkbox', { name: 'example-pro', exact: true })).not.toBeChecked();
  await expect(editor.getByRole('checkbox', { name: 'example-code', exact: true })).toBeChecked();
  await expect(editor.getByLabel('example-code 的显示备注')).toHaveValue('稳定编程');
  await editor.getByLabel('搜索渠道模型').fill('example-fast');
  await expect(editor.locator('.editor-model-choice')).toHaveCount(1);
  await editor.getByRole('button', { name: '选中搜索结果', exact: true }).click();
  await editor.getByRole('button', { name: '保存渠道', exact: true }).click();
  await nav(page, '模型库').click();
  await expect(modelRow(page, '主力渠道', 'example-code').locator('.catalog-model')).toContainText('稳定编程');
  await expect(modelRow(page, '主力渠道', 'example-fast')).toBeVisible();
  await expect(modelRow(page, '主力渠道', 'example-pro')).toHaveCount(0);
});

test('unsafe remote HTTP is rejected before discovery or saving', async ({ page }) => {
  await openHome(page);
  await page.getByRole('button', { name: '添加渠道', exact: true }).click();
  await page.getByLabel('渠道名称', { exact: false }).fill('Wrong');
  await page.getByLabel('API 地址', { exact: true }).fill('http://remote.example.test/v1');
  await page.getByLabel('API Key', { exact: true }).fill('test-only');
  await page.getByRole('button', { name: '拉取模型与余额' }).click();
  await expect(page.getByRole('alert')).toContainText('HTTPS');
  await expect(page.getByRole('heading', { name: '添加渠道', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '关闭弹窗' }).click();
  await expect(page.locator('.channel-card')).toHaveCount(2);
});

test('settings persist provider, port and refresh choices and require reapplication', async ({ page }) => {
  await openHome(page);
  await page.getByRole('button', { name: 'ahaX 设置', exact: true }).click();
  await expect(page.getByLabel('服务商显示名称')).toHaveValue('ahaX');
  await page.getByLabel('服务商显示名称').fill('ahaX Studio');
  await selectOption(page, page.getByRole('combobox', { name: '同步间隔', exact: true }), '30 分钟');
  await page.getByRole('button', { name: '连接与存储', exact: true }).click();
  await page.getByLabel('本地服务端口').fill('19001');
  await page.locator('label[for="auto-refresh"]').click();
  await expect(page.getByRole('checkbox', { name: '自动同步模型与余额', exact: true })).not.toBeChecked();
  await expect(page.getByRole('combobox', { name: '同步间隔', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '保存设置', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('.connection-status')).toContainText('有更改待同步');
  await page.getByRole('button', { name: 'ahaX 设置', exact: true }).click();
  await expect(page.getByLabel('服务商显示名称')).toHaveValue('ahaX Studio');
  await page.getByRole('button', { name: '连接与存储', exact: true }).click();
  await expect(page.getByLabel('本地服务端口')).toHaveValue('19001');
  await expect(page.getByLabel('自动同步模型与余额', { exact: false })).not.toBeChecked();
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await nav(page, '模型库').click();
  await page.getByRole('button', { name: '更新 Codex 配置', exact: true }).click();
  await expect(page.getByRole('dialog')).toContainText('ahaX Studio');
  await page.getByRole('button', { name: '确认并应用', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await nav(page, '渠道').click();
  await expect(page.locator('.connection-status')).toContainText('已同步至 Codex');
  await page.getByRole('button', { name: 'ahaX 设置', exact: true }).click();
  await page.getByRole('button', { name: '连接与存储', exact: true }).click();
  await expect(page.getByLabel('本地服务端口')).toHaveValue('19001');
});

test('gateway backup restore restores the previous default and creates another snapshot', async ({ page }) => {
  await openHome(page);
  await nav(page, '模型库').click();
  const original = modelRow(page, '主力渠道', 'example-code');
  const alternate = modelRow(page, '备用渠道', 'example-reasoning');
  await expect(original.getByRole('button', { name: '默认模型', exact: true })).toBeDisabled();
  await alternate.getByRole('button', { name: '设为默认', exact: true }).click();
  await page.getByRole('button', { name: '确认并应用', exact: true }).click();
  await expect(alternate.getByRole('button', { name: '默认模型', exact: true })).toBeDisabled();
  await nav(page, '恢复').click();
  await expect(page.locator('.timeline-entry')).toHaveCount(1);
  await page.getByRole('button', { name: '恢复到此时', exact: true }).click();
  await page.getByRole('button', { name: '确认并恢复', exact: true }).click();
  await expect(page.locator('.timeline-entry')).toHaveCount(2);
  await nav(page, '模型库').click();
  await expect(original.getByRole('button', { name: '默认模型', exact: true })).toBeDisabled();
  await expect(alternate.getByRole('button', { name: '设为默认', exact: true })).toBeEnabled();
});

test('reasoning default persists, requires application, and restores with its catalog snapshot', async ({ page }) => {
  await openHome(page);
  await nav(page, '模型库').click();
  const row = modelRow(page, '主力渠道', 'example-code');
  const strength = row.getByRole('combobox', { name: '主力渠道 · 编程主力（example-code） 的推理强度', exact: true });
  await expect(strength).toHaveText('中');
  await selectOption(page, strength, '高');
  await expect(strength).toHaveText('高');
  await expect(page.locator('.connection-status')).toContainText('有更改待同步');
  await row.getByRole('button', { name: '设置 主力渠道 · 编程主力（example-code）', exact: true }).click();
  await expect(page.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true })).toHaveText('高');
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await page.getByRole('button', { name: '更新 Codex 配置', exact: true }).click();
  await page.getByRole('button', { name: '确认并应用', exact: true }).click();
  await expect(page.locator('.connection-status')).toContainText('已同步至 Codex');
  await expect(strength).toHaveText('高');
  await nav(page, '恢复').click();
  await page.getByRole('button', { name: '恢复到此时', exact: true }).click();
  await page.getByRole('button', { name: '确认并恢复', exact: true }).click();
  await nav(page, '模型库').click();
  await expect(strength).toHaveText('中');
  await expect(page.locator('.connection-status')).toContainText('已同步至 Codex');
});

test('model reasoning presets and explicit defaults survive channel editing', async ({ page }) => {
  await openHome(page);
  await channel(page, '主力渠道').getByRole('button', { name: '管理模型', exact: true }).click();
  const editor = page.getByRole('dialog');
  await expandModel(editor, 'example-code');
  await selectOption(page, editor.getByRole('combobox', { name: 'example-code 的推理档位', exact: true }), '不支持推理强度');
  await expect(editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true })).toHaveCount(0);
  await selectOption(page, editor.getByRole('combobox', { name: 'example-code 的推理档位', exact: true }), '低 · 中 · 高');
  await selectOption(page, editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true }), '高');
  await editor.getByRole('button', { name: '保存渠道', exact: true }).click();
  await nav(page, '模型库').click();
  const strength = modelRow(page, '主力渠道', 'example-code').getByRole('combobox');
  await expect(strength).toHaveText('高');
  await strength.click();
  await expect(page.getByRole('option')).toHaveText(['低', '中', '高']);
  await page.keyboard.press('Escape');
  await expect(strength).toBeFocused();
});

test('custom selects support keyboard use and Escape closes only the popup', async ({ page }) => {
  test.setTimeout(60000);
  await openHome(page);
  await channel(page, '主力渠道').getByRole('button', { name: '管理模型', exact: true }).click();
  const editor = page.getByRole('dialog');
  await expandModel(editor, 'example-code');
  const strength = editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true });
  await strength.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('listbox', { name: 'example-code 的默认推理强度', exact: true })).toBeVisible();
  await checkAccessibility(page, '打开的推理下拉');
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  await expect(strength).toHaveText('超高');
  await expect(strength).toBeFocused();
  await strength.click();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await expect(editor).toBeVisible();
  await expect(strength).toBeFocused();
});

test('large model catalogs stay searchable while navigation and footer remain fixed', async ({ page }) => {
  test.setTimeout(60000);
  await openHome(page);
  await page.evaluate(async () => {
    // Seed only the browser demo's in-memory API; this never invokes native IPC.
    const modulePath = performance.getEntriesByType('resource').map((entry) => entry.name).find((name) => new URL(name).pathname === '/src/lib/api.ts');
    if (!modulePath) throw new Error('The page has not loaded its API module.');
    const { api, desktop } = await import(modulePath);
    if (desktop) throw new Error('This fixture must never run in the desktop application.');
    const { profiles } = await api.dashboard();
    const original = profiles.find((entry: { id: string }) => entry.id === 'demo-work');
    await api.saveProfile({
      id: original.id, name: original.name, baseUrl: original.baseUrl, model: 'bulk-model-000',
      models: Array.from({ length: 240 }, (_, index) => ({ id: `bulk-model-${String(index).padStart(3, '0')}`, alias: '', enabled: true, reasoningEfforts: ['low', 'medium', 'high'], defaultReasoningEffort: 'medium' })),
    });
  });
  await page.getByRole('button', { name: '刷新主力渠道', exact: true }).click();
  await expect(channel(page, '主力渠道').locator('.channel-models')).toHaveText('已启用模型240个');
  await nav(page, '模型库').click();
  await expect(page.locator('.catalog-row[data-channel="demo-work"]')).toHaveCount(200);
  const header = page.locator('.app-header');
  const before = await header.boundingBox();
  expect(before).not.toBeNull();
  await page.locator('.main-content').evaluate((element) => { element.scrollTop = element.scrollHeight; });
  await expect.poll(() => page.locator('.main-content').evaluate((element) => element.scrollTop)).toBeGreaterThan(1000);
  const after = await header.boundingBox();
  expect(after!.y).toBeCloseTo(before!.y, 1);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  await expect(nav(page, '渠道')).toBeInViewport();
  await expect(page.locator('.app-footer')).toBeInViewport();
  await page.getByRole('textbox', { name: '搜索全部模型' }).fill('bulk-model-239');
  await expect(page.locator('.catalog-row')).toHaveCount(1);
  await expect(modelRow(page, '主力渠道', 'bulk-model-239')).toBeVisible();
  await modelRow(page, '主力渠道', 'bulk-model-239').locator('.catalog-model').click();
  await expect(page.getByRole('button', { name: 'bulk-model-239 的模型设置', exact: true })).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByRole('combobox', { name: 'bulk-model-239 的默认推理强度', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '关闭弹窗' }).click();
  await nav(page, '渠道').click();
  await expect(page.getByRole('heading', { name: '渠道管理', exact: true })).toBeInViewport();
  await expect.poll(() => page.locator('.main-content').evaluate((element) => element.scrollTop)).toBe(0);
});

test('all pages and completed diagnostics satisfy WCAG checks', async ({ page }) => {
  test.setTimeout(60000);
  await openHome(page);
  for (const name of ['渠道', '模型库', '诊断', '恢复']) { await nav(page, name).click(); await checkAccessibility(page, name); }
  await nav(page, '诊断').click();
  await page.getByRole('button', { name: '开始检查', exact: true }).click();
  await expect(page.getByRole('heading', { name: '检查完成', exact: true })).toBeVisible();
  await checkAccessibility(page, '诊断报告');
});

test('model editor and expanded custom balance fields satisfy WCAG checks', async ({ page }) => {
  test.setTimeout(60000);
  await openHome(page);
  await channel(page, '主力渠道').getByRole('button', { name: '管理模型', exact: true }).click();
  await checkAccessibility(page, '模型编辑');
  await expandModel(page.getByRole('dialog'), 'example-code');
  await checkAccessibility(page, '展开的模型推理设置');
  await page.getByRole('button', { name: '余额查询设置', exact: true }).click();
  await selectOption(page, page.getByRole('combobox', { name: '查询方式', exact: true }), '自定义接口');
  await page.getByLabel('接口路径', { exact: true }).fill('/api/account/balance');
  await page.getByLabel('余额字段', { exact: true }).fill('data.balance');
  await page.getByLabel('显示单位', { exact: true }).fill('CNY');
  await checkAccessibility(page, '自定义余额');
});

test('settings, verification and configuration preview satisfy WCAG checks', async ({ page }) => {
  test.setTimeout(60000);
  await openHome(page);
  await page.getByRole('button', { name: 'ahaX 设置', exact: true }).click();
  await checkAccessibility(page, '后台设置');
  await page.getByRole('button', { name: '关闭弹窗' }).click();
  await nav(page, '模型库').click();
  await page.getByRole('button', { name: '验证 备用渠道（example-reasoning）', exact: true }).click();
  await checkAccessibility(page, '验证');
  await page.getByRole('button', { name: '关闭弹窗' }).click();
  await modelRow(page, '备用渠道', 'example-reasoning').getByRole('button', { name: '设为默认', exact: true }).click();
  await checkAccessibility(page, '配置预览');
});

test('390px layout fits all pages, model editor, custom fields and settings', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openHome(page);
  await page.evaluate(() => document.fonts.ready);
  for (const name of ['渠道', '模型库', '诊断', '恢复']) { await nav(page, name).click(); await checkWidth(page, name); }
  await nav(page, '渠道').click();
  await channel(page, '主力渠道').getByRole('button', { name: '管理模型', exact: true }).click();
  await checkWidth(page, '模型编辑');
  await expandModel(page.getByRole('dialog'), 'example-code');
  await page.getByRole('combobox', { name: 'example-code 的推理档位', exact: true }).click();
  await checkWidth(page, '推理档位下拉');
  expect(await page.locator('.ahax-select-popup:popover-open').evaluate((element) => parseFloat(getComputedStyle(element).borderTopLeftRadius))).toBeGreaterThanOrEqual(12);
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: '余额查询设置', exact: true }).click();
  await page.getByRole('combobox', { name: '查询方式', exact: true }).click();
  await checkWidth(page, '余额下拉');
  await page.getByRole('option', { name: '自定义接口', exact: true }).click();
  await checkWidth(page, '自定义余额');
  await page.getByRole('button', { name: '关闭弹窗' }).click();
  await page.getByRole('button', { name: 'ahaX 设置', exact: true }).click();
  await checkWidth(page, '设置');
  await page.getByRole('combobox', { name: '同步间隔', exact: true }).click();
  await checkWidth(page, '同步间隔下拉');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: '关闭弹窗' }).click();
  await nav(page, '模型库').click();
  await modelRow(page, '主力渠道', 'example-code').getByRole('combobox').click();
  await checkWidth(page, '模型库推理强度下拉');
});
