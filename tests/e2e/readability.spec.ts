import { test, expect, type Locator, type Page } from '@playwright/test';

const navigation = (page: Page, label: string) => page.getByRole('navigation', { name: '主导航', exact: true }).getByRole('button', { name: label, exact: true });

async function inspectTextAndWidth(page: Page, context: string) {
  const result = await page.locator('.main-content').evaluate((root) => {
    const smallText: { text: string; size: string }[] = [];
    for (const element of root.querySelectorAll('*')) {
      if (!(element instanceof HTMLElement) || !element.checkVisibility()) continue;
      if (![...element.childNodes].some((node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim())) continue;
      const size = getComputedStyle(element).fontSize;
      if (parseFloat(size) < 13) smallText.push({ text: element.textContent?.slice(0, 80) ?? '', size });
    }
    return { smallText, contentWidth: root.clientWidth, scrollWidth: root.scrollWidth };
  });
  expect(result.smallText, `${context}: supporting text remains at least 13px`).toEqual([]);
  expect(result.scrollWidth, `${context}: no clipped horizontal content`).toBeLessThanOrEqual(result.contentWidth);
}

async function inspectTypography(locator: Locator, context: string, minimumSize: number, minimumContrast: number) {
  const items = await locator.evaluateAll((elements) => {
    type Rgba = [number, number, number, number];
    const rgba = (value: string): Rgba => {
      const parts = value.match(/[\d.]+/g)?.map(Number) ?? [];
      return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0, parts[3] ?? 1];
    };
    const over = (front: Rgba, back: Rgba): Rgba => [
      front[0] * front[3] + back[0] * (1 - front[3]),
      front[1] * front[3] + back[1] * (1 - front[3]),
      front[2] * front[3] + back[2] * (1 - front[3]), 1,
    ];
    const luminance = (color: Rgba) => {
      const linear = color.slice(0, 3).map((component) => {
        const channel = component / 255;
        return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
      });
      return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    };
    return elements.filter((element) => element instanceof HTMLElement && element.checkVisibility()).map((element) => {
      const chain: Element[] = [];
      for (let ancestor: Element | null = element; ancestor; ancestor = ancestor.parentElement) chain.unshift(ancestor);
      const background = chain.reduce<Rgba>((color, ancestor) => over(rgba(getComputedStyle(ancestor).backgroundColor), color), [255, 255, 255, 1]);
      const style = getComputedStyle(element);
      const foreground = over(rgba(style.color), background);
      const luminances = [luminance(foreground), luminance(background)].sort((a, b) => a - b);
      return {
        text: element.textContent?.trim().slice(0, 70) || element.getAttribute('aria-label') || element.tagName,
        size: parseFloat(style.fontSize),
        contrast: (luminances[1] + 0.05) / (luminances[0] + 0.05),
      };
    });
  });
  expect(items.length, `${context}: inspect actual rendered content`).toBeGreaterThan(0);
  for (const item of items) {
    expect(item.size, `${context}: ${item.text}`).toBeGreaterThanOrEqual(minimumSize);
    expect(item.contrast, `${context}: ${item.text} contrast`).toBeGreaterThanOrEqual(minimumContrast);
  }
}

async function inspectControlHeight(locator: Locator, context: string, height: number) {
  const heights = await locator.evaluateAll((elements) => elements
    .filter((element) => element instanceof HTMLElement && element.checkVisibility())
    .map((element) => ({ name: element.getAttribute('aria-label') || element.textContent?.trim().slice(0, 40), height: element.getBoundingClientRect().height })));
  expect(heights.length, `${context}: inspect visible controls`).toBeGreaterThan(0);
  for (const control of heights) expect(control.height, `${context}: ${control.name}`).toBeCloseTo(height, 0);
}

async function inspectSegmentedControls(locator: Locator, context: string) {
  const groups = await locator.evaluateAll((elements) => elements
    .filter((element) => element instanceof HTMLElement && element.checkVisibility())
    .map((element) => {
      const buttons = [...element.querySelectorAll('button')].filter((button) => button.checkVisibility());
      return {
        selected: buttons.filter((button) => button.matches('[aria-current="page"], [aria-pressed="true"], [aria-selected="true"]')).length,
        buttons: buttons.map((button) => ({ name: button.textContent?.trim(), size: parseFloat(getComputedStyle(button).fontSize), height: button.getBoundingClientRect().height })),
      };
    }));
  expect(groups.length, `${context}: uses shared segmented controls`).toBeGreaterThan(0);
  for (const group of groups) {
    expect(group.selected, `${context}: one legible selected item per group`).toBe(1);
    for (const button of group.buttons) {
      expect(button.size, `${context}: ${button.name}`).toBeGreaterThanOrEqual(14);
      expect(button.height, `${context}: ${button.name} compact height`).toBeGreaterThanOrEqual(36);
    }
  }
}

