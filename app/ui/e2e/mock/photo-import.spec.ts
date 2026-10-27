import { boot, expect, item, listed, test } from './helpers';
import type { Page } from '@playwright/test';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

async function pickPhotos(page: Page) {
  await page.getByRole('button', { name: 'Importa', exact: true }).click();
  await page.getByRole('menu', { name: 'Importa da' }).getByRole('menuitem', { name: 'Foto e video' }).click();
}

test('every photo comes in, N di M counts up, the slip closes itself', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile&photos=8');
  await listed(page);
  await pickPhotos(page);
  const sheet = page.getByTestId('photo-import');
  await expect(sheet).toBeVisible();
  await expect(page.getByTestId('photo-import-status')).toContainText(/di 8/);
  await expect(page.getByTestId('photo-import-status')).toHaveText('Importate 8 di 8');
  await expect(item(page, 'IMG_0412.HEIC')).toBeVisible();
  await expect(item(page, 'IMG_1007.HEIC')).toBeVisible();
  await expect(sheet).toBeHidden({ timeout: 5_000 });
});

test('the ones iCloud could not deliver are listed with the reason; Riprova questi brings them in', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile&photos=6&pickfail=2');
  await listed(page);
  await pickPhotos(page);
  await expect(page.getByTestId('photo-import-status')).toHaveText('Importate 4 di 6');
  const fails = page.getByTestId('photo-import-failures').getByRole('listitem');
  await expect(fails).toHaveCount(2);
  await expect(fails.first()).toContainText('IMG_0412.HEIC');
  await expect(fails.first()).toContainText('Senza rete');
  await expect(item(page, 'IMG_0415.JPG')).toBeVisible();
  await page.getByTestId('photo-import-retry').click();
  await expect(page.getByTestId('photo-import-status')).toHaveText('Importate 2 di 2');
  await expect(item(page, 'IMG_0412.HEIC')).toBeVisible();
  await expect(item(page, 'IMG_0413.MOV')).toBeVisible();
  await expect(page.getByTestId('photo-import')).toBeHidden({ timeout: 5_000 });
});

test('Annulla stops; what was not imported is listed as cancelled and can be retried', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile&photos=40');
  await listed(page);
  await pickPhotos(page);
  await expect(page.getByTestId('photo-import-status')).toContainText(/di 40/);
  await page.getByTestId('photo-import-cancel').click();
  await expect(page.getByTestId('photo-import-status')).toContainText(/^Annullato: importate \d+ di 40$/);
  await expect(page.getByTestId('photo-import-failures').getByRole('listitem').first()).toContainText('Annullata');
  await expect(page.getByTestId('photo-import-retry')).toBeVisible();
  await page.getByTestId('photo-import-close').click();
  await expect(page.getByTestId('photo-import')).toBeHidden();
});
