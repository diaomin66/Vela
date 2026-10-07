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
  await expect(page.getByRole('heading', { name: '单次检测', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '开始检测', exact: true })).toBeEnabled();
});

async function mode(page: Page, name: '单次检测' | '定时评测') {
  await page.getByRole('navigation', { name: '评测子导航', exact: true }).getByRole('button', { name, exact: true }).click();
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
}
async function openManual(page: Page) {
  await page.getByRole('button', { name: '开始检测', exact: true }).click();
  return page.getByRole('dialog', { name: '新建检测', exact: true });
}
async function openSchedule(page: Page) {
  await page.getByRole('button', { name: '编辑计划', exact: true }).click();
  return page.getByRole('dialog', { name: '定时计划', exact: true });
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
  await drawer.getByRole('button', { name: '开始检测', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(progress(page)).toBeVisible();
}
async function complete(page: Page) {
  await expect(progress(page)).toHaveCount(0, { timeout: 15_000 });
  await expect(page.getByRole('button', { name: '开始检测', exact: true })).toBeEnabled();
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
  expect(download.suggestedFilename()).toMatch(/^ahax-evaluation-.+\.json$/);
  return JSON.parse(await readFile((await download.path())!, 'utf8'));
}
async function accessible(page: Page, state: string) {
  const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(result.violations.map((violation) => ({ id: violation.id, targets: violation.nodes.map((node) => node.target) })), state).toEqual([]);
}
async function fits(page: Page, state: string) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), state).toBe(true);
  for (const bounds of await page.locator('[role="dialog"], .ahax-select-popup:popover-open').evaluateAll((elements) => elements.map((element) => ({ width: element.scrollWidth, client: element.clientWidth, left: element.getBoundingClientRect().left, right: element.getBoundingClientRect().right, window: innerWidth })))) {
    expect(bounds.width, state).toBeLessThanOrEqual(bounds.client + 1);
    expect(bounds.left, state).toBeGreaterThanOrEqual(0);
    expect(bounds.right, state).toBeLessThanOrEqual(bounds.window + 1);
  }
}
async function selectCase(page: Page, name: string) {
  await page.getByRole('tablist', { name: '评测项目', exact: true }).getByRole('tab', { name, exact: true }).click();
}
async function timeline(page: Page, name: string, count = 2) {
  await selectCase(page, name);
  await expect(page.locator('.evaluation-timeline-card')).toHaveCount(count);
  for (const card of await page.locator('.evaluation-timeline-card').all()) {
    await expect(card.getByRole('group', { name: '最近 24 小时，每格 30 分钟', exact: true }).getByRole('button')).toHaveCount(48);
  }
}

