// Browser tests for the canforge demo, run against site/ as
// scripts/assemble-site.sh builds it. Playwright starts its own server on a
// port of its own, so a running dev server never stands in for a fresh build.
import { defineConfig, devices } from '@playwright/test';

const port = 4173;

export default defineConfig({
  testDir: './tests',
  globalSetup: './global-setup.js',
  // One test at a time, so the timing tests never compete for the CPU.
  workers: 1,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : [['list']],
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: `python3 -m http.server --bind 127.0.0.1 --directory ../site ${port}`,
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: false,
    // http.server logs every request to stderr; keep the test output readable.
    stdout: 'ignore',
    stderr: 'ignore',
  },
});
