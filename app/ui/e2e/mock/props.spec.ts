import type { Page } from '@playwright/test';
import { boot, expect, expectFolder, fileList, item, listed, modKey, test } from './helpers';

const card = (page: Page) => page.getByTestId('props');
const dialog = (page: Page) => page.locator('dialog[open]');
const altEnter = async (page: Page) => {
  await fileList(page).focus();
  await page.keyboard.press('Alt+Enter');
};

test('a file: type, size, pixels, dates, origin; SHA-256 on request; Esc closes', async ({ page }) => {
  await boot(page, 'screen=vault&view=grid&dir=Foto/Vacanze 2024');
  await listed(page);
  await item(page, 'Dolomiti 01.jpg').click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Proprietà/ }).click();
  await expect(card(page)).toHaveAttribute('data-kind', 'file');
  await expect(dialog(page).getByRole('heading', { name: 'Dolomiti 01.jpg' })).toBeVisible();
  await expect(card(page)).toContainText('Immagine');
  await expect(card(page)).toContainText('image/jpeg');
  await expect(page.getByTestId('props-dimensions')).toHaveText('960 × 659 px');
  await expect(page.getByTestId('props-size')).toContainText('byte');
  await expect(page.getByTestId('props-location')).toHaveText('/Foto/Vacanze 2024');
  await expect(page.getByTestId('props-origin')).toHaveText('/home/utente/Documenti/Foto/Vacanze 2024/Dolomiti 01.jpg');
  await expect(page.getByTestId('props-imported')).not.toHaveText('—');

  await page.getByTestId('props-sha').getByRole('button', { name: 'Calcola' }).click();
  await expect(page.getByTestId('props-sha').locator('code')).toHaveText(/^[0-9a-f]{64}$/);
  await page.keyboard.press('Escape');
  await expect(card(page)).toHaveCount(0);
  await expect(fileList(page)).toBeFocused();

  await altEnter(page);
  await expect(page.getByTestId('props-sha').locator('code')).toHaveText(/^[0-9a-f]{64}$/);
});

test('a folder with Alt+Enter: direct count, recursive totals computed', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await item(page, 'Foto').click();
  await altEnter(page);
  await expect(card(page)).toHaveAttribute('data-kind', 'dir');
  await expect(card(page)).toContainText('Cartella');
  await expect(page.getByTestId('props-tree')).toHaveAttribute('data-done', 'true');
  await expect(page.getByTestId('props-tree')).toHaveText(/\d+ file · \d+ cartell[ae]/);
  await expect(page.getByTestId('props-total')).toContainText('byte');
  await page.getByRole('button', { name: 'Chiudi' }).click();
  await expect(card(page)).toHaveCount(0);
});

test('a selection: counts and totals of everything selected', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await fileList(page).focus();
  await page.keyboard.press(`${await modKey(page)}+a`);
  const n = await page.getByRole('option', { selected: true }).count();
  await page.keyboard.press('Alt+Enter');
  await expect(card(page)).toHaveAttribute('data-kind', 'selection');
  await expect(dialog(page).getByRole('heading', { name: `${n} elementi` })).toBeVisible();
  await expect(page.getByTestId('props-tree')).toHaveAttribute('data-done', 'true');
});

test('the volume: background menu at the root', async ({ page }) => {
  await boot(page, 'screen=vault&view=grid');
  await listed(page);
  const box = await fileList(page).boundingBox();
  await page.mouse.click(box!.x + box!.width - 20, box!.y + box!.height - 20, { button: 'right' });
  await page.getByRole('menuitem', { name: /Proprietà/ }).click();
  await expect(card(page)).toHaveAttribute('data-kind', 'volume');
  await expect(card(page)).toContainText('Volume NiceVault');
  await expect(card(page)).toContainText('/run/media/utente/CHIAVETTA/Documenti.nvault');
  await expect(page.getByTestId('props-tree')).toHaveAttribute('data-done', 'true');
});

test('made in the volume: no import, no origin', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await fileList(page).focus();
  await page.keyboard.press(`${await modKey(page)}+Shift+N`);
  await page.keyboard.press('Enter');
  await expectFolder(page, 'Documenti');
  await expect(fileList(page).getByRole('option', { selected: true })).toHaveCount(1);
  await fileList(page).focus();
  await page.keyboard.press('Alt+Enter');
  await expect(page.getByTestId('props-imported')).toHaveText('Creato nel volume');
  await expect(page.getByTestId('props-origin')).toHaveText('—');
});

test('English', async ({ page }) => {
  await boot(page, 'screen=vault&view=list&lang=en');
  const list = page.getByRole('listbox');
  await expect(list).toHaveAttribute('aria-busy', 'false');
  await list.getByRole('option').filter({ hasText: 'Leggimi.txt' }).click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Properties/ }).click();
  await expect(card(page)).toContainText('Imported from');
  await expect(page.getByTestId('props-sha').getByRole('button', { name: 'Compute' })).toBeVisible();
  await expect(page.getByTestId('props-size')).toContainText('bytes');
});
