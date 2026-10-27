import { boot, expect, expectScreen, item, listed, test } from './helpers';
import type { Page } from '@playwright/test';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

async function visibility(page: Page, state: 'hidden' | 'visible') {
  await page.evaluate((s) => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => s });
    document.dispatchEvent(new Event('visibilitychange'));
  }, state);
}

test('a short trip to the background keeps the volume; a long one shows Sblocca at once', async ({ page }) => {
  await page.clock.install();
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await visibility(page, 'hidden');
  await page.clock.fastForward(5_000);
  await visibility(page, 'visible');
  await listed(page);
  await expectScreen(page, 'vault');

  await item(page, 'Copertina.jpg').dblclick();
  await visibility(page, 'hidden');
  await page.clock.setSystemTime((await page.evaluate(() => Date.now())) + 31_000);
  const back = Date.now();
  await visibility(page, 'visible');
  await expectScreen(page, 'unlock');
  expect(Date.now() - back).toBeLessThan(3_000);
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('the lock timer that runs in the background leaves Sblocca ready for the return', async ({ page }) => {
  await page.clock.install();
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await visibility(page, 'hidden');
  await page.clock.fastForward(31_000);
  await expectScreen(page, 'unlock');
  await visibility(page, 'visible');
  await expectScreen(page, 'unlock');
  await page.clock.fastForward(1_000);
  await expectScreen(page, 'unlock');
});
