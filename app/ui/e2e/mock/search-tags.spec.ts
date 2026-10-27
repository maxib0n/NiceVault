import type { Page } from '@playwright/test';
import { boot, expect, expectFolder, fileList, item, listed, modKey, names, test } from './helpers';

const field = (page: Page) => page.getByTestId('search-field');
const results = (page: Page) => page.getByTestId('search-results');
const hits = (page: Page) => results(page).getByTestId('search-hit');
const hit = (page: Page, name: string) => hits(page).filter({ has: page.getByTestId('item-name').and(page.getByText(name, { exact: true })) });
const hitNames = async (page: Page) => (await hits(page).getByTestId('item-name').allTextContents()).map((s) => s.trim());

async function search(page: Page, q: string) {
  await field(page).fill(q);
  await expect(page.getByTestId('search-title')).toContainText(`«${q}»`);
  await expect(results(page)).toHaveAttribute('aria-busy', 'false');
}

test.describe('search', () => {
  test.beforeEach(async ({ page }) => {
    await boot(page, 'screen=vault&view=grid');
    await listed(page);
  });

  test('Ctrl/Cmd+F opens the field and focuses it; the key opens it too', async ({ page }) => {
    await expect(page.getByTestId('search-close')).toHaveCount(0);
    await fileList(page).focus();
    await page.keyboard.press(`${await modKey(page)}+f`);
    await expect(field(page)).toBeFocused();
    await expect(page.getByTestId('search-scope').getByRole('radio', { name: 'In tutto il volume' })).toHaveAttribute('aria-checked', 'true');
    await page.getByTestId('search-close').click();
    await expect(field(page)).not.toBeFocused();
    await expect(page.getByTestId('search-scope')).toHaveCount(0);
    await field(page).click();
    await expect(field(page)).toBeFocused();
  });

  test('typing finds a file in another folder, its matched part marked; its place opens that folder', async ({ page }) => {
    await page.keyboard.press(`${await modKey(page)}+f`);
    await search(page, 'braies');
    expect(await hitNames(page)).toEqual(['Lago di Braies.jpg']);
    await expect(hit(page, 'Lago di Braies.jpg').locator('mark')).toHaveText('Braies');
    await expect(page.getByTestId('search-title')).toHaveText('1 risultato per «braies»');
    const place = hit(page, 'Lago di Braies.jpg').getByTestId('hit-path');
    await expect(place).toContainText('Vacanze 2024');
    await place.click();
    await expectFolder(page, 'Vacanze 2024');
    await expect(page.getByTestId('search-close')).toHaveCount(0);
    await expect(item(page, 'Lago di Braies.jpg')).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Alt+ArrowLeft');
    await expectFolder(page, 'Documenti');
  });

  test('accents and case are ignored; Enter on a folder hit opens it', async ({ page }) => {
    await page.keyboard.press(`${await modKey(page)}+f`);
    await search(page, 'RAGU');
    expect(await hitNames(page)).toEqual(['Ragù della nonna.txt']);
    await search(page, 'ricette');
    expect(await hitNames(page)).toEqual(['Ricette']);
    await page.keyboard.press('ArrowDown');
    await expect(results(page)).toBeFocused();
    await page.keyboard.press('Enter');
    await expectFolder(page, 'Ricette');
    expect(await names(page)).toContain('Tiramisù.txt');
  });

  test('Esc clears the query (the folder is back), Esc again closes the field', async ({ page }) => {
    await page.keyboard.press(`${await modKey(page)}+f`);
    await search(page, 'dolomiti');
    await expect(hits(page)).toHaveCount(3);
    await page.keyboard.press('Escape');
    await expect(results(page)).toHaveCount(0);
    await expect(field(page)).toHaveValue('');
    await expect(field(page)).toBeFocused();
    expect(await names(page)).toContain('Leggimi.txt');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('search-close')).toHaveCount(0);
    await expect(fileList(page)).toBeFocused();
  });

  test('no results: a designed empty state; this folder only narrows the scope', async ({ page }) => {
    await page.keyboard.press(`${await modKey(page)}+f`);
    await search(page, 'zebra quantica');
    await expect(page.getByTestId('search-empty')).toContainText('Nessun risultato.');
    await search(page, 'jpg');
    const all = await hits(page).count();
    expect(all).toBeGreaterThan(10);
    await field(page).fill('');
    await page.keyboard.press('Escape');
    await item(page, 'Foto').dblclick();
    await expectFolder(page, 'Foto');
    await page.keyboard.press(`${await modKey(page)}+f`);
    await page.getByTestId('search-scope').getByRole('radio', { name: 'In questa cartella' }).click();
    await search(page, 'jpg');
    await expect.poll(() => hits(page).count()).toBeLessThan(all);
    for (const p of await hits(page).getByTestId('hit-path').allTextContents()) expect(p).toMatch(/Foto/);
  });

  test('a kind filter narrows the results', async ({ page }) => {
    await page.keyboard.press(`${await modKey(page)}+f`);
    await search(page, 'compleanno');
    expect(await hitNames(page)).toEqual(['Compleanno Giulia', 'Compleanno Giulia.webm']);
    await page.getByTestId('filter-chip-kind').click();
    await page.getByTestId('filter-menu').getByRole('checkbox', { name: 'Video' }).click();
    await expect.poll(() => hitNames(page)).toEqual(['Compleanno Giulia.webm']);
    await expect(page.getByTestId('filter-chip-kind')).toContainText('Video');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('filter-menu')).toHaveCount(0);
    await page.getByTestId('filter-clear-kind').click();
    await expect.poll(() => hitNames(page)).toEqual(['Compleanno Giulia', 'Compleanno Giulia.webm']);
  });

  test('filters without a query narrow the folder on screen', async ({ page }) => {
    await item(page, 'Lavoro').dblclick();
    await expectFolder(page, 'Lavoro');
    await field(page).click();
    await page.getByTestId('filter-chip-size').click();
    await page.getByTestId('filter-menu').getByRole('radio', { name: '1–100 MB' }).click();
    await expect.poll(() => names(page)).toEqual(['Presentazione Q3.pptx', 'Preventivo cucina.pdf']);
    await page.getByTestId('filter-clear-all').click();
    await expect.poll(async () => (await names(page)).length).toBe(7);
  });
});

