import { chromium } from '@playwright/test';
const browser = await chromium.launch({ channel: 'msedge', args: ['--no-proxy-server'] });
try {
  const page = await browser.newPage({ viewport: { width: 1180, height: 900 } });
  page.on('pageerror', (error) => console.log('PAGE ERROR:', error.message));
  page.on('requestfailed', (request) => console.log('FAILED:', request.url(), request.failure()?.errorText));
  page.on('response', (response) => { if (response.status() >= 400) console.log('HTTP', response.status(), response.url()); });
  await page.goto('http://127.0.0.1:1420', { waitUntil: 'domcontentloaded', timeout: 20000 });
  console.log('DOM READY', await page.title());
  await page.locator('h1').waitFor({ timeout: 20000 });
  await page.evaluate(() => document.fonts.ready);
  console.log('CONTENT', (await page.locator('body').innerText()).slice(0, 500));
  await page.screenshot({ path: 'artifacts/screenshots/inspection.png', fullPage: true });
} finally { await browser.close(); }
