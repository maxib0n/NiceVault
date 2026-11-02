import { boot, expect, test } from './helpers';

test('a normal start: gone once the launcher is ready, nothing left to intercept', async ({ page }) => {
  await boot(page);
  await expect(page.getByTestId('splash')).toHaveCount(0);
  await page.getByTestId('open-settings').click();
  await expect(page.locator('[data-ready="true"]')).toHaveAttribute('data-screen', 'settings');
});

test('shown from the first paint, it yields to the ready launcher', async ({ page }) => {
  await page.goto('/?splash=3000');
  const splash = page.getByTestId('splash');
  await expect(splash).toBeVisible();
  await expect(page.locator('[data-ready="true"]')).toHaveCount(1);
  await expect(splash).toHaveCount(0);
  await page.getByTestId('open-settings').click();
  await expect(page.locator('[data-ready="true"]')).toHaveAttribute('data-screen', 'settings');
});

test('skipped by a key', async ({ page }) => {
  await page.goto('/?splash=hold');
  await expect(page.locator('[data-ready="true"]')).toHaveCount(1);
  await expect(page.getByTestId('splash')).toHaveAttribute('aria-hidden', 'true');
  await expect(page.getByTestId('splash')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('splash')).toHaveCount(0);
});

test('skipped by a click, which reaches nothing beneath', async ({ page }) => {
  await page.goto('/?splash=hold');
  await expect(page.locator('[data-ready="true"]')).toHaveCount(1);
  await page.getByTestId('splash').click();
  await expect(page.getByTestId('splash')).toHaveCount(0);
  await expect(page.locator('[data-ready="true"]')).toHaveAttribute('data-screen', 'welcome');
});

test('never in the compact unlock window', async ({ page }) => {
  await page.goto('/?splash=hold&compact=unlock&path=' + encodeURIComponent('/run/media/utente/CHIAVETTA/Documenti.nvault'));
  await expect(page.getByTestId('unlock-compact')).toBeVisible();
  await expect(page.getByTestId('splash')).toHaveCount(0);
});

test('reduced motion: the still frame, nothing turns', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?splash=hold');
  const splash = page.getByTestId('splash');
  await expect(splash).toBeVisible();
  const running = await splash.evaluate((el) =>
    el
      .getAnimations({ subtree: true })
      .filter((a) => Number(a.effect?.getComputedTiming().duration) > 16)
      .map((a) => (a as CSSAnimation).animationName ?? (a as CSSTransition).transitionProperty),
  );
  expect(running).toEqual([]);
});