test('manual run shows animated works and individual candy and judgment results', async ({ page }) => {
  test.setTimeout(60_000);
  const externalRequests: string[] = [];
  const errors: string[] = [];
  page.on('request', (request) => { const url = new URL(request.url()); if (/^https?:$/.test(url.protocol) && url.hostname !== '127.0.0.1') externalRequests.push(url.href); });
  page.on('pageerror', (error) => errors.push(error.message));
  await expect(page.getByRole('heading', { name: '开始一次鹈鹕动画', exact: true })).toBeVisible();
  await expect(page.locator('iframe')).toHaveCount(0);
  const drawer = await openManual(page);
  await expect(drawer.getByRole('button', { name: '保存计划', exact: true })).toHaveCount(0);
  await expect(drawer.getByRole('checkbox', { name: '启用定时', exact: true })).toHaveCount(0);
  await addAlternate(page, drawer);
  await allCases(drawer);
  await choose(page, drawer.locator('.evaluation-target').first().getByRole('combobox'), '高');
  await choose(page, drawer.getByRole('combobox', { name: '评审模型', exact: true }), /备用渠道 · example-reasoning/);
  await choose(page, drawer.getByRole('combobox', { name: '评审模型推理强度', exact: true }), '低');
  await expect(drawer.locator('.evaluation-cost')).toContainText('10 次请求，不发送 API 请求');
  await accessible(page, 'manual setup with two channels and independent reviewer');
  await start(page, drawer);
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(2);
  for (const card of await page.getByTestId('pelican-card').all()) {
    await card.scrollIntoViewIfNeeded();
    await expect(card.locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
    await expect(card.frameLocator('iframe').locator('svg')).toBeVisible();
    const wheel = card.frameLocator('iframe').locator('.wheel').first();
    const transform = await wheel.evaluate((element) => getComputedStyle(element).transform);
    await expect.poll(() => wheel.evaluate((element) => getComputedStyle(element).transform)).not.toBe(transform);
  }
  await expect(page.getByRole('button', { name: /暂停动画/ })).toHaveCount(0);
  const filters = page.locator('[aria-label="筛选渠道"]');
  await filters.getByRole('button', { name: /^主力渠道/ }).click();
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await expect(page.getByTestId('pelican-card').locator('.evaluation-effort')).toHaveText('推理 高');
  await filters.getByRole('button', { name: /^全部渠道/ }).click();
  await page.getByTestId('pelican-card').first().getByRole('button', { name: '查看 example-code 鹈鹕动画 结果', exact: true }).click();
  const detail = page.getByRole('dialog', { name: '鹈鹕动画', exact: true });
  await expect(detail.locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
  await expect(detail.getByRole('button', { name: /暂停/ })).toHaveCount(0);
  await detail.getByRole('button', { name: '重新播放', exact: true }).click();
  await expect(detail.locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
  await expect(detail.getByRole('heading', { name: '模型复评', exact: true })).toHaveCount(0);
  await detail.getByRole('tab', { name: '题目', exact: true }).click();
  await expect(detail.locator('.evaluation-raw')).toHaveText(pelicanPrompt);
  await detail.getByRole('tab', { name: '原文', exact: true }).click();
  await expect(detail.locator('.evaluation-raw')).toContainText('@keyframes');
  const exported = await exportReport(page, detail);
  expect(exported.results).toHaveLength(6);
  expect(exported.trigger).toBe('manual');
  expect(exported.plan.scheduleEnabled).toBe(false);
  expect(exported.plan.judge.reasoningEffort).toBe('low');
  for (const artwork of exported.results.filter((result: { caseId: string }) => result.caseId === 'pelican')) {
    expect(artwork).toMatchObject({ status: 'generated', score: null, checks: [], judge: null, prompt: pelicanPrompt });
    expect(artwork.artifactHtml).toContain('<animateTransform');
  }
  await closeDialog(page);
  await selectCase(page, '糖果推理');
  await expect(page.getByTestId('manual-results').getByRole('button')).toHaveCount(2);
  await expect(page.locator('.evaluation-timeline-card')).toHaveCount(0);
  await page.getByTestId('manual-results').getByRole('button').filter({ hasText: '备用渠道' }).click();
  const candy = page.getByRole('dialog', { name: '糖果推理', exact: true });
  await expect(candy.locator('.evaluation-outcome')).toContainText('未通过');
  await expect(candy.locator('.evaluation-answer')).toHaveText('{"answer":20}');
  await expect(candy.getByRole('heading', { name: '模型复评', exact: true })).toBeVisible();
  await closeDialog(page);
  await selectCase(page, '模型判题');
  await expect(page.getByTestId('manual-results').getByRole('button')).toHaveCount(2);
  await page.getByTestId('manual-results').getByRole('button').first().click();
  await expect(page.getByRole('dialog').locator('.evaluation-answer')).toContainText('"J6":true');
  await closeDialog(page);
  await accessible(page, 'desktop individual results');
  await page.screenshot({ path: 'artifacts/screenshots/v7-manual-results.png' });
  expect(externalRequests).toEqual([]);
  expect(errors).toEqual([]);
});

test('active records cannot be deleted and cancelled runs remain inspectable', async ({ page }) => {
  const drawer = await openManual(page);
  await addAlternate(page, drawer);
  await allCases(drawer);
  await start(page, drawer);
  const running = await history(page);
  const active = running.locator('.evaluation-history-item').filter({ hasText: '进行中' });
  await expect(active.getByRole('checkbox')).toBeDisabled();
  await expect(active.getByRole('button', { name: /^删除/ })).toBeDisabled();
  await active.locator('.evaluation-history-row').click();
  await expect(page.getByRole('dialog').getByRole('button', { name: '删除本轮评测', exact: true })).toBeDisabled();
  await closeDialog(page);
  await page.getByRole('button', { name: '停止', exact: true }).click();
  await expect(progress(page)).toHaveCount(0);
  const cancelled = await history(page);
  await expect(cancelled.locator('.evaluation-history-state')).toHaveText('已取消');
  await closeDialog(page);
  await start(page, await openManual(page));
  await complete(page);
  const list = await history(page);
  await expect(list.locator('.evaluation-history-row')).toHaveCount(2);
  await expect(list.locator('.evaluation-history-state').first()).toHaveText('已完成');
  await expect(list.locator('.evaluation-history-state').last()).toHaveText('已取消');
});

test('manual history survives navigation and compares the matching scored case', async ({ page }) => {
  const drawer = await openManual(page);
  await allCases(drawer);
  await start(page, drawer);
  await complete(page);
  await navigation(page, '渠道').click();
  await navigation(page, '评测').click();
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await start(page, await openManual(page));
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(2);
  await selectCase(page, '糖果推理');
  await expect(page.getByTestId('manual-results').getByRole('button')).toHaveCount(2);
  const selectedIds = new Set<string>();
  for (const index of [0, 1]) {
    await page.getByTestId('manual-results').getByRole('button').nth(index).click();
    const selected = page.getByRole('dialog', { name: '糖果推理', exact: true });
    await expect(selected.locator('.evaluation-answer')).toHaveText('{"answer":21}');
    selectedIds.add((await exportReport(page, selected)).id);
    await closeDialog(page);
  }
  expect(selectedIds.size).toBe(2);
  const report = await latestReport(page);
  await choose(page, report.getByRole('combobox', { name: '对比历史评测', exact: true }), /1 个模型/);
  await expect(report.locator('.evaluation-comparison')).toContainText('0 较对比记录 · 相同模型、档位与测试项目');
  const exported = await exportReport(page, report);
  expect(exported).toMatchObject({ status: 'completed', caseVersion: 'ahax-demo-2', targetCount: 1 });
  expect(selectedIds.has(exported.id)).toBe(true);
  await report.getByRole('combobox', { name: '选择评测结果', exact: true }).click();
  await page.getByRole('option').filter({ hasText: '鹈鹕动画' }).click();
  await expect(report.getByRole('combobox', { name: '对比历史评测', exact: true })).toHaveCount(0);
});

test('scheduled setup saves without starting a manual run and can pause or resume', async ({ page }) => {
  await mode(page, '定时评测');
  const drawer = await openSchedule(page);
  await expect(drawer.getByRole('button', { name: '开始检测', exact: true })).toHaveCount(0);
  await expect(drawer.getByRole('checkbox', { name: '启用定时', exact: true })).toBeChecked();
  await expect(drawer.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 30 分钟');
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  const summary = page.getByRole('region', { name: '定时计划概况', exact: true });
  await expect(summary).toContainText('计划运行中');
  await expect(summary).toContainText('每 30 分钟');
  await expect(summary).toContainText('演示 ·');
  await expect(progress(page)).toHaveCount(0);
  await expect((await history(page)).locator('.evaluation-history-row')).toHaveCount(0);
  await closeDialog(page);
  await page.getByRole('button', { name: '暂停计划', exact: true }).click();
  await expect(summary).toContainText('计划已暂停');
  await expect(summary).not.toContainText('演示 ·');
  await page.getByRole('button', { name: '启用计划', exact: true }).click();
  await expect(summary).toContainText('计划运行中');
  await mode(page, '单次检测');
  await expect(page.getByRole('region', { name: '定时计划概况', exact: true })).toHaveCount(0);
  await expect(page.getByTestId('pelican-card')).toHaveCount(0);
});

test('legacy hourly plans preserve their interval and two minute timeout', async ({ page }) => {
  await page.evaluate(async () => {
    const { evaluationApi } = await import('/src/lib/' + 'evaluation.ts');
    const current = await evaluationApi.dashboard();
    await evaluationApi.save({ ...current.plan, targets: [{ profileId: 'demo-work', modelId: 'example-code', reasoningEffort: null }], scheduleEnabled: true, intervalHours: 6, intervalMinutes: null, requestTimeoutSeconds: 120 });
  });
  await page.getByRole('button', { name: '刷新评测记录', exact: true }).click();
  await mode(page, '定时评测');
  await expect(page.getByRole('region', { name: '定时计划概况', exact: true })).toContainText('每 6 小时');
  const drawer = await openSchedule(page);
  await expect(drawer.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 6 小时');
  await expect(drawer.getByRole('combobox', { name: '单次请求超时', exact: true })).toHaveText('2 分钟');
  await drawer.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(drawer).toHaveCount(0);
  await expect(page.getByRole('region', { name: '定时计划概况', exact: true })).toContainText('每 6 小时');
});

test('custom timeouts validate and manual choices never overwrite saved schedule', async ({ page }) => {
  await mode(page, '定时评测');
  const scheduled = await openSchedule(page);
  await choose(page, scheduled.getByRole('combobox', { name: '单次请求超时', exact: true }), '自定义');
  await scheduled.getByRole('spinbutton', { name: '超时秒数', exact: true }).fill('125');
  await choose(page, scheduled.getByRole('combobox', { name: '评测间隔', exact: true }), '每 3 小时');
  await scheduled.getByRole('button', { name: '保存计划', exact: true }).click();
  await expect(scheduled).toHaveCount(0);
  await mode(page, '单次检测');
  const drawer = await openManual(page);
  const timeout = drawer.getByRole('combobox', { name: '单次请求超时', exact: true });
  const seconds = drawer.getByRole('spinbutton', { name: '超时秒数', exact: true });
  await expect(seconds).toHaveValue('125');
  for (const value of ['29', '3601']) {
    await seconds.fill(value);
    await drawer.getByRole('button', { name: '开始检测', exact: true }).click();
    await expect(drawer.getByRole('alert')).toHaveText('请求超时应为 30–3600 秒的整数。');
    await expect(seconds).toHaveAttribute('aria-invalid', 'true');
  }
  await seconds.fill('300');
  await page.setViewportSize({ width: 390, height: 780 });
  await fits(page, 'custom timeout at 390 pixels');
  await accessible(page, 'custom timeout at 390 pixels');
  await choose(page, timeout, '10 分钟');
  await start(page, drawer);
  await complete(page);
  const exported = await exportReport(page, await latestReport(page));
  expect(exported.plan.requestTimeoutSeconds).toBe(600);
  expect(exported.plan.scheduleEnabled).toBe(false);
  await closeDialog(page);
  await mode(page, '定时评测');
  await openSchedule(page);
  await expect(scheduled.getByRole('spinbutton', { name: '超时秒数', exact: true })).toHaveValue('125');
  await expect(scheduled.getByRole('checkbox', { name: '启用定时', exact: true })).toBeChecked();
  await expect(scheduled.getByRole('combobox', { name: '评测间隔', exact: true })).toHaveText('每 3 小时');
});

test('manual setup rejects missing models or cases before creating a run', async ({ page }) => {
  const drawer = await openManual(page);
  await drawer.getByRole('button', { name: '移除被测模型 example-code', exact: true }).click();
  await drawer.getByRole('button', { name: '开始检测', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请至少选择一个模型。');
  await choose(page, drawer.getByRole('combobox', { name: '添加被测模型', exact: true }), /主力渠道 · example-code/);
  await drawer.getByRole('button', { name: '添加所选模型', exact: true }).click();
  await drawer.getByRole('checkbox', { name: /^糖果推理/ }).uncheck();
  await drawer.getByRole('checkbox', { name: /^鹈鹕动画/ }).uncheck();
  await drawer.getByRole('button', { name: '开始检测', exact: true }).click();
  await expect(drawer.getByRole('alert')).toHaveText('请至少选择一项测试。');
  await closeDialog(page);
  await expect((await history(page)).locator('.evaluation-history-row')).toHaveCount(0);
});

test('single and batch deletion update reports gallery and answer lists after confirmation', async ({ page }) => {
  test.setTimeout(60_000);
  for (let index = 0; index < 3; index++) {
    await start(page, await openManual(page));
    await complete(page);
  }
  await expect(page.getByTestId('pelican-card')).toHaveCount(3);
  const report = await latestReport(page);
  const exported = await exportReport(page, report);
  await report.getByRole('button', { name: '删除本轮评测', exact: true }).click();
  let confirmation = page.getByRole('dialog', { name: '删除这轮评测？', exact: true });
  await confirmation.getByRole('button', { name: '取消', exact: true }).click();
  await expect(report).toBeVisible();
  await report.getByRole('button', { name: '删除本轮评测', exact: true }).click();
  confirmation = page.getByRole('dialog', { name: '删除这轮评测？', exact: true });
  await expect(confirmation).toContainText('已导出的文件会保留');
  await confirmation.getByRole('button', { name: '确认删除', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByTestId('pelican-card')).toHaveCount(2);
  await selectCase(page, '糖果推理');
  await expect(page.getByTestId('manual-results').getByRole('button')).toHaveCount(2);
  const list = await history(page);
  await expect(list.locator('.evaluation-history-row')).toHaveCount(2);
  await list.getByRole('checkbox', { name: '选择全部评测记录', exact: true }).check();
  await list.getByRole('button', { name: /^删除所选/ }).click();
  const bulk = page.getByRole('dialog', { name: '删除 2 轮评测？', exact: true });
  await accessible(page, 'batch deletion confirmation');
  await bulk.getByRole('button', { name: '确认删除', exact: true }).click();
  await expect(bulk).toHaveCount(0);
  await expect(list.locator('.evaluation-history-row')).toHaveCount(0);
  await expect(list).toContainText('这里还没有评测记录');
  await closeDialog(page);
  await expect(page.getByTestId('manual-results')).toHaveCount(0);
  await selectCase(page, '鹈鹕动画');
  await expect(page.getByTestId('pelican-card')).toHaveCount(0);
  expect(exported.results).toHaveLength(2);
});

test('scheduled history stays separate and deleting a run removes its timeline slots', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto('/?evaluationDemo=gallery');
  await navigation(page, '评测').click();
  await expect(page.getByTestId('pelican-card')).toHaveCount(0);
  await start(page, await openManual(page));
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await expect((await history(page)).locator('.evaluation-history-row')).toHaveCount(1);
  await closeDialog(page);
  await mode(page, '定时评测');
  await timeline(page, '糖果推理');
  expect(await page.locator('.evaluation-time-block:not([data-state-value="empty"])').count()).toBe(72);
  const list = await history(page);
  await expect(list.locator('.evaluation-history-row')).toHaveCount(36);
  await list.locator('.evaluation-history-item').first().getByRole('button', { name: /^删除/ }).click();
  await page.getByRole('dialog', { name: '删除这轮评测？', exact: true }).getByRole('button', { name: '确认删除', exact: true }).click();
  await expect(list.locator('.evaluation-history-row')).toHaveCount(35);
  await closeDialog(page);
  await expect(page.locator('.evaluation-time-block:not([data-state-value="empty"])')).toHaveCount(70);
  await mode(page, '单次检测');
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await expect((await history(page)).locator('.evaluation-history-row')).toHaveCount(1);
});

test('scheduled gallery lazily opens 72 works and exposes distinct timeline outcomes', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto('/?evaluationDemo=gallery');
  await navigation(page, '评测').click();
  await mode(page, '定时评测');
  await selectCase(page, '鹈鹕动画');
  await expect(page.locator('[aria-label="筛选渠道"]').getByRole('button', { name: /^全部渠道/ })).toContainText('72');
  await expect(page.locator('.evaluation-channel-group')).toHaveCount(2);
  await expect(page.getByTestId('pelican-card')).toHaveCount(40);
  await expect(page.locator('iframe').first()).toBeVisible();
  expect(await page.locator('iframe').count()).toBeLessThan(40);
  await page.locator('[aria-label="筛选渠道"]').getByRole('button', { name: /^备用渠道/ }).click();
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
  await accessible(page, 'scheduled 24 hour timeline');
  await page.screenshot({ path: 'artifacts/screenshots/v7-scheduled-timeline.png' });
});

test('manual gallery and dialogs remain accessible at 390 pixels', async ({ page }) => {
  test.setTimeout(60_000);
  await page.setViewportSize({ width: 390, height: 780 });
  await fits(page, 'narrow empty manual page');
  await accessible(page, 'narrow empty manual page');
  const drawer = await openManual(page);
  await drawer.getByRole('combobox', { name: '添加被测模型', exact: true }).click();
  await fits(page, 'narrow searchable channel list');
  await accessible(page, 'narrow searchable channel list');
  await page.keyboard.press('Escape');
  await expect(drawer).toBeVisible();
  await expect(drawer.getByRole('combobox', { name: '添加被测模型', exact: true })).toHaveAttribute('aria-expanded', 'false');
  await page.keyboard.press('Escape');
  await expect(drawer).toHaveCount(0);
  await openManual(page);
  await allCases(drawer);
  await start(page, drawer);
  await complete(page);
  await expect(page.getByTestId('pelican-card')).toHaveCount(1);
  await fits(page, 'narrow animated gallery');
  await accessible(page, 'narrow animated gallery');
  await page.getByTestId('pelican-card').getByRole('button', { name: '查看 example-code 鹈鹕动画 结果', exact: true }).click();
  await expect(page.getByRole('dialog').locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
  await fits(page, 'narrow animated detail');
  await accessible(page, 'narrow animated detail');
  await page.screenshot({ path: 'artifacts/screenshots/v7-manual-mobile-detail.png' });
  await closeDialog(page);
  await selectCase(page, '糖果推理');
  await fits(page, 'narrow manual results');
  await accessible(page, 'narrow manual results');
});
