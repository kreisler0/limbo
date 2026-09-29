import { defineConfig } from '@playwright/test';

// End-to-end tests of the UI against the mock backend (src/lib/mock.ts) in
// Chromium. The real host is Windows-only; see docs/MANUAL_TEST.md for that.
export default defineConfig({
  testDir: 'e2e',
  timeout: 20_000,
  expect: { timeout: 3_000 },
  fullyParallel: true,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: 'http://localhost:5173/',
    viewport: { width: 1280, height: 800 },
    actionTimeout: 4_000,
    launchOptions: process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {},
  },
  webServer: {
    command: 'npx vite --port 5173 --strictPort',
    url: 'http://localhost:5173/',
    reuseExistingServer: !process.env.CI,
  },
});
