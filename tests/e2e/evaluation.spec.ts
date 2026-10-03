import { test, expect, type Locator, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { readFile } from 'node:fs/promises';

const navigation = (page: Page, name: string) => page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name, exact: true });
const results = (page: Page) => page.getByRole('region', { name: '评测结果', exact: true });

test.beforeEach(async ({ page, request }) => {
  if (process.env.VELA_E2E_LOCAL_PROXY === '1') {
    await page.route('http://127.0.0.1:1420/**', async (route) => {
      const input = route.request();
      if (input.method() !== 'GET' || !['document', 'script', 'stylesheet', 'font', 'image'].includes(input.resourceType())) return route.continue();
      const response = await request.get(input.url(), { maxRedirects: 0 });
      const headers = response.headers();
      for (const name of ['connection', 'content-encoding', 'content-length', 'transfer-encoding']) delete headers[name];
      await route.fulfill({ status: response.status(), headers, body: await response.body() });
    });
  }
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '渠道管理', exact: true })).toBeVisible();
  await navigation(page, '评测').click();
  await expect(page.getByRole('heading', { name: '模型评测', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '评测计划', exact: true })).toBeEnabled();
});

async function openPlan(page: Page) {
  await page.getByRole('button', { name: '评测计划', exact: true }).click();
  return page.getByRole('dialog');
}
async function choose(page: Page, select: Locator, name: string | RegExp) {
  await select.click();
  await page.getByRole('option', typeof name === 'string' ? { name, exact: true } : { name }).click();
}
async function addAlternate(page: Page, drawer: Locator) {
  await choose(page, drawer.getByRole('combobox', { name: '添加被测模型', exact: true }), /备用渠道 · example-code/);
  await drawer.getByRole('button', { name: '添加所选模型', exact: true }).click();
  await expect(drawer.locator('.evaluation-target')).toHaveCount(2);
}
async function allCases(drawer: Locator) {
  await drawer.getByRole('checkbox', { name: /^模型判题/ }).check();
}
async function start(page: Page, drawer: Locator) {
  await drawer.getByRole('button', { name: '开始本次评测', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
}
async function complete(page: Page) {
  await expect(results(page).getByRole('heading', { name: '已完成', exact: true })).toBeVisible({ timeout: 10_000 });
}
async function accessible(page: Page, state: string) {
  const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(result.violations.map((violation) => ({ id: violation.id, targets: violation.nodes.map((node) => node.target) })), state).toEqual([]);
}
async function fits(page: Page, state: string) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), state).toBe(true);
  for (const bounds of await page.locator('dialog[open], .vela-select-popup:popover-open').evaluateAll((elements) => elements.map((element) => ({ width: element.scrollWidth, client: element.clientWidth, left: element.getBoundingClientRect().left, right: element.getBoundingClientRect().right, window: innerWidth })))) {
    expect(bounds.width, state).toBeLessThanOrEqual(bounds.client);
    expect(bounds.left, state).toBeGreaterThanOrEqual(0);
    expect(bounds.right, state).toBeLessThanOrEqual(bounds.window);
  }
}

