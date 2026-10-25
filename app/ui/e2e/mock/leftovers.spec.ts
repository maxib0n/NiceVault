// Small UI leftovers: layout at large UI sizes, a pulled drive on Sblocca, empty volume vs
// empty folder, filters in a bare folder, the sort key's name, the mount path.
import type { Page } from '@playwright/test';
import { boot, expect, expectScreen, listed, test } from './helpers';

const drive = (page: Page, action: 'add' | 'remove', d?: unknown) =>
  page.evaluate(
    ([a, x]) => (window as unknown as { __nv: { mock: { simulateDrive(a: string, d?: unknown): unknown } } }).__nv.mock.simulateDrive(a, x),
    [action, d] as const,
  );

// the window's size at 100 / 125 / 150% UI size, as CSS px
const SIZES = [900, 1280, 1680].flatMap((w) => [1, 1.25, 1.5].map((s) => ({ w: Math.round(w / s), h: Math.round((w * 0.625) / s), label: `${w}@${s * 100}%` })));

for (const { w, h, label } of SIZES) {
  test(`nothing overflows the volume's head at ${label}`, async ({ page }) => {
    await page.setViewportSize({ width: w, height: h });
    await boot(page, 'screen=vault');
    await listed(page);
    const over = await page.evaluate(() => {
      const W = document.documentElement.clientWidth;
      return [...document.querySelectorAll('.folder-head *')]
        .filter((el) => {
          const b = el.getBoundingClientRect();
          return b.width > 0 && getComputedStyle(el).visibility !== 'hidden' && (b.right > W + 1 || b.left < -1);
        })
        .map((el) => el.tagName + '.' + [...el.classList].join('.'));
    });
    expect(over).toEqual([]);
  });
}

test('Sblocca with the drive pulled: the field and Sblocca are off, with the reason', async ({ page }) => {
  await boot(page, 'screen=welcome');
  await drive(page, 'add');
  await page.getByTestId('passport').and(page.locator('[data-name="Fotografie"]')).getByRole('button', { name: /Fotografie/ }).first().click();
  await expectScreen(page, 'unlock');
  await page.getByLabel('Password', { exact: true }).fill('demo');
  await drive(page, 'remove', '/dev/sdc1');
  const field = page.getByLabel('Password', { exact: true });
  await expect(field).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Sblocca' })).toBeDisabled();
  await expect(page.getByRole('status')).toContainText('ricollegala');
  // plugged back in: usable again
  await drive(page, 'add');
  await expect(field).toBeEnabled();
  await field.fill('demo');
  await expect(page.getByRole('button', { name: 'Sblocca' })).toBeEnabled();
});

test('an empty volume has its own wording; an empty folder keeps "Cartella vuota"; no filters in either', async ({ page }) => {
  await boot(page, 'screen=vault&empty=1');
  await expect(page.getByTestId('empty-state').or(page.locator('[data-state="empty-folder"]'))).toContainText('Volume vuoto');
  await expect(page.getByRole('button', { name: 'Tipo', exact: true })).toHaveCount(0);
  await boot(page, 'screen=vault&dir=Lavoro/Archivio 2023');
  await expect(page.locator('[data-state="empty-folder"]')).toContainText('Cartella vuota.');
  await expect(page.getByRole('button', { name: 'Tipo', exact: true })).toHaveCount(0);
  // a folder with items keeps them
  await boot(page, 'screen=vault');
  await expect(page.getByRole('button', { name: 'Tipo', exact: true })).toBeVisible();
});

test('the sort key says what it is', async ({ page }) => {
  await boot(page, 'screen=vault');
  await expect(page.getByRole('button', { name: 'Ordina per: Nome' })).toBeVisible();
  await boot(page, 'screen=vault&lang=en');
  await expect(page.getByRole('button', { name: 'Sort by: Name' })).toBeVisible();
});

test('the mounted path breaks at its separators and carries the full path in its title', async ({ page }) => {
  await boot(page, 'screen=vault&mount=mounted');
  const path = page.locator('.mount-desc .path');
  await expect(path).toHaveAttribute('title', /^\/run\/user\/1000\/nicevault\/Documenti$/);
  await expect(path.locator('wbr')).not.toHaveCount(0);
});
