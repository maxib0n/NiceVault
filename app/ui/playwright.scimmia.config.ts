import { defineConfig, devices } from '@playwright/test';

const PORT = Number(process.env.NV_E2E_PORT ?? 5411);
const desk = { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 } };

export default defineConfig({
  testDir: 'e2e/scimmia',
  fullyParallel: true,
  retries: 0,
  workers: Math.min(2, Number(process.env.SCIMMIA_WORKERS ?? 2)),
  reporter: [['list']],
  outputDir: 'e2e/.results-scimmia',
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    locale: 'it-IT',
    trace: 'off',
    screenshot: 'off',
  },
  projects: [
    { name: 'chromium-dark-it', use: { ...desk }, metadata: { query: '&app_theme=dark' } },
    { name: 'chromium-light-en', use: { ...desk, locale: 'en-US', viewport: { width: 1024, height: 700 } }, metadata: { query: '&app_theme=light' } },
    { name: 'chromium-hidpi', use: { ...desk, viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1.3 }, metadata: { query: '&scale=110' } },
    { name: 'chromium-phone', use: { ...devices['Pixel 7'], locale: 'it-IT' }, metadata: { query: '&app_theme=dark' } },
    { name: 'webkit-light-it', use: { ...devices['Desktop Safari'], viewport: { width: 1280, height: 800 } }, metadata: { query: '&app_theme=light' } },
  ],
  webServer: {
    command: process.env.SCIMMIA_DEV
      ? `pnpm exec vite --host 127.0.0.1 --port ${PORT} --strictPort`
      : `NV_KEEP_MOCK=1 pnpm exec vite build --outDir e2e/.scimmia-dist --emptyOutDir --logLevel warn && pnpm exec vite preview --outDir e2e/.scimmia-dist --host 127.0.0.1 --port ${PORT} --strictPort`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: true,
    timeout: 180_000,
  },
});
