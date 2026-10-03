import { test, expect } from '@playwright/test';
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
  await expect(page.getByRole('heading', { name: '渠道管理', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Vela 设置', exact: true }).click();
});

test('preview update stays in memory and waits for the user to install', async ({ page }) => {
  const requests: string[] = [];
  page.on('request', (request) => { if (new URL(request.url()).hostname !== '127.0.0.1') requests.push(request.url()); });
  const panel = page.getByRole('region', { name: '软件更新', exact: true });
  await expect(panel.getByText('演示模式 · 仅模拟更新，不下载或安装软件。')).toBeVisible();
  await panel.getByRole('button', { name: '检查更新' }).click();
  await expect(panel.getByRole('progressbar', { name: '更新下载进度' })).toBeVisible();
  await page.getByRole('button', { name: '取消', exact: true }).click();
  const badge = page.getByRole('button', { name: '更新已就绪', exact: true });
  await expect(badge).toBeVisible();
  await badge.click();
  await expect(panel.getByText('安装会暂时停止本地转发，请在当前任务结束后继续。')).toBeVisible();
  await expect(panel.getByRole('button', { name: '安装并重启', exact: true })).toBeEnabled();
  await page.screenshot({ path: 'artifacts/screenshots/v4-update-ready.png' });
  const accessibility = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(accessibility.violations.map((v) => ({ id: v.id, targets: v.nodes.map((node) => node.target) }))).toEqual([]);
  await panel.getByRole('button', { name: '安装并重启', exact: true }).click();
  await expect(panel.getByText('已是最新版本', { exact: true })).toBeVisible();
  await expect(badge).toHaveCount(0);
  expect(requests).toEqual([]);
});

test('automatic download preference saves independently of channel settings', async ({ page }) => {
  const panel = page.getByRole('region', { name: '软件更新', exact: true });
  await panel.getByRole('checkbox', { name: '自动下载新版本', exact: true }).uncheck();
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await page.getByRole('button', { name: 'Vela 设置', exact: true }).click();
  await expect(panel.getByRole('checkbox', { name: '自动下载新版本', exact: true })).not.toBeChecked();
  await panel.getByRole('button', { name: '检查更新', exact: true }).click();
  await expect(panel.getByText('发现新版本', { exact: true })).toBeVisible();
  await expect(panel.getByRole('progressbar')).toHaveCount(0);
  await panel.getByRole('button', { name: '下载更新', exact: true }).click();
  await expect(panel.getByRole('button', { name: '安装并重启', exact: true })).toBeEnabled();
});

test('update panel and ready badge fit a narrow window', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 780 });
  const panel = page.getByRole('region', { name: '软件更新', exact: true });
  await panel.getByRole('button', { name: '检查更新', exact: true }).click();
  await expect(panel.getByRole('button', { name: '安装并重启', exact: true })).toBeEnabled();
  await panel.getByText('版本说明', { exact: true }).click();
  await page.screenshot({ path: 'artifacts/screenshots/v4-update-mobile.png' });
  const bounds = await page.getByRole('dialog').evaluate((dialog) => ({ scrollWidth: dialog.scrollWidth, clientWidth: dialog.clientWidth, right: dialog.getBoundingClientRect().right }));
  expect(bounds.scrollWidth).toBeLessThanOrEqual(bounds.clientWidth);
  expect(bounds.right).toBeLessThanOrEqual(390);
  const accessibility = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(accessibility.violations.map((v) => ({ id: v.id, targets: v.nodes.map((node) => node.target) }))).toEqual([]);
  await page.getByRole('button', { name: '取消', exact: true }).click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await expect(page.getByRole('button', { name: '更新已就绪', exact: true })).toBeInViewport();
});

test('maximum reasoning and opt-in none survive saving and reopening model settings', async ({ page }) => {
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await page.locator('.channel-card').filter({ has: page.getByRole('heading', { name: '主力渠道', exact: true }) }).getByRole('button', { name: '管理模型', exact: true }).click();
  const editor = page.getByRole('dialog');
  await editor.getByRole('button', { name: 'example-code 的模型设置', exact: true }).click();
  const presets = editor.getByRole('combobox', { name: 'example-code 的推理档位', exact: true });
  await presets.click();
  await page.getByRole('option', { name: '低 · 中 · 高 · 超高 · 最高', exact: true }).click();
  const strength = editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true });
  await strength.click();
  await expect(page.getByRole('option')).toHaveText(['模型默认', '低', '中', '高', '超高', '最高']);
  await page.getByRole('option', { name: '最高', exact: true }).click();
  await presets.click();
  await page.getByRole('option', { name: '自定义档位', exact: true }).click();
  await editor.getByRole('checkbox', { name: '关闭', exact: true }).focus();
  await page.keyboard.press('Space');
  await expect(editor.getByRole('checkbox', { name: '关闭', exact: true })).toBeChecked();
  await editor.getByRole('button', { name: '保存渠道', exact: true }).click();
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '模型库', exact: true }).click();
  const model = page.getByRole('region', { name: '主力渠道', exact: true }).locator('.catalog-row[data-model="example-code"]');
  await expect(model.getByRole('combobox')).toHaveText('最高');
  await model.getByRole('combobox').click();
  await expect(page.getByRole('option')).toHaveText(['关闭', '低', '中', '高', '超高', '最高']);
  await page.keyboard.press('Escape');
  await model.locator('.catalog-model').click();
  await expect(editor.getByRole('combobox', { name: 'example-code 的推理档位', exact: true })).toHaveText('自定义档位');
  await expect(editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true })).toHaveText('最高');
  await expect(editor.getByRole('checkbox', { name: '最高', exact: true })).toBeChecked();
  await expect(editor.getByRole('checkbox', { name: '关闭', exact: true })).toBeChecked();
});
