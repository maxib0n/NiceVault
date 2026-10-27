import type { Page } from '@playwright/test';
import { boot, breadcrumb, expect, expectFolder, fileList, item, listed, modKey, names, selectedNames, test } from './helpers';

const ROOT_ORDER = [
  'Audio',
  'Chiavi',
  'Foto',
  'Lavoro',
  'Personale',
  'Video',
  'Archivio 2019.zip',
  'Backup telefono.tar.gz',
  'Copertina.jpg',
  'Foto profilo.jpg',
  'Leggimi.txt',
];

async function gridColumns(page: Page): Promise<number> {
  return fileList(page)
    .getByRole('option')
    .evaluateAll((els) => {
      const top = (els[0] as HTMLElement).getBoundingClientRect().top;
      return els.filter((e) => Math.abs(e.getBoundingClientRect().top - top) < 2).length;
    });
}

const focusedName = (page: Page) =>
  fileList(page).evaluate((lb) => {
    const id = lb.getAttribute('aria-activedescendant');
    return id ? (document.getElementById(id)?.querySelector('[data-testid="item-name"]')?.textContent?.trim() ?? null) : null;
  });

test.describe('keyboard — list view', () => {
  test.beforeEach(async ({ page }) => {
    await boot(page, 'screen=vault&view=list');
    await listed(page);
    await fileList(page).focus();
  });

  test('arrows, Home/End, Shift range, Ctrl moves cursor only + Ctrl+Space toggles', async ({ page }) => {
    expect(await names(page)).toEqual(ROOT_ORDER);
    const MOD = await modKey(page);
    await page.keyboard.press('ArrowDown');
    expect(await selectedNames(page)).toEqual(['Audio']);
    await expect(fileList(page)).toHaveAttribute('aria-activedescendant', /^item-\d+$/);
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Shift+ArrowDown');
    await page.keyboard.press('Shift+ArrowDown');
    expect(await selectedNames(page)).toEqual(['Chiavi', 'Foto', 'Lavoro']);
    await page.keyboard.press('Shift+ArrowUp');
    expect(await selectedNames(page)).toEqual(['Chiavi', 'Foto']);
    await page.keyboard.press('ArrowRight');
    expect(await selectedNames(page)).toEqual(['Chiavi', 'Foto']);

    await page.keyboard.press('End');
    expect(await selectedNames(page)).toEqual(['Leggimi.txt']);
    await page.keyboard.press('Home');
    expect(await selectedNames(page)).toEqual(['Audio']);
    await page.keyboard.press('Shift+End');
    expect(await selectedNames(page)).toEqual(ROOT_ORDER);

    await page.keyboard.press('Home');
    await page.keyboard.press(`${MOD}+ArrowDown`);
    await page.keyboard.press(`${MOD}+ArrowDown`);
    expect(await selectedNames(page)).toEqual(['Audio']);
    expect(await focusedName(page)).toBe('Foto');
    await page.keyboard.press(`${MOD}+Space`);
    expect(await selectedNames(page)).toEqual(['Audio', 'Foto']);
    await page.keyboard.press(`${MOD}+Space`);
    expect(await selectedNames(page)).toEqual(['Audio']);
  });

  test('Ctrl/Cmd+A selects all, Esc clears, type-ahead jumps', async ({ page }) => {
    const MOD = await modKey(page);
    await page.keyboard.press(`${MOD}+a`);
    expect(await selectedNames(page)).toEqual(ROOT_ORDER);
    await expect(page.getByText(/11 selezionati/)).toBeVisible();
    await page.keyboard.press('Escape');
    expect(await selectedNames(page)).toEqual([]);
    await page.keyboard.type('legg');
    expect(await selectedNames(page)).toEqual(['Leggimi.txt']);
    await page.waitForTimeout(800);
    await page.keyboard.type('v');
    expect(await selectedNames(page)).toEqual(['Video']);
  });

  test('Enter opens a folder, Backspace goes up and re-selects it, Alt+Up too', async ({ page }) => {
    await page.keyboard.type('foto');
    await page.keyboard.press('Enter');
    await expectFolder(page, 'Foto');
    await expect(breadcrumb(page).getByRole('button', { name: 'Documenti' })).toBeVisible();
    await expect(fileList(page)).toBeFocused();
    await page.keyboard.press('Home');
    await page.keyboard.press('Enter');
    await expectFolder(page, 'Compleanno Giulia');
    await page.keyboard.press('Alt+ArrowUp');
    await expectFolder(page, 'Foto');
    expect(await selectedNames(page)).toEqual(['Compleanno Giulia']);
    await page.keyboard.press('Backspace');
    await expectFolder(page, 'Documenti');
    expect(await selectedNames(page)).toEqual(['Foto']);
    await page.keyboard.press('Backspace');
    await expectFolder(page, 'Documenti');
    await expect(page.getByRole('button', { name: 'Cartella superiore' })).toBeDisabled();
  });

  test('Space toggles quick look; ←/→ step; Esc closes and focus returns to the list', async ({ page }) => {
    await page.keyboard.type('cop');
    await page.keyboard.press(' ');
    const ql = page.getByRole('dialog', { name: 'Anteprima di Copertina.jpg' });
    await expect(ql).toBeVisible();
    await expect(ql.getByRole('img', { name: 'Copertina.jpg' })).toBeVisible();
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('dialog', { name: 'Anteprima di Foto profilo.jpg' })).toBeVisible();
    expect(await selectedNames(page)).toEqual(['Foto profilo.jpg']);
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowLeft');
    await expect(page.getByRole('dialog', { name: 'Anteprima di Backup telefono.tar.gz' })).toBeVisible();
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowLeft');
    await expect(page.getByRole('dialog', { name: 'Anteprima di Leggimi.txt' })).toBeVisible();
    await page.keyboard.press(' ');
    await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
    await expect(fileList(page)).toBeFocused();
    await page.keyboard.press(' ');
    await expect(page.getByRole('dialog', { name: 'Anteprima di Leggimi.txt' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
    await expect(fileList(page)).toBeFocused();
    await page.keyboard.press('Home');
    await page.keyboard.press(' ');
    await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
    await page.keyboard.press('End');
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('dialog', { name: 'Anteprima di Foto profilo.jpg' })).toBeVisible();
  });

  test('F2 rename: stem pre-selected, Enter commits, Esc cancels', async ({ page }) => {
    await page.keyboard.type('legg');
    await page.keyboard.press('F2');
    const input = page.getByRole('textbox', { name: 'Nuovo nome' });
    await expect(input).toBeFocused();
    await expect(input).toHaveValue('Leggimi.txt');
    const sel = await input.evaluate((el: HTMLInputElement) => [el.selectionStart, el.selectionEnd]);
    expect(sel).toEqual([0, 'Leggimi'.length]);
    await page.keyboard.type('Istruzioni');
    await page.keyboard.press('Enter');
    await expect(item(page, 'Istruzioni.txt')).toBeVisible();
    await expect(input).toHaveCount(0);

    await expect(fileList(page)).toBeFocused();
    await page.keyboard.press('F2');
    await expect(input).toBeFocused();
    await expect(input).toHaveValue('Istruzioni.txt');
    await page.keyboard.type('Qualcosa');
    await page.keyboard.press('Escape');
    await expect(input).toHaveCount(0);
    await expect(fileList(page)).toBeFocused();
    await expect(item(page, 'Istruzioni.txt')).toBeVisible();
    expect(await names(page)).not.toContain('Qualcosa.txt');
  });

  test('Delete asks for confirmation: Annulla keeps, Elimina removes (multi-select)', async ({ page }) => {
    await page.keyboard.type('arch');
    await page.keyboard.press('Shift+ArrowDown');
    await page.keyboard.press('Delete');
    const dlg = page.getByRole('dialog', { name: 'Eliminare 2 elementi?' });
    await expect(dlg).toBeVisible();
    await expect(dlg.getByRole('listitem')).toHaveText(['Archivio 2019.zip', 'Backup telefono.tar.gz']);
    await expect(dlg.getByRole('button', { name: 'Elimina' })).toBeFocused();
    await dlg.getByRole('button', { name: 'Annulla' }).click();
    await expect(dlg).toHaveCount(0);
    expect(await names(page)).toContain('Archivio 2019.zip');

    await expect(fileList(page)).toBeFocused();
    await page.keyboard.press('Delete');
    await expect(dlg).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(dlg).toHaveCount(0);
    expect(await names(page)).toContain('Archivio 2019.zip');

    await expect(fileList(page)).toBeFocused();
    await page.keyboard.press('Delete');
    await dlg.getByRole('button', { name: 'Elimina' }).click();
    await expect(item(page, 'Archivio 2019.zip')).toHaveCount(0);
    await expect(item(page, 'Backup telefono.tar.gz')).toHaveCount(0);
    await expect(page.getByRole('status').filter({ hasText: 'Eliminati 2 elementi.' })).toBeVisible();
    await expect(fileList(page)).toBeFocused();
  });

  test('Ctrl/Cmd+Shift+N creates a folder in rename mode; Shift+F10 opens the context menu', async ({ page }) => {
    const MOD = await modKey(page);
    await page.keyboard.press(`${MOD}+Shift+KeyN`);
    const input = page.getByRole('textbox', { name: 'Nuovo nome' });
    await expect(input).toHaveValue('Nuova cartella');
    await page.keyboard.type('Bollette');
    await page.keyboard.press('Enter');
    await expect(item(page, 'Bollette')).toBeVisible();
    await expect(item(page, 'Bollette')).toHaveAttribute('data-kind', 'dir');

    await page.keyboard.type('vid');
    await page.keyboard.press('Shift+F10');
    const menu = page.getByRole('menu', { name: 'Azioni elemento' });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole('menuitem').first()).toBeFocused();
    await expect(menu.getByRole('menuitem')).toHaveText([/Apri/, /Rinomina/, /Tag…/, /Esporta…/, /Proprietà/, /Elimina/]);
    await page.keyboard.press('ArrowDown');
    await expect(menu.getByRole('menuitem', { name: /Rinomina/ })).toBeFocused();
    await page.keyboard.press('End');
    await expect(menu.getByRole('menuitem', { name: /Elimina/ })).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(menu.getByRole('menuitem', { name: /Apri/ })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(fileList(page)).toBeFocused();
    await page.keyboard.press('Shift+F10');
    await page.keyboard.press('Enter');
    await expectFolder(page, 'Video');
  });
});

test.describe('keyboard — grid view', () => {
  test('←/→ move by one, ↑/↓ by a row, Shift extends, Enter opens', async ({ page }) => {
    await boot(page, 'screen=vault&view=grid');
    await listed(page);
    await fileList(page).focus();
    const cols = await gridColumns(page);
    expect(cols).toBeGreaterThan(1);
    await page.keyboard.press('ArrowRight');
    expect(await selectedNames(page)).toEqual(['Audio']);
    await page.keyboard.press('ArrowRight');
    expect(await selectedNames(page)).toEqual(['Chiavi']);
    await page.keyboard.press('ArrowLeft');
    expect(await selectedNames(page)).toEqual(['Audio']);
    await page.keyboard.press('ArrowDown');
    expect(await selectedNames(page)).toEqual([ROOT_ORDER[cols]]);
    await page.keyboard.press('ArrowUp');
    expect(await selectedNames(page)).toEqual(['Audio']);
    await page.keyboard.press('Shift+ArrowDown');
    expect(await selectedNames(page)).toEqual(ROOT_ORDER.slice(0, cols + 1));
    await page.keyboard.press('Home');
    await page.keyboard.press('ArrowUp');
    expect(await selectedNames(page)).toEqual(['Audio']);
    await page.keyboard.press('End');
    await page.keyboard.press('ArrowDown');
    expect(await selectedNames(page)).toEqual(['Leggimi.txt']);
    await page.keyboard.press('Home');
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('Enter');
    await expectFolder(page, 'Chiavi');
  });

  test('view toggle keeps the selection and switches layouts', async ({ page }) => {
    await boot(page, 'screen=vault&view=grid');
    await listed(page);
    await item(page, 'Lavoro').click();
    await page.getByRole('button', { name: 'Elenco' }).click();
    await expect(page.getByRole('button', { name: 'Elenco' })).toHaveAttribute('aria-pressed', 'true');
    await expect(fileList(page)).toHaveAttribute('data-layout', 'list');
    expect(await selectedNames(page)).toEqual(['Lavoro']);
    await page.getByRole('button', { name: 'Ordina per dimensione' }).click();
    const sorted = await names(page);
    expect(sorted.slice(0, 6)).toEqual(['Audio', 'Chiavi', 'Foto', 'Lavoro', 'Personale', 'Video']);
    expect(sorted.slice(6)).toEqual(['Leggimi.txt', 'Foto profilo.jpg', 'Copertina.jpg', 'Archivio 2019.zip', 'Backup telefono.tar.gz']);
    await page.getByRole('button', { name: 'Ordina per dimensione' }).click();
    expect((await names(page)).slice(6)).toEqual(['Backup telefono.tar.gz', 'Archivio 2019.zip', 'Copertina.jpg', 'Foto profilo.jpg', 'Leggimi.txt']);
  });
});
