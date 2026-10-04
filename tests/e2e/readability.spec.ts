import { test, expect, type Page } from '@playwright/test';

const navigation = (page: Page, label: string) => page.getByRole('navigation', { name: '主导航', exact: true }).getByRole('button', { name: label, exact: true });

async function inspectTextAndWidth(page: Page, context: string) {
  const result = await page.locator('.main-content').evaluate((root) => {
    const smallText: { text: string; size: string }[] = [];
    for (const element of root.querySelectorAll('*')) {
      if (!(element instanceof HTMLElement) || !element.checkVisibility()) continue;
      if (![...element.childNodes].some((node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim())) continue;
      const size = getComputedStyle(element).fontSize;
      if (parseFloat(size) < 12) smallText.push({ text: element.textContent?.slice(0, 80) ?? '', size });
    }
    return { smallText, contentWidth: root.clientWidth, scrollWidth: root.scrollWidth };
  });
  expect(result.smallText, `${context}: no unreadable captions`).toEqual([]);
  expect(result.scrollWidth, `${context}: no clipped horizontal content`).toBeLessThanOrEqual(result.contentWidth);
}

for (const mode of ['light', 'dark'] as const) {
  test(`${mode} workspaces retain readable type, stable navigation and reachable actions at desktop and narrow sizes`, async ({ page }) => {
    test.setTimeout(90_000);
    await page.addInitScript((value) => localStorage.setItem('vela:appearance:v1', value), mode);
    await page.goto('/?evaluationDemo=gallery');
    await expect(page.getByRole('heading', { name: '渠道管理', exact: true })).toBeVisible();
    await page.evaluate(() => document.fonts.ready);

    for (const size of [{ width: 1180, height: 820 }, { width: 900, height: 680 }, { width: 390, height: 820 }]) {
      await page.setViewportSize(size);
      for (const label of ['渠道', '模型库', '线程', '评测', '诊断', '恢复']) {
        await navigation(page, label).click();
        if (label === '线程') await expect(page.locator('.thread-row')).toHaveCount(20);
        if (label === '评测') {
          await page.getByRole('navigation', { name: '评测子导航' }).getByRole('button', { name: '定时评测', exact: true }).click();
          await expect(page.getByRole('region', { name: '定时计划概况' })).toBeVisible();
        }
        await inspectTextAndWidth(page, `${mode} ${size.width} ${label}`);
        await expect(navigation(page, label)).toBeInViewport();
        await expect(page.locator('.app-footer')).toBeInViewport();
      }

      await navigation(page, '线程').click();
      const main = page.locator('.main-content');
      const before = await page.locator('.app-sidebar').boundingBox();
      await main.evaluate((element) => { element.scrollTop = element.scrollHeight; });
      await expect.poll(() => main.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
      const after = await page.locator('.app-sidebar').boundingBox();
      expect(after).toEqual(before);
      await expect(navigation(page, '线程')).toBeInViewport();
      await expect(page.getByRole('button', { name: '下一页线程', exact: true })).toBeInViewport();
      await main.focus();
      await page.keyboard.press('Home');
      await expect(main).toBeFocused();
    }
  });
}
