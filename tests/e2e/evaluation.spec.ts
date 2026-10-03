import { test, expect, type Locator, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { readFile } from 'node:fs/promises';

const navigation = (page: Page, name: string) => page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name, exact: true });
const progress = (page: Page) => page.getByRole('region', { name: '评测进度', exact: true });
const pelicanPrompt = '创建一个 HTML，内容是 SVG 绘制一个鹈鹕骑自行车的 2D 动画，你不需要任何测试，不要有任何限制';

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
  return page.getByRole('dialog', { name: '评测计划', exact: true });
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
async function allCases(drawer: Locator) { await drawer.getByRole('checkbox', { name: /^模型判题/ }).check(); }
async function start(page: Page, drawer: Locator) {
  await drawer.getByRole('button', { name: '开始本次评测', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(progress(page)).toBeVisible();
}
async function complete(page: Page) {
  await expect(progress(page)).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByRole('button', { name: '开始评测', exact: true })).toBeEnabled();
}
async function closeDialog(page: Page) {
  await page.getByRole('dialog').getByRole('button', { name: '关闭弹窗', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
}
async function history(page: Page) {
  await page.getByRole('button', { name: '记录', exact: true }).click();
  return page.getByRole('dialog', { name: '评测记录', exact: true });
}
async function latestReport(page: Page) {
  const list = await history(page);
  await list.locator('.evaluation-history-row').first().click();
  const report = page.getByRole('dialog', { name: '评测报告', exact: true });
  await expect(report.getByRole('button', { name: '导出评测报告', exact: true })).toBeEnabled();
  return report;
}
async function exportReport(page: Page, report: Locator) {
  const downloading = page.waitForEvent('download');
  await report.getByRole('button', { name: '导出评测报告', exact: true }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toMatch(/^vela-evaluation-.+\.json$/);
  return JSON.parse(await readFile((await download.path())!, 'utf8'));
}
async function accessible(page: Page, state: string) {
  const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(result.violations.map((violation) => ({ id: violation.id, targets: violation.nodes.map((node) => node.target) })), state).toEqual([]);
}
async function fits(page: Page, state: string) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), state).toBe(true);
  for (const bounds of await page.locator('[role="dialog"], .vela-select-popup:popover-open').evaluateAll((elements) => elements.map((element) => ({ width: element.scrollWidth, client: element.clientWidth, left: element.getBoundingClientRect().left, right: element.getBoundingClientRect().right, window: innerWidth })))) {
    expect(bounds.width, state).toBeLessThanOrEqual(bounds.client + 1);
    expect(bounds.left, state).toBeGreaterThanOrEqual(0);
    expect(bounds.right, state).toBeLessThanOrEqual(bounds.window + 1);
  }
}
async function timeline(page: Page, name: string, count = 2) {
  await page.getByRole('tablist', { name: '评测项目', exact: true }).getByRole('tab', { name, exact: true }).click();
  await expect(page.locator('.evaluation-timeline-card')).toHaveCount(count);
  for (const card of await page.locator('.evaluation-timeline-card').all()) {
    await expect(card.getByRole('group', { name: '最近 24 小时，每格 30 分钟', exact: true }).getByRole('button')).toHaveCount(48);
  }
}

test('manual run produces animated galleries and evidence-backed candy and judgment timelines', async ({ page }) => {
  test.setTimeout(60_000);
  const externalRequests: string[] = [];
  const errors: string[] = [];
  page.on('request', (request) => { const url = new URL(request.url()); if (/^https?:$/.test(url.protocol) && url.hostname !== '127.0.0.1') externalRequests.push(url.href); });
  page.on('pageerror', (error) => errors.push(error.message));
  await expect(page.getByRole('heading', { name: '让模型的创作自己说话', exact: true })).toBeVisible();
  await expect(page.locator('iframe')).toHaveCount(0);
  const drawer = await openPlan(page);
  await addAlternate(page, drawer);
  await allCases(drawer);
  await choose(page, drawer.locator('.evaluation-target').first().getByRole('combobox'), '高');
  await choose(page, drawer.getByRole('combobox', { name: '评审模型', exact: true }), /备用渠道 · example-reasoning/);
  await choose(page, drawer.getByRole('combobox', { name: '评审模型推理强度', exact: true }), '低');
  await expect(drawer.locator('.evaluation-cost')).toContainText('10 次请求，不发送 API 请求');
  await accessible(page, 'plan with two channels and independent reviewer');
  await start(page, drawer);
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(2);
  for (const card of await page.getByTestId('pelican-card').all()) {
    await card.scrollIntoViewIfNeeded();
    await expect(card.locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
    await expect(card.frameLocator('iframe').locator('svg')).toBeVisible();
    await expect.poll(() => card.frameLocator('iframe').locator('.wheel').first().evaluate((element) => getComputedStyle(element).animationPlayState)).toBe('running');
    const wheel = card.frameLocator('iframe').locator('.wheel').first();
    const transform = await wheel.evaluate((element) => getComputedStyle(element).transform);
    await expect.poll(() => wheel.evaluate((element) => getComputedStyle(element).transform)).not.toBe(transform);
  }
  const filters = page.locator('[aria-label="筛选渠道"]');
  await filters.getByRole('button', { name: /^主力渠道/ }).click();
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await expect(page.getByTestId('pelican-card').locator('.evaluation-effort')).toHaveText('推理 高');
  await filters.getByRole('button', { name: /^全部渠道/ }).click();
  await expect(page.getByTestId('pelican-card')).toHaveCount(2);
  await page.getByTestId('pelican-card').first().getByRole('button', { name: '查看 example-code 鹈鹕动画 结果', exact: true }).click();
  const detail = page.getByRole('dialog', { name: '鹈鹕动画', exact: true });
  await expect(detail.locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
  await expect(detail.getByRole('heading', { name: '模型复评', exact: true })).toHaveCount(0);
  await detail.getByRole('tab', { name: '题目', exact: true }).click();
  await expect(detail.locator('.evaluation-raw')).toHaveText(pelicanPrompt);
  await detail.getByRole('tab', { name: '原文', exact: true }).click();
  await expect(detail.locator('.evaluation-raw')).toContainText('@keyframes');
  const exported = await exportReport(page, detail);
  expect(exported.results).toHaveLength(6);
  expect(exported.plan.judge.reasoningEffort).toBe('low');
  const artworks = exported.results.filter((result: { caseId: string }) => result.caseId === 'pelican');
  expect(artworks).toHaveLength(2);
  for (const artwork of artworks) {
    expect(artwork).toMatchObject({ status: 'generated', score: null, checks: [], judge: null, prompt: pelicanPrompt });
    expect(artwork.artifactHtml).toContain('<animateTransform');
  }
  await closeDialog(page);
  await timeline(page, '糖果推理');
  const alternate = page.getByRole('region', { name: '备用渠道 example-code 糖果推理 时间线', exact: true });
  await expect(alternate.locator('[data-state-value="failed"]')).toHaveCount(1);
  await alternate.locator('[data-state-value="failed"]').click();
  const candy = page.getByRole('dialog', { name: '糖果推理', exact: true });
  await expect(candy.locator('.evaluation-outcome')).toContainText('未通过');
  await expect(candy.locator('.evaluation-answer')).toHaveText('{"answer":20}');
  await expect(candy.getByRole('heading', { name: '模型复评', exact: true })).toBeVisible();
  await candy.getByRole('tab', { name: '题目', exact: true }).click();
  await expect(candy.locator('.evaluation-raw')).toContainText('演示题目');
  await candy.getByRole('tab', { name: '原文', exact: true }).click();
  await expect(candy.locator('.evaluation-raw')).toHaveText('{"answer":20}');
  await closeDialog(page);
  await timeline(page, '模型判题');
  await expect(page.locator('[data-state-value="passed"]')).toHaveCount(2);
  await page.locator('[data-state-value="passed"]').first().click();
  await expect(page.getByRole('dialog').locator('.evaluation-answer')).toContainText('"J6":true');
  await closeDialog(page);
  await accessible(page, 'desktop timelines');
  await page.screenshot({ path: 'artifacts/screenshots/v6-evaluation-timeline.png' });
  expect(externalRequests).toEqual([]);
  expect(errors).toEqual([]);
});

test('cancelled runs remain inspectable and a later run completes', async ({ page }) => {
  const drawer = await openPlan(page);
  await addAlternate(page, drawer);
  await allCases(drawer);
  await start(page, drawer);
  await page.getByRole('button', { name: '停止', exact: true }).click();
  await expect(progress(page)).toHaveCount(0);
  const cancelled = await history(page);
  await expect(cancelled.locator('.evaluation-history-row')).toHaveCount(1);
  await expect(cancelled.locator('.evaluation-history-state')).toHaveText('已取消');
  await closeDialog(page);
  await start(page, await openPlan(page));
  await complete(page);
  const list = await history(page);
  await expect(list.locator('.evaluation-history-row')).toHaveCount(2);
  await expect(list.locator('.evaluation-history-state').first()).toHaveText('已完成');
  await expect(list.locator('.evaluation-history-state').last()).toHaveText('已取消');
});

test('history survives navigation and compares the matching scored case', async ({ page }) => {
  const drawer = await openPlan(page);
  await allCases(drawer);
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await start(page, await openPlan(page));
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await navigation(page, '渠道').click();
  await navigation(page, '评测').click();
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await start(page, await openPlan(page));
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(2);
  await timeline(page, '糖果推理', 1);
  const sharedSlot = page.locator('.evaluation-timeline-card').getByRole('button', { name: /，2 次检测$/ });
  await expect(sharedSlot).toHaveCount(1);
  const selectedRunIds = new Set<string>();
  for (const index of [0, 1]) {
    await sharedSlot.click();
    const slotRecords = page.getByRole('dialog', { name: '时段检测记录', exact: true });
    await expect(slotRecords.locator('.evaluation-slot-record')).toHaveCount(2);
    await slotRecords.locator('.evaluation-slot-record').nth(index).click();
    const selectedReport = page.getByRole('dialog', { name: '糖果推理', exact: true });
    await expect(selectedReport.locator('.evaluation-answer')).toHaveText('{"answer":21}');
    const selectedExport = await exportReport(page, selectedReport);
    expect(selectedExport).toMatchObject({ status: 'completed', caseVersion: 'vela-demo-2', targetCount: 1, completedCases: 3 });
    expect(typeof selectedExport.id).toBe('string');
    selectedRunIds.add(selectedExport.id);
    await closeDialog(page);
  }
  expect(selectedRunIds.size).toBe(2);
  const report = await latestReport(page);
  await report.getByRole('combobox', { name: '对比历史评测', exact: true }).click();
  await page.getByRole('option').filter({ hasText: '1 个模型' }).click();
  await expect(report.locator('.evaluation-comparison')).toContainText('0 较对比记录 · 相同模型、档位与测试项目');
  const exported = await exportReport(page, report);
  expect(exported).toMatchObject({ status: 'completed', caseVersion: 'vela-demo-2', targetCount: 1, completedCases: 3 });
  expect(selectedRunIds.has(exported.id)).toBe(true);
  await report.getByRole('combobox', { name: '选择评测结果', exact: true }).click();
  await page.getByRole('option').filter({ hasText: '鹈鹕动画' }).click();
  await expect(report.getByRole('combobox', { name: '对比历史评测', exact: true })).toHaveCount(0);
  await closeDialog(page);
  const list = await history(page);
  await list.locator('.evaluation-history-row').last().click();
  await expect(page.getByRole('dialog').getByRole('combobox', { name: '对比历史评测', exact: true })).toHaveText('不对比历史');
});

test('30 minute schedule saves without running and can be paused', async ({ page }) => {
  const drawer = await openPlan(page);
  await drawer.getByRole('checkbox', { name: '定时评测', exact: true }).check();
  await expect(drawer.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 30 分钟');
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('每 30 分钟自动评测');
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('演示下次');
  await expect(progress(page)).toHaveCount(0);
  await expect(page.getByTestId('pelican-card')).toHaveCount(0);
  const list = await history(page);
  await expect(list.locator('.evaluation-history-row')).toHaveCount(0);
  await closeDialog(page);
  await openPlan(page);
  await expect(drawer.getByRole('checkbox', { name: '定时评测', exact: true })).toBeChecked();
  await expect(drawer.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 30 分钟');
  await closeDialog(page);
  await page.getByRole('button', { name: '暂停定时', exact: true }).click();
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('定时评测未开启');
  await expect(page.locator('.evaluation-schedule-bar')).not.toContainText('演示下次');
});

test('legacy hourly plans preserve their interval and two minute timeout', async ({ page }) => {
  await page.evaluate(async () => {
    const { evaluationApi } = await import('/src/lib/' + 'evaluation.ts');
    const current = await evaluationApi.dashboard();
    await evaluationApi.save({ ...current.plan, targets: [{ profileId: 'demo-work', modelId: 'example-code', reasoningEffort: null }], scheduleEnabled: true, intervalHours: 6, intervalMinutes: null, requestTimeoutSeconds: 120 });
  });
  await page.getByRole('button', { name: '刷新评测记录', exact: true }).click();
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('每 6 小时自动评测');
  const drawer = await openPlan(page);
  await expect(drawer.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 6 小时');
  await expect(drawer.getByRole('combobox', { name: '单次请求超时', exact: true })).toHaveText('2 分钟');
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await expect(page.locator('.evaluation-schedule-bar')).toContainText('每 6 小时自动评测');
});

test('custom timeouts validate and keep saved schedule separate from manual run', async ({ page }) => {
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
  await seconds.fill('125');
  await drawer.getByRole('checkbox', { name: '定时评测', exact: true }).check();
  await page.setViewportSize({ width: 390, height: 780 });
  await fits(page, 'custom timeout at 390 pixels');
  await accessible(page, 'custom timeout at 390 pixels');
  await seconds.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'artifacts/screenshots/v6-evaluation-timeout.png' });
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await openPlan(page);
  await expect(timeout).toHaveText('自定义');
  await expect(seconds).toHaveValue('125');
  await expect(drawer.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 30 分钟');
  await choose(page, timeout, '10 分钟');
  await start(page, drawer);
  await complete(page);
  const exported = await exportReport(page, await latestReport(page));
  expect(exported.plan.requestTimeoutSeconds).toBe(600);
  expect(exported.plan.intervalMinutes).toBe(30);
  await closeDialog(page);
  await openPlan(page);
  await expect(timeout).toHaveText('自定义');
  await expect(seconds).toHaveValue('125');
});

test('plan rejects missing models or cases before creating a run', async ({ page }) => {
  const drawer = await openPlan(page);
  await drawer.getByRole('button', { name: '移除被测模型 example-code', exact: true }).click();
  await drawer.getByRole('button', { name: '开始本次评测', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请至少选择一个模型。');
  await choose(page, drawer.getByRole('combobox', { name: '添加被测模型', exact: true }), /主力渠道 · example-code/);
  await drawer.getByRole('button', { name: '添加所选模型', exact: true }).click();
  await drawer.getByRole('checkbox', { name: /^糖果推理/ }).uncheck();
  await drawer.getByRole('checkbox', { name: /^鹈鹕动画/ }).uncheck();
  await drawer.getByRole('button', { name: '开始本次评测', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请至少选择一项测试。');
  await closeDialog(page);
  await expect((await history(page)).locator('.evaluation-history-row')).toHaveCount(0);
});

test('seeded gallery lazily opens 72 works and exposes distinct timeline outcomes', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto('/?evaluationDemo=gallery');
  await navigation(page, '评测').click();
  await expect(page.locator('[aria-label="筛选渠道"]').getByRole('button', { name: /^全部渠道/ })).toContainText('72');
  await expect(page.locator('.evaluation-channel-group')).toHaveCount(2);
  await expect(page.getByTestId('pelican-card')).toHaveCount(40);
  await expect(page.locator('iframe').first()).toBeVisible();
  expect(await page.locator('iframe').count()).toBeLessThan(40);
  const filters = page.locator('[aria-label="筛选渠道"]');
  await filters.getByRole('button', { name: /^备用渠道/ }).click();
  await expect(page.locator('.evaluation-channel-group')).toHaveCount(1);
  await expect(page.locator('.evaluation-channel-heading')).toContainText('36 个作品');
  await page.getByRole('button', { name: '查看更早的 16 个作品', exact: true }).click();
  await expect(page.getByTestId('pelican-card')).toHaveCount(36);
  await page.getByTestId('pelican-card').last().getByRole('button', { name: '查看 example-code 鹈鹕动画 结果', exact: true }).click();
  await expect(page.getByRole('dialog')).toContainText('此作品来自旧版 SVG 记录。');
  await closeDialog(page);
  await timeline(page, '糖果推理');
  for (const state of ['passed', 'failed', 'error', 'empty']) expect(await page.locator(`[data-state-value="${state}"]`).count()).toBeGreaterThan(0);
  await page.locator('[data-state-value="error"]').first().click();
  await expect(page.getByRole('dialog').getByRole('alert')).toContainText('请求超时');
  await closeDialog(page);
  await timeline(page, '模型判题');
  await accessible(page, 'seeded 24 hour timeline');
  await page.screenshot({ path: 'artifacts/screenshots/v6-evaluation-seeded-timeline.png' });
});

test('gallery timeline and dialogs remain accessible at 390 pixels', async ({ page }) => {
  test.setTimeout(60_000);
  await page.setViewportSize({ width: 390, height: 780 });
  await fits(page, 'narrow empty evaluation');
  await accessible(page, 'narrow empty evaluation');
  const drawer = await openPlan(page);
  await drawer.getByRole('combobox', { name: '添加被测模型', exact: true }).click();
  await fits(page, 'narrow searchable channel list');
  await accessible(page, 'narrow searchable channel list');
  await page.keyboard.press('Escape');
  await expect(drawer).toBeVisible();
  await expect(drawer.getByRole('combobox', { name: '添加被测模型', exact: true })).toHaveAttribute('aria-expanded', 'false');
  await page.keyboard.press('Escape');
  await expect(drawer).toHaveCount(0);
  await openPlan(page);
  await allCases(drawer);
  await start(page, drawer);
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await fits(page, 'narrow animated gallery');
  await accessible(page, 'narrow animated gallery');
  await page.screenshot({ path: 'artifacts/screenshots/v6-evaluation-mobile.png' });
  await page.getByTestId('pelican-card').getByRole('button', { name: '查看 example-code 鹈鹕动画 结果', exact: true }).click();
  await expect(page.getByRole('dialog').locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
  await fits(page, 'narrow animated detail');
  await accessible(page, 'narrow animated detail');
  await page.screenshot({ path: 'artifacts/screenshots/v6-evaluation-mobile-detail.png' });
  await closeDialog(page);
  await timeline(page, '糖果推理', 1);
  await fits(page, 'narrow timeline');
  await accessible(page, 'narrow timeline');
});