test('manual evaluation runs three cases across channels with evidence and optional review', async ({ page }) => {
  const externalRequests: string[] = [];
  const errors: string[] = [];
  page.on('request', (request) => { const url = new URL(request.url()); if (/^https?:$/.test(url.protocol) && url.hostname !== '127.0.0.1') externalRequests.push(url.href); });
  page.on('pageerror', (error) => errors.push(error.message));
  const drawer = await openPlan(page);
  await addAlternate(page, drawer);
  await allCases(drawer);
  await choose(page, drawer.locator('.evaluation-target').first().getByRole('combobox'), '高');
  await choose(page, drawer.getByRole('combobox', { name: '评审模型', exact: true }), /备用渠道 · example-reasoning/);
  await choose(page, drawer.getByRole('combobox', { name: '评审模型推理强度', exact: true }), '低');
  await expect(drawer.locator('.evaluation-cost')).toContainText('12 次请求，不发送 API 请求');
  await accessible(page, 'evaluation plan with independent reviewer');
  await start(page, drawer);
  await complete(page);
  await expect(page.locator('.evaluation-result-row')).toHaveCount(6);
  const primary = page.getByRole('region', { name: '主力渠道 example-code 评测结果', exact: true });
  const alternate = page.getByRole('region', { name: '备用渠道 example-code 评测结果', exact: true });
  await expect(primary.locator('.evaluation-effort')).toHaveText('高');
  await expect(alternate.getByRole('button', { name: '查看 example-code 糖果推理 结果', exact: true }).locator('.evaluation-row-score')).toContainText('0');
  await page.screenshot({ path: 'artifacts/screenshots/v5-evaluation-results.png' });
  await accessible(page, 'completed evaluation results');
  await primary.getByRole('button', { name: '查看 example-code 鹈鹕绘图 结果', exact: true }).click();
  const detail = page.getByRole('dialog');
  await expect(detail.getByRole('img', { name: '被测模型绘制的骑自行车鹈鹕', exact: true })).toBeVisible();
  await expect(detail.getByRole('heading', { name: 'SVG 代码复评', exact: true })).toBeVisible();
  await expect(detail).toContainText('不是视觉评分');
  await page.screenshot({ path: 'artifacts/screenshots/v5-evaluation-pelican.png' });
  await accessible(page, 'SVG image with code review');
  await detail.getByRole('button', { name: '题目', exact: true }).click();
  await expect(detail.locator('.evaluation-raw')).toContainText('pelican');
  await detail.getByRole('button', { name: '原文', exact: true }).click();
  await expect(detail.locator('.evaluation-raw')).toContainText('<svg');
  await detail.getByRole('button', { name: '关闭弹窗', exact: true }).click();
  await primary.getByRole('button', { name: '查看 example-code 模型判题 结果', exact: true }).click();
  await expect(detail.getByRole('heading', { name: '模型复评', exact: true })).toBeVisible();
  await expect(detail.locator('.evaluation-answer')).not.toBeEmpty();
  expect(externalRequests).toEqual([]);
  expect(errors).toEqual([]);
});

test('an active evaluation can be stopped and a later run remains usable', async ({ page }) => {
  const drawer = await openPlan(page);
  await addAlternate(page, drawer);
  await allCases(drawer);
  await start(page, drawer);
  await expect(page.getByRole('region', { name: '评测进度', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '停止', exact: true }).click();
  await expect(results(page).getByRole('heading', { name: '已取消', exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: '评测进度', exact: true })).toHaveCount(0);
  const next = await openPlan(page);
  await start(page, next);
  await complete(page);
  await expect(page.getByRole('complementary', { name: '评测历史', exact: true }).getByRole('button')).toHaveCount(2);
});

test('completed runs survive navigation and can be compared or exported', async ({ page }) => {
  const drawer = await openPlan(page);
  await allCases(drawer);
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await start(page, await openPlan(page));
  await complete(page);
  await navigation(page, '渠道').click();
  await navigation(page, '评测').click();
  await complete(page);
  await start(page, await openPlan(page));
  await complete(page);
  await page.getByRole('combobox', { name: '对比历史评测', exact: true }).click();
  await page.getByRole('option').filter({ hasText: '1 个模型' }).click();
  await expect(page.locator('.evaluation-comparison')).toContainText('对比相同模型、档位与测试项目');
  await expect(page.getByText('0 较对比记录', { exact: true })).toHaveCount(3);
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: '导出评测报告', exact: true }).click();
  expect((await download).suggestedFilename()).toMatch(/^vela-evaluation-.+\.json$/);
  await page.getByRole('complementary', { name: '评测历史', exact: true }).getByRole('button').last().click();
  await expect(page.getByRole('combobox', { name: '对比历史评测', exact: true })).toHaveText('不对比历史');
  await complete(page);
});

