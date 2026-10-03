import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: true,
  retries: 0,
  reporter: 'list',
  use: { baseURL: 'http://127.0.0.1:1420', channel: 'msedge', launchOptions: { args: ['--no-proxy-server'] }, reducedMotion: 'reduce', viewport: { width: 1180, height: 900 }, trace: 'retain-on-failure' },
  webServer: { command: 'npm run dev', url: 'http://127.0.0.1:1420', reuseExistingServer: !process.env.CI, timeout: 60_000 },
});
