import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const settings = (page: Page) => page.getByRole('button', { name: 'AhaX 设置', exact: true });
const theme = (page: Page) => page.locator('html');
const navigation = (page: Page, label: string) => page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: label, exact: true });

test.beforeEach(async ({ page, request }) => {
  if (process.env.VELA_E2E_LOCAL_PROXY === '1') await page.route('http://127.0.0.1:1420/**', async (route) => {
    const input = route.request();
    if (input.method() !== 'GET' || !['document', 'script', 'stylesheet', 'font', 'image'].includes(input.resourceType())) return route.continue();
    const response = await request.get(input.url(), { maxRedirects: 0, headers: { accept: input.headers().accept ?? '*/*' } });
    const headers = response.headers();
    for (const name of ['connection', 'content-encoding', 'content-length', 'transfer-encoding']) delete headers[name];
    await route.fulfill({ status: response.status(), headers, body: await response.body() });
  });
});

test('appearance follows the system and persists an explicit preference across reloads', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/');
  await expect(theme(page)).toHaveAttribute('data-theme', 'dark');
  await expect(theme(page)).toHaveAttribute('data-theme-mode', 'system');
  await settings(page).click();
  await page.getByRole('radio', { name: '浅色', exact: true }).check();
  await expect(theme(page)).toHaveAttribute('data-theme', 'light');
  await page.reload();
  await expect(theme(page)).toHaveAttribute('data-theme', 'light');
  await expect(theme(page)).toHaveAttribute('data-theme-mode', 'light');
  await settings(page).click();
  await page.getByRole('radio', { name: '跟随系统', exact: true }).check();
  await expect(theme(page)).toHaveAttribute('data-theme', 'dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(theme(page)).toHaveAttribute('data-theme', 'light');
  await page.getByRole('radio', { name: '深色', exact: true }).check();
  await expect(theme(page)).toHaveAttribute('data-theme', 'dark');
  await expect(page.getByRole('radio', { name: '深色', exact: true })).toBeChecked();
  await page.screenshot({ path: 'artifacts/screenshots/v7-settings-dark.png' });
});

for (const mode of ['light', 'dark'] as const) {
  test(`${mode} surfaces remain accessible across navigation and narrow layouts`, async ({ page }) => {
    test.setTimeout(90_000);
    await page.addInitScript((value) => localStorage.setItem('vela:appearance:v1', value), mode);
    await page.goto('/?evaluationDemo=gallery');
    await expect(page.getByRole('heading', { name: '渠道管理', exact: true })).toBeVisible();
    await expect(theme(page)).toHaveAttribute('data-theme', mode);
    await page.screenshot({ path: `artifacts/screenshots/v7-channels-${mode}.png` });
    for (const label of ['渠道', '模型库', '评测', '诊断', '恢复']) {
      await navigation(page, label).click();
      const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
      expect(result.violations, `${mode} ${label}`).toEqual([]);
    }
    await navigation(page, '评测').click();
    await page.getByRole('button', { name: '开始检测', exact: true }).click();
    const plan = page.getByRole('dialog', { name: '新建检测', exact: true });
    await plan.getByRole('button', { name: '开始检测', exact: true }).click();
    await expect(page.getByRole('region', { name: '评测进度', exact: true })).toHaveCount(0);
    await expect(page.getByTestId('pelican-card')).toHaveCount(1);
    await expect(page.getByTestId('pelican-card').locator('.artifact-preview')).toHaveAttribute('data-ready', 'true');
    await page.screenshot({ path: `artifacts/screenshots/v7-gallery-${mode}.png` });
    await page.getByRole('tab', { name: '糖果推理', exact: true }).click();
    await expect(page.getByTestId('manual-results')).toBeVisible();
    await page.screenshot({ path: `artifacts/screenshots/v7-manual-${mode}.png` });
    await page.locator('.evaluation-answer-row').first().click();
    const detail = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
    expect(detail.violations, `${mode} result detail`).toEqual([]);
    await page.getByRole('button', { name: '删除本轮评测', exact: true }).click();
    const deletion = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
    expect(deletion.violations, `${mode} delete confirmation`).toEqual([]);
    await page.screenshot({ path: `artifacts/screenshots/v7-deletion-${mode}.png` });
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await page.getByRole('navigation', { name: '评测子导航' }).getByRole('button', { name: '定时评测', exact: true }).click();
    await page.screenshot({ path: `artifacts/screenshots/v7-scheduled-${mode}.png` });
    await expect(page.getByRole('button', { name: /暂停.*动画|播放.*动画/ })).toHaveCount(0);
    const headerTop = await page.locator('.app-header').evaluate((node) => node.getBoundingClientRect().top);
    await page.locator('.main-content').evaluate((node) => { node.scrollTop = node.scrollHeight; });
    expect(await page.locator('.app-header').evaluate((node) => node.getBoundingClientRect().top)).toBe(headerTop);
    await page.setViewportSize({ width: 390, height: 780 });
    await page.locator('.main-content').evaluate((node) => { node.scrollTop = 0; });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const narrow = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
    expect(narrow.violations, `${mode} narrow scheduled`).toEqual([]);
    await page.screenshot({ path: `artifacts/screenshots/v7-scheduled-mobile-${mode}.png` });
    await settings(page).click();
    const drawer = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
    expect(drawer.violations, `${mode} narrow settings`).toEqual([]);
    await page.screenshot({ path: `artifacts/screenshots/v7-settings-mobile-${mode}.png` });
  });
}

test('saved appearance applies before the application module executes', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.addInitScript(() => localStorage.setItem('vela:appearance:v1', 'dark'));
  await page.route('**/src/main.tsx', (route) => route.abort());
  await page.goto('/');
  await expect(theme(page)).toHaveAttribute('data-theme', 'dark');
  await expect(page.locator('#root')).toBeEmpty();
  await expect(theme(page)).toHaveCSS('color-scheme', 'dark');
  await expect(theme(page)).toHaveCSS('background-color', 'rgb(23, 25, 30)');
});