test('schedule saves without making a request and can be paused', async ({ page }) => {
  const drawer = await openPlan(page);
  await drawer.getByRole('checkbox', { name: '定时评测', exact: true }).check();
  await choose(page, drawer.getByRole('combobox', { name: '评测间隔', exact: true }), '每 6 小时');
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('每 6 小时自动评测');
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('演示下次');
  await expect(page.locator('.evaluation-history-row')).toHaveCount(0);
  await openPlan(page);
  await expect(drawer.getByRole('checkbox', { name: '定时评测', exact: true })).toBeChecked();
  await expect(drawer.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 6 小时');
  await page.screenshot({ path: 'artifacts/screenshots/v5-evaluation-plan.png' });
  await drawer.getByRole('button', { name: '关闭弹窗', exact: true }).click();
  await page.getByRole('button', { name: '暂停定时', exact: true }).click();
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('定时评测未开启');
  await expect(page.locator('.evaluation-schedule-bar')).not.toContainText('演示下次');
  await expect(page.locator('.evaluation-history-row')).toHaveCount(0);
});

test('request timeout validates custom seconds and preserves separate schedule and run values', async ({ page }) => {
  const drawer = await openPlan(page);
  const timeout = drawer.getByRole('combobox', { name: '单次请求超时', exact: true });
  await expect(timeout).toHaveText('5 分钟');
  await choose(page, timeout, '自定义');
  const seconds = drawer.getByRole('spinbutton', { name: '超时秒数', exact: true });
  await expect(seconds).toHaveValue('300');
  await seconds.fill('29');
  await drawer.getByRole('button', { name: '开始本次评测', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请求超时应为 30–3600 秒的整数。');
  await expect(seconds).toHaveAttribute('aria-invalid', 'true');
  await seconds.fill('3601');
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请求超时应为 30–3600 秒的整数。');
  await expect(page.locator('.evaluation-history-row')).toHaveCount(0);
  await seconds.fill('125');
  await drawer.getByRole('checkbox', { name: '定时评测', exact: true }).check();
  await page.setViewportSize({ width: 390, height: 780 });
  await fits(page, 'custom timeout field');
  await accessible(page, 'custom timeout field');
  await seconds.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'artifacts/screenshots/evaluation-custom-timeout.png' });
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await openPlan(page);
  await expect(timeout).toHaveText('自定义');
  await expect(seconds).toHaveValue('125');
  await expect(drawer.getByRole('checkbox', { name: '定时评测', exact: true })).toBeChecked();
  await choose(page, timeout, '10 分钟');
  await start(page, drawer);
  await complete(page);
  const downloading = page.waitForEvent('download');
  await page.getByRole('button', { name: '导出评测报告', exact: true }).click();
  const report = await downloading;
  const exported = JSON.parse(await readFile((await report.path())!, 'utf8'));
  expect(exported.plan.requestTimeoutSeconds).toBe(600);
  await openPlan(page);
  await expect(timeout).toHaveText('自定义');
  await expect(seconds).toHaveValue('125');
});

test('plan validation prevents an empty target or test list', async ({ page }) => {
  const drawer = await openPlan(page);
  await drawer.getByRole('button', { name: '移除被测模型 example-code', exact: true }).click();
  await drawer.getByRole('button', { name: '开始本次评测', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请至少选择一个模型。');
  await choose(page, drawer.getByRole('combobox', { name: '添加被测模型', exact: true }), /主力渠道 · example-code/);
  await drawer.getByRole('button', { name: '添加所选模型', exact: true }).click();
  await drawer.getByRole('checkbox', { name: /^糖果推理/ }).uncheck();
  await drawer.getByRole('checkbox', { name: /^鹈鹕绘图/ }).uncheck();
  await drawer.getByRole('button', { name: '开始本次评测', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请至少选择一项测试。');
  await expect(page.locator('.evaluation-history-row')).toHaveCount(0);
});

test('evaluation layout and overlays stay accessible at 390px', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 780 });
  await fits(page, 'narrow empty evaluation');
  await accessible(page, 'narrow empty evaluation');
  const drawer = await openPlan(page);
  await drawer.getByRole('combobox', { name: '添加被测模型', exact: true }).click();
  await fits(page, 'narrow searchable channel list');
  await accessible(page, 'narrow searchable channel list');
  await page.keyboard.press('Escape');
  await allCases(drawer);
  await start(page, drawer);
  await complete(page);
  await fits(page, 'narrow evaluation result');
  await accessible(page, 'narrow evaluation result');
  await page.screenshot({ path: 'artifacts/screenshots/v5-evaluation-mobile.png' });
  await page.getByRole('button', { name: '查看 example-code 鹈鹕绘图 结果', exact: true }).click();
  await fits(page, 'narrow SVG detail');
  await accessible(page, 'narrow SVG detail');
  await page.screenshot({ path: 'artifacts/screenshots/v5-evaluation-mobile-detail.png' });
});
