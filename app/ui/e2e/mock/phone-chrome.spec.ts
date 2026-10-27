import { type Page } from '@playwright/test';
import { boot, expect, test } from './helpers';

const NOTCH = 59;
const HOME = 34;

test.use({ viewport: { width: 393, height: 852 }, hasTouch: true });

async function notch(page: Page) {
  await page.evaluate(
    ([t, b]) => {
      const s = document.documentElement.style;
      s.setProperty('--nv-inset-top', `${t}px`);
      s.setProperty('--nv-inset-bottom', `${b}px`);
    },
    [NOTCH, HOME],
  );
}

test("Benvenuto: Impostazioni is a finger's key below the notch, not the notch's height", async ({ page }) => {
  await boot(page, 'screen=welcome&platform=mobile');
  await notch(page);
  const key = page.getByTestId('open-settings');
  await expect(async () => {
    const b = (await key.boundingBox())!;
    expect(b.y).toBeGreaterThanOrEqual(NOTCH);
    expect(b.height).toBeGreaterThanOrEqual(44);
    expect(b.height).toBeLessThanOrEqual(48);
  }).toPass();
});


test('a dialog with nothing to type is a slip from the bottom, as tall as its content', async ({ page }) => {
  await boot(page, 'screen=details&dialog=resize&platform=mobile');
  await notch(page);
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(async () => {
    const d = (await dialog.boundingBox())!;
    expect(Math.round(d.y + d.height)).toBe(852);
    expect(d.y).toBeGreaterThan(852 / 3);
    const close = (await dialog.getByRole('button', { name: 'Chiudi' }).last().boundingBox())!;
    expect(close.y + close.height).toBeLessThanOrEqual(852 - HOME);
  }).toPass();
});

test('a form stays a page of its own', async ({ page }) => {
  await boot(page, 'screen=details&dialog=add-password&platform=mobile');
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(async () => {
    const d = (await dialog.boundingBox())!;
    expect(Math.round(d.y)).toBe(0);
    expect(Math.round(d.height)).toBe(852);
  }).toPass();
});
