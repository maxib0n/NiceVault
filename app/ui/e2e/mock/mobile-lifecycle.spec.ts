// Phone lifecycle (lib/mobile.ts, NvPlugin.swift): back from the background the app is on screen
// at once: a short trip keeps the volume open, a trip of 30 s or more shows Sblocca (also when the
// page's timers were frozen in the background), with an open preview too.
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

  // timers frozen in the background (no timer ran): the clock says 31 s, locked on return
  await item(page, 'Copertina.jpg').dblclick();
  await visibility(page, 'hidden');
  // setSystemTime moves the clock without running the page's timers
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
  // the second lock on return (nothing open any more) is harmless: no error, Sblocca stays
  await page.clock.fastForward(1_000);
  await expectScreen(page, 'unlock');
});
