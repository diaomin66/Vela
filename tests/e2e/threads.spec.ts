import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

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
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '线程', exact: true }).click();
  await expect(page.getByRole('heading', { name: '线程管理', exact: true })).toBeVisible();
  await expect(page.locator('.thread-row')).toHaveCount(20);
});

async function close(page: Page) { await page.getByRole('dialog').getByRole('button', { name: '关闭弹窗', exact: true }).click(); }
async function accessible(page: Page) {
  const report = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(report.violations.map((violation) => ({ id: violation.id, targets: violation.nodes.map((node) => node.target) }))).toEqual([]);
}

test('thread pagination, filters and safe recovery are independent from healthy records', async ({ page }) => {
  await expect(page.locator('.thread-pagination')).toContainText('28 条记录');
  await page.getByRole('button', { name: '下一页线程', exact: true }).click();
  await expect(page.locator('.thread-row')).toHaveCount(8);
  await expect(page.locator('.thread-pagination')).toContainText('2 / 2');
  await page.getByRole('navigation', { name: '线程筛选' }).getByRole('button', { name: '已归档', exact: true }).click();
  await expect(page.locator('.thread-row')).toHaveCount(5);
  await page.getByRole('navigation', { name: '线程筛选' }).getByRole('button', { name: /^全部/ }).click();
  await page.getByRole('textbox', { name: '搜索线程' }).fill('知识库');
  await expect(page.locator('.thread-row')).toHaveCount(3);
  await page.locator('.thread-row').first().click();
  await expect(page.getByRole('dialog', { name: '线程详情', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '预览恢复', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '在 Codex 打开', exact: true }).click();
  await expect(page.getByRole('dialog')).toContainText('另一个数据目录');
  await close(page);
  await page.getByRole('button', { name: '清除线程搜索', exact: true }).click();
  await page.getByRole('navigation', { name: '线程筛选' }).getByRole('button', { name: /^待处理/ }).click();
  await expect(page.locator('.thread-row')).toHaveCount(2);
  await page.locator('.thread-row').filter({ hasText: '可找回' }).click();
  await expect(page.getByRole('button', { name: '在 Codex 打开', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '预览恢复', exact: true }).click();
  const restore = page.getByRole('dialog', { name: '恢复线程', exact: true });
  await expect(restore.getByRole('button', { name: '确认恢复', exact: true })).toBeEnabled();
  await expect(restore.locator('.thread-restore-destination')).toContainText('rollout-');
  await restore.getByRole('button', { name: '取消', exact: true }).click();
  await expect(page.locator('.thread-row').filter({ hasText: '可找回' })).toHaveCount(1);
  await page.locator('.thread-row').filter({ hasText: '可找回' }).click();
  await page.getByRole('button', { name: '预览恢复', exact: true }).click();
  await page.getByRole('dialog').getByRole('button', { name: '确认恢复', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('.thread-row')).toHaveCount(1);
  await expect(page.locator('.thread-completed')).toContainText('线程文件已恢复');
  await accessible(page);
});

test('protection settings persist across navigation and reindex uses an explicit source action', async ({ page }) => {
  await page.getByRole('button', { name: '保护设置', exact: true }).click();
  let dialog = page.getByRole('dialog', { name: '线程保护设置', exact: true });
  await dialog.getByRole('checkbox', { name: /^自动保护/ }).uncheck();
  await dialog.getByRole('combobox', { name: '线程检查间隔', exact: true }).click();
  await page.getByRole('option', { name: '自定义', exact: true }).click();
  await dialog.getByRole('spinbutton', { name: '间隔秒数', exact: true }).fill('125');
  await dialog.getByRole('checkbox', { name: /^包含已归档线程/ }).uncheck();
  await dialog.getByRole('button', { name: '保存设置', exact: true }).click();
  await expect(page.getByRole('heading', { name: '自动保护已暂停', exact: true })).toBeVisible();
  const navigation = page.getByRole('navigation', { name: '主导航' });
  await navigation.getByRole('button', { name: '渠道', exact: true }).click();
  await navigation.getByRole('button', { name: '线程', exact: true }).click();
  await page.getByRole('button', { name: '保护设置', exact: true }).click();
  dialog = page.getByRole('dialog', { name: '线程保护设置', exact: true });
  await expect(dialog.getByRole('spinbutton', { name: '间隔秒数', exact: true })).toHaveValue('125');
  await expect(dialog.getByRole('checkbox', { name: /^自动保护/ })).not.toBeChecked();
  await accessible(page);
  await close(page);
  await page.getByRole('button', { name: /个数据来源/ }).click();
  const sources = page.getByRole('dialog', { name: '数据来源', exact: true });
  await expect(sources.getByRole('button', { name: '重新索引', exact: true }).nth(1)).toBeDisabled();
  await sources.getByRole('button', { name: '重新索引', exact: true }).first().click();
  const reconcile = page.getByRole('dialog', { name: '重新索引线程', exact: true });
  await expect(reconcile).toContainText('不会发送模型请求');
  await reconcile.getByRole('button', { name: '开始重新索引', exact: true }).click();
  await expect(reconcile).toContainText('索引检查完成');
  await expect(reconcile.locator('.thread-reconcile-counts')).toBeVisible();
  await accessible(page);
});

test('thread page and details fit light and dark compact windows', async ({ page }) => {
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
    await page.setViewportSize({ width: 1180, height: 900 });
    await accessible(page);
    await page.screenshot({ path: `artifacts/screenshots/thread-manager-${theme}.png` });
    await page.setViewportSize({ width: 390, height: 844 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const header = page.locator('.app-header');
    const initial = await header.boundingBox();
    await page.locator('.main-content').evaluate((element) => element.scrollTop = 500);
    expect((await header.boundingBox())?.y).toBe(initial?.y);
    await page.locator('.main-content').evaluate((element) => element.scrollTop = 0);
    await accessible(page);
    await page.locator('.thread-row').first().click();
    await expect(page.getByRole('dialog', { name: '线程详情', exact: true })).toBeVisible();
    await page.getByText('文件与标识', { exact: true }).click();
    const bounds = await page.getByRole('dialog').evaluate((element) => ({ width: element.scrollWidth, client: element.clientWidth, right: element.getBoundingClientRect().right }));
    expect(bounds.width).toBeLessThanOrEqual(bounds.client + 1);
    expect(bounds.right).toBeLessThanOrEqual(390);
    await accessible(page);
    await page.screenshot({ path: `artifacts/screenshots/thread-detail-${theme}-390.png` });
    await close(page);
  }
});

test('batch recovery previews conflicts, retains failed records and requires a fresh preview before retry', async ({ page }) => {
  await page.goto('/?threadDemo=batch');
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '线程', exact: true }).click();
  await expect(page.getByRole('checkbox', { name: /^选择找回/ })).toHaveCount(3);
  await page.getByRole('checkbox', { name: '选择本页可找回', exact: true }).check();
  await page.getByRole('button', { name: '预览找回 3 条', exact: true }).click();
  let dialog = page.getByRole('dialog', { name: '批量找回线程', exact: true });
  await expect(dialog.locator('.thread-batch-summary')).toContainText('可恢复 2 条 · 1 条暂不可用');
  await expect(dialog).toContainText('目标位置已有不同内容');
  await accessible(page);
  await dialog.getByRole('button', { name: '确认找回 2 条', exact: true }).click();
  await expect(dialog.locator('.thread-batch-summary')).toContainText('已找回 1 条 · 2 条待处理');
  await expect(dialog.locator('[data-state="success"]')).toHaveCount(1);
  await expect(dialog.locator('[data-state="failed"]')).toHaveCount(1);
  await expect(dialog.locator('[data-state="blocked"]')).toHaveCount(1);
  await expect(dialog.getByRole('button', { name: /^确认找回/ })).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '重新预览 2 条', exact: true })).toBeEnabled();
  await dialog.getByRole('button', { name: '完成', exact: true }).click();
  await expect(page.getByRole('button', { name: '预览找回 2 条', exact: true })).toBeVisible();
  await expect(page.getByRole('checkbox', { name: /^选择找回/ })).toHaveCount(2);
  await expect(page.getByRole('checkbox', { name: /^选择找回/ }).first()).toBeChecked();
  await page.getByRole('button', { name: '预览找回 2 条', exact: true }).click();
  dialog = page.getByRole('dialog', { name: '批量找回线程', exact: true });
  await expect(dialog.locator('.thread-batch-summary')).toContainText('可恢复 1 条 · 1 条暂不可用');
  await dialog.getByRole('button', { name: '确认找回 1 条', exact: true }).click();
  await expect(dialog.locator('.thread-batch-summary')).toContainText('已找回 1 条 · 1 条待处理');
  await dialog.getByRole('button', { name: '完成', exact: true }).click();
  await expect(page.getByRole('button', { name: '预览找回 1 条', exact: true })).toBeVisible();
  await expect(page.getByRole('checkbox', { name: /^选择找回/ })).toHaveCount(1);
});