test.describe('tags', () => {
  test('create, give to two files, chips, filter by it, rename, delete', async ({ page }) => {
    await boot(page, 'screen=vault&view=list&dir=Lavoro');
    await listed(page);
    const MOD = await modKey(page);
    await item(page, 'Budget 2025.xlsx').click();
    await item(page, 'Contratto affitto.pdf').click({ modifiers: [MOD] });

    await page.getByTestId('tag-selection').click();
    const picker = page.getByTestId('tag-picker');
    await expect(picker).toBeVisible();
    await expect(picker.getByRole('checkbox', { name: /Lavoro/ })).toHaveAttribute('aria-checked', 'mixed');
    await expect(page.getByTestId('tag-picker-input')).toBeFocused();
    await page.getByTestId('tag-picker-input').fill('Urgente');
    await page.keyboard.press('Enter');
    await expect(picker.getByRole('checkbox', { name: /Urgente/ })).toHaveAttribute('aria-checked', 'true');
    await page.keyboard.press('Escape');
    await expect(picker).toHaveCount(0);

    const chip = (name: string) => item(page, name).getByTestId('tag-chip').filter({ hasText: 'Urgente' });
    await expect(chip('Budget 2025.xlsx')).toBeVisible();
    await expect(chip('Contratto affitto.pdf')).toBeVisible();
    await expect(item(page, 'Note riunione.md').getByTestId('tag-chip').filter({ hasText: 'Urgente' })).toHaveCount(0);

    await field(page).click();
    await page.getByTestId('filter-chip-tags').click();
    await page.getByTestId('filter-menu').getByRole('checkbox', { name: /Urgente/ }).click();
    await expect.poll(() => names(page)).toEqual(['Budget 2025.xlsx', 'Contratto affitto.pdf']);
    await page.keyboard.press('Escape');

    await page.getByTestId('tags-manage').click();
    const manager = page.getByTestId('tag-manager');
    const name = manager.getByRole('textbox', { name: 'Rinomina Urgente' });
    await name.fill('Urgentissimo');
    await name.press('Enter');
    await expect(item(page, 'Budget 2025.xlsx').getByTestId('tag-chip').filter({ hasText: 'Urgentissimo' })).toBeVisible();
    await expect(page.getByTestId('filter-chip-tags')).toContainText('Urgentissimo');

    const tagId = await manager.getByTestId('tag-row').filter({ has: page.locator('[aria-label="Colore di Urgentissimo"]') }).getAttribute('data-tag');
    const renamed = manager.locator(`[data-testid=tag-row][data-tag="${tagId}"]`);
    await renamed.getByTestId('tag-ink').click();
    await renamed.locator('[data-testid=tag-swatch][data-color="prussia"]').click();
    await expect(item(page, 'Budget 2025.xlsx').getByTestId('tag-chip').filter({ hasText: 'Urgentissimo' })).toHaveAttribute('data-accent', 'prussia');

    await renamed.getByTestId('tag-delete').click();
    await expect(renamed).toContainText('Lo perderanno 2 elementi.');
    await manager.getByTestId('tag-delete-confirm').click();
    await expect(renamed).toHaveCount(0);
    await page.keyboard.press('Escape');
    await expect.poll(async () => (await names(page)).length).toBe(7);
    await expect(fileList(page).getByTestId('tag-chip').filter({ hasText: 'Urgentissimo' })).toHaveCount(0);
  });

  test('the context menu has Tag…; tags show on grid tiles', async ({ page }) => {
    await boot(page, 'screen=vault&view=grid&dir=Foto/Vacanze 2024');
    await listed(page);
    await expect(item(page, 'Lago di Braies.jpg').getByTestId('tag-chip')).toContainText('Vacanze');
    await item(page, 'Rifugio.jpg').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Tag…' }).click();
    const picker = page.getByTestId('tag-picker');
    await picker.getByRole('checkbox', { name: /Vacanze/ }).click();
    await expect(item(page, 'Rifugio.jpg').getByTestId('tag-chip')).toContainText('Vacanze');
    await picker.getByRole('checkbox', { name: /Vacanze/ }).click();
    await expect(item(page, 'Rifugio.jpg').getByTestId('tag-chip')).toHaveCount(0);
  });
});