async function inspectResourceHierarchy(page: Page, label: string, context: string) {
  await inspectTypography(page.locator('.main-content h1'), `${context} page title`, 28, 7);
  if (label === '渠道') {
    await inspectTypography(page.locator('.channel-heading h2'), `${context} channel names`, 18, 7);
    await inspectTypography(page.locator('.channel-heading p'), `${context} channel addresses`, 13, 4.5);
    await inspectControlHeight(page.locator('.channel-toolbar .search-input'), `${context} channel search`, 44);
  }
  if (label === '模型库') {
    await inspectTypography(page.locator('.catalog-model strong'), `${context} model names`, 18, 7);
    await inspectTypography(page.locator('.catalog-model > span'), `${context} original model identifiers`, 13, 4.5);
    await inspectControlHeight(page.locator('.catalog-toolbar .search-input, .catalog-toolbar .vela-select-trigger'), `${context} model toolbar`, 44);
    await inspectTypography(page.locator('.catalog-toolbar input'), `${context} model search text`, 16, 7);
  }
  if (label === '线程') {
    await inspectTypography(page.locator('.thread-row-main strong'), `${context} thread names`, 18, 7);
    await inspectTypography(page.locator('.thread-row-time'), `${context} thread dates`, 13, 4.5);
    await inspectControlHeight(page.locator('.thread-search-controls .search-input, .thread-search-controls .vela-select-trigger'), `${context} thread toolbar`, 44);
    await inspectSegmentedControls(page.locator('.thread-scopes.segmented-control'), `${context} thread filters`);
  }
  if (label === '评测') {
    await inspectTypography(page.locator('.evaluation-schedule-title h2'), `${context} plan title`, 20, 7);
    await inspectTypography(page.locator('.evaluation-schedule-title p'), `${context} plan description`, 16, 4.5);
    await inspectSegmentedControls(page.locator('.evaluation-subnav.segmented-control'), `${context} evaluation pages`);
  }
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
        await inspectResourceHierarchy(page, label, `${mode} ${size.width} ${label}`);
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

  test(`${mode} settings and model editors preserve the shared form hierarchy at desktop and narrow widths`, async ({ page }) => {
    await page.addInitScript((value) => localStorage.setItem('vela:appearance:v1', value), mode);
    await page.goto('/');
    await expect(page.locator('.channel-card')).toHaveCount(2);
    await page.evaluate(() => document.fonts.ready);
    for (const width of [1180, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.getByRole('button', { name: 'AhaX 设置', exact: true }).click();
      const settings = page.getByRole('dialog');
      await inspectSegmentedControls(settings.locator('.settings-pages.segmented-control'), `${mode} ${width} settings pages`);
      await inspectTypography(settings.locator('.editor-section-heading h3'), `${mode} ${width} settings groups`, 20, 7);
      await inspectTypography(settings.locator('.editor-field > label'), `${mode} ${width} field labels`, 14, 7);
      await inspectTypography(settings.locator('.editor-field input'), `${mode} ${width} field values`, 16, 7);
      await inspectTypography(settings.locator('.editor-hint'), `${mode} ${width} field explanations`, 13, 4.5);
      await inspectControlHeight(settings.locator('.editor-field input, .editor-field .vela-select-trigger'), `${mode} ${width} form controls`, 44);
      await inspectTypography(settings.getByRole('button', { name: '保存设置', exact: true }), `${mode} ${width} primary action`, 14, 4.5);
      await inspectControlHeight(settings.getByRole('button', { name: '保存设置', exact: true }), `${mode} ${width} form action`, 44);
      await page.getByRole('button', { name: '关闭弹窗', exact: true }).click();

      await page.locator('.channel-card').first().getByRole('button', { name: '管理模型', exact: true }).click();
      const editor = page.getByRole('dialog');
      await inspectTypography(editor.locator('.editor-model-name strong'), `${mode} ${width} selectable models`, 16, 7);
      await inspectTypography(editor.locator('.editor-field > label'), `${mode} ${width} channel field labels`, 14, 7);
      await inspectTypography(editor.locator('.editor-field input:not([type=checkbox], [readonly], :disabled)'), `${mode} ${width} channel field values`, 16, 7);
      await inspectTypography(editor.locator('.editor-field input[readonly]'), `${mode} ${width} retained API address`, 16, 4.5);
      const setting = editor.getByRole('button', { name: 'example-code 的模型设置', exact: true });
      if (await setting.getAttribute('aria-expanded') !== 'true') await setting.click();
      await inspectTypography(editor.getByLabel('example-code 的显示备注'), `${mode} ${width} model alias value`, 16, 7);
      await inspectControlHeight(editor.getByLabel('example-code 的显示备注'), `${mode} ${width} nested model field`, 44);
      await page.getByRole('button', { name: '关闭弹窗', exact: true }).click();
    }
  });
}
