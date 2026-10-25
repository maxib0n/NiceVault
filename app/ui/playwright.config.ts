// Mock-UI interaction suite (`pnpm e2e`): the Svelte UI against the in-browser
// mock backend, in Chromium and WebKit. Tests target ARIA roles/names, text and
// data-* / data-testid hooks only, never CSS classes or pixels, so they survive
// a visual redesign. The real-app suite (Tauri + nv-core) is e2e/real/.
import { defineConfig, devices } from '@playwright/test';

const PORT = Number(process.env.NV_E2E_PORT ?? 5188);

export default defineConfig({
  testDir: 'e2e/mock',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: process.env.CI ? 2 : 4,
  timeout: 30_000,
  expect: { timeout: 10_000 },
  reporter: [['list']],
  outputDir: 'e2e/.results',
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    // the suite reads Italian (the source language); English runs pass ?lang=en
    locale: 'it-IT',
    viewport: { width: 1280, height: 800 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 }, locale: 'it-IT' } },
    { name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 1280, height: 800 }, locale: 'it-IT' } },
  ],
  webServer: {
    command: `pnpm exec vite --host 127.0.0.1 --port ${PORT} --strictPort`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
