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
});
async function settings(page: Page, search = '') {
  await page.goto('/' + search);
  await page.getByRole('button', { name: 'AhaX 设置', exact: true }).click();
  return page.getByRole('dialog', { name: 'AhaX 设置', exact: true });
}
async function accessible(page: Page) {
  const report = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(report.violations.map((item) => ({ id: item.id, targets: item.nodes.map((node) => node.target) }))).toEqual([]);
}

test('location preview expires on edit and pending changes can be reset to real defaults', async ({ page }) => {
  const dialog = await settings(page);
  await dialog.getByRole('button', { name: '数据位置', exact: true }).click();
  const field = dialog.getByRole('textbox', { name: 'Codex 数据目录', exact: true });
  await field.fill('D:\\Codex');
  await dialog.getByRole('button', { name: '检查位置更改', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '保存位置更改', exact: true })).toBeEnabled();
  await field.fill('E:\\Codex');
  await expect(dialog.getByRole('button', { name: '保存位置更改', exact: true })).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '保存设置', exact: true })).toHaveCount(0);
  await dialog.getByRole('navigation', { name: '设置页面' }).getByRole('button', { name: '常规', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '保存设置', exact: true })).toBeDisabled();
  await dialog.getByRole('navigation', { name: '设置页面' }).getByRole('button', { name: '数据位置', exact: true }).click();
  await expect(field).toHaveValue('E:\\Codex');
  await expect(dialog.locator('.location-restart')).toHaveCount(0);
  await dialog.getByRole('button', { name: '检查位置更改', exact: true }).click();
  await dialog.getByRole('button', { name: '保存位置更改', exact: true }).click();
  await expect(dialog.locator('.location-restart')).toBeVisible();
  await expect(dialog.locator('.location-current').first()).toContainText('C:\\Users\\Demo\\.codex');
  await expect(dialog.locator('.location-next').first()).toContainText('E:\\Codex');
  await dialog.getByRole('button', { name: '关闭弹窗', exact: true }).click();
  await page.getByRole('button', { name: 'AhaX 设置', exact: true }).click();
  await dialog.getByRole('button', { name: '数据位置', exact: true }).click();
  await expect(field).toHaveValue('E:\\Codex');
  await dialog.getByRole('button', { name: '恢复默认', exact: true }).click();
  await expect(field).toHaveValue('');
  await dialog.getByRole('button', { name: '检查位置更改', exact: true }).click();
  await dialog.getByRole('button', { name: '保存位置更改', exact: true }).click();
  await expect(dialog.locator('.location-restart')).toHaveCount(0);
  await expect(dialog.locator('.location-next')).toHaveCount(0);
  await field.fill('relative-path');
  await dialog.getByRole('button', { name: '检查位置更改', exact: true }).click();
  await expect(dialog.locator('.location-preview')).toContainText('绝对路径');
  await expect(dialog.getByRole('button', { name: '保存位置更改', exact: true })).toHaveCount(0);
  await accessible(page);
});

test('location read failure is retryable and environment overrides remain visible', async ({ page }) => {
  let dialog = await settings(page, '?locationDemo=error');
  await dialog.getByRole('button', { name: '数据位置', exact: true }).click();
  await expect(dialog.getByText('数据位置暂时无法读取', { exact: true })).toBeVisible();
  await expect(dialog.getByText('正在读取位置设置', { exact: true })).toHaveCount(0);
  await dialog.getByRole('button', { name: '重新读取位置', exact: true }).click();
  await expect(dialog.getByRole('textbox', { name: 'Codex 数据目录', exact: true })).toBeEnabled();
  dialog = await settings(page, '?locationDemo=override');
  await dialog.getByRole('button', { name: '数据位置', exact: true }).click();
  await expect(dialog.getByRole('textbox', { name: 'Codex 数据目录', exact: true })).toBeDisabled();
  await expect(dialog.locator('.location-fields')).toContainText('CODEX_HOME');
  await expect(dialog.locator('.location-current').first()).toContainText('D:\\Portable\\Codex');
});

test('location settings fit compact light and dark windows with long paths', async ({ page }) => {
  const dialog = await settings(page);
  await dialog.getByRole('button', { name: '数据位置', exact: true }).click();
  await dialog.getByRole('textbox', { name: 'Codex 数据目录', exact: true }).fill('D:\\工作资料与历史项目\\Codex Data Archive\\团队项目与长期保留的会话数据');
  await dialog.getByRole('button', { name: '检查位置更改', exact: true }).click();
  await dialog.getByRole('button', { name: '保存位置更改', exact: true }).click();
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
    for (const width of [900, 390]) {
      await page.setViewportSize({ width, height: width === 900 ? 680 : 844 });
      await dialog.locator('.location-page-scroll').evaluate((element) => element.scrollTop = 0);
      const bounds = await dialog.evaluate((element) => ({ width: element.scrollWidth, client: element.clientWidth, right: element.getBoundingClientRect().right }));
      expect(bounds.width).toBeLessThanOrEqual(bounds.client + 1);
      expect(bounds.right).toBeLessThanOrEqual(width);
      await accessible(page);
      await page.screenshot({ path: `artifacts/screenshots/locations-${theme}-${width}.png` });
    }
  }
});
