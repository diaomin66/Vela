import { expect, test } from '@playwright/test';

test('native drawers and dialogs return keyboard focus to their opener', async ({ page }) => {
  await page.goto('/');
  const settings = page.getByRole('button', { name: 'ahaX 设置', exact: true });
  const drawer = page.getByRole('dialog', { name: 'ahaX 设置', exact: true });
  await settings.click();
  const interval = drawer.getByRole('combobox', { name: '同步间隔', exact: true });
  await interval.focus();
  await page.keyboard.press('ArrowDown');
  await expect(interval).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByRole('listbox', { name: '同步间隔', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(drawer).toBeVisible();
  await expect(interval).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(drawer).toHaveCount(0);
  await expect(settings).toBeFocused();
  await settings.click();
  await drawer.getByRole('button', { name: '关闭弹窗', exact: true }).click();
  await expect(settings).toBeFocused();

  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '模型库', exact: true }).click();
  const verify = page.getByRole('button', { name: '验证 备用渠道（example-reasoning）', exact: true });
  const dialog = page.getByRole('dialog', { name: '验证模型', exact: true });
  await verify.click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(verify).toBeFocused();
  await verify.click();
  await dialog.getByRole('button', { name: '关闭弹窗', exact: true }).click();
  await expect(verify).toBeFocused();
});
