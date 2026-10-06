import { test, expect, type Page } from '@playwright/test';
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
});

async function openModel(page: Page) {
  await page.locator('.channel-card').filter({ has: page.getByRole('heading', { name: '主力渠道', exact: true }) }).getByRole('button', { name: '管理模型', exact: true }).click();
  const editor = page.getByRole('dialog');
  await editor.getByRole('button', { name: 'example-code 的模型设置', exact: true }).click();
  return editor;
}

async function savedNativeReasoning(page: Page) {
  return page.evaluate(async () => {
    const modulePath = performance.getEntriesByType('resource').map((entry) => entry.name).find((name) => new URL(name).pathname === '/src/lib/api.ts');
    if (!modulePath) throw new Error('Missing preview API.');
    const { api, desktop } = await import(modulePath);
    if (desktop) throw new Error('This fixture is only for browser preview.');
    const data = await api.dashboard();
    return data.profiles.find((profile: { id: string }) => profile.id === 'demo-work').models.find((model: { id: string }) => model.id === 'example-code').nativeReasoning;
  });
}

test('Ultra remains distinct from max and persists as a native default', async ({ page }) => {
  const editor = await openModel(page);
  await editor.getByRole('combobox', { name: 'example-code 的推理档位', exact: true }).click();
  await page.getByRole('option').filter({ hasText: '低 · 中 · 高 · 超高 · 最高 · Ultra' }).click();
  await editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true }).click();
  await expect(page.getByRole('option', { name: '最高', exact: true })).toBeVisible();
  await page.getByRole('option', { name: 'Ultra 自动委派任务', exact: true }).click();
  await editor.getByRole('button', { name: '保存渠道', exact: true }).click();
  await expect(editor).toHaveCount(0);
  expect(await savedNativeReasoning(page)).toEqual({ multiAgentVersion: 'v2', ultraEffort: 'max' });
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '模型库', exact: true }).click();
  const row = page.locator('.catalog-row[data-channel="demo-work"][data-model="example-code"]');
  await expect(row.getByRole('combobox')).toHaveText('Ultra');
  await row.getByRole('combobox').click();
  await expect(page.getByRole('option')).toHaveText(['低', '中', '高', '超高', '最高', 'Ultra']);
  await page.keyboard.press('Escape');
  await row.locator('.catalog-model').click();
  await expect(editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true })).toHaveText('Ultra');
  await page.screenshot({ path: 'artifacts/screenshots/ultra-model-editor.png' });
  const audit = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(audit.violations.map((violation) => ({ id: violation.id, targets: violation.nodes.map((node) => node.target) }))).toEqual([]);
});

test('editing a default preserves native metadata while editing capabilities recalculates it', async ({ page }) => {
  await page.evaluate(async () => {
    const modulePath = performance.getEntriesByType('resource').map((entry) => entry.name).find((name) => new URL(name).pathname === '/src/lib/api.ts');
    if (!modulePath) throw new Error('Missing preview API.');
    const { api, desktop } = await import(modulePath);
    if (desktop) throw new Error('This fixture is only for browser preview.');
    const { profiles } = await api.dashboard();
    const profile = profiles.find((entry: { id: string }) => entry.id === 'demo-work');
    await api.saveProfile({ id: profile.id, name: profile.name, baseUrl: profile.baseUrl, model: profile.model, models: profile.models.map((model: { id: string }) => model.id !== 'example-code' ? model : { ...model, reasoningEfforts: ['low', 'high', 'ultra'], defaultReasoningEffort: 'high', nativeReasoning: { multiAgentVersion: 'v2', ultraEffort: 'low' } }) });
  });
  await page.getByRole('button', { name: '刷新主力渠道', exact: true }).click();
  await expect(page.getByRole('button', { name: '刷新主力渠道', exact: true })).toBeEnabled();
  await expect(page.getByRole('status')).toContainText('主力渠道已更新');
  const editor = await openModel(page);
  await editor.getByRole('combobox', { name: 'example-code 的默认推理强度', exact: true }).click();
  await page.getByRole('option', { name: 'Ultra 自动委派任务', exact: true }).click();
  await editor.getByRole('button', { name: '保存渠道', exact: true }).click();
  await expect(editor).toHaveCount(0);
  expect(await savedNativeReasoning(page)).toEqual({ multiAgentVersion: 'v2', ultraEffort: 'low' });
  await openModel(page);
  await editor.getByRole('checkbox', { name: '最高', exact: true }).focus();
  await page.keyboard.press('Space');
  await editor.getByRole('button', { name: '保存渠道', exact: true }).click();
  await expect(editor).toHaveCount(0);
  expect(await savedNativeReasoning(page)).toEqual({ multiAgentVersion: 'v2', ultraEffort: 'max' });
});

test('Ultra requires at least one conventional API effort', async ({ page }) => {
  const editor = await openModel(page);
  await editor.getByRole('combobox', { name: 'example-code 的推理档位', exact: true }).click();
  await page.getByRole('option', { name: '自定义档位', exact: true }).click();
  for (const label of ['中', '高', '超高']) {
    await editor.getByRole('checkbox', { name: label, exact: true }).focus();
    await page.keyboard.press('Space');
  }
  const low = editor.getByRole('checkbox', { name: '低', exact: true });
  const ultra = editor.getByRole('checkbox', { name: 'Ultra', exact: true });
  await ultra.focus(); await page.keyboard.press('Space');
  await expect(low).toBeDisabled();
  await expect(low).toBeChecked();
  await ultra.focus(); await page.keyboard.press('Space');
  await low.focus(); await page.keyboard.press('Space');
  await expect(ultra).toBeDisabled();
  await expect(ultra).not.toBeChecked();
});
