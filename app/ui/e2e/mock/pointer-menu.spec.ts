import { boot, expect, expectFolder, fileList, item, jobTray, listed, modKey, names, selectedNames, test, toasts } from './helpers';

test.beforeEach(async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
});

test('click, Shift-click range, Ctrl/Cmd-click toggle, background click clears, double-click opens', async ({ page }) => {
  const MOD = await modKey(page);
  await item(page, 'Audio').click();
  await item(page, 'Lavoro').click({ modifiers: ['Shift'] });
  expect(await selectedNames(page)).toEqual(['Audio', 'Chiavi', 'Foto', 'Lavoro']);
  await item(page, 'Chiavi').click({ modifiers: [MOD] });
  expect(await selectedNames(page)).toEqual(['Audio', 'Foto', 'Lavoro']);
  await item(page, 'Leggimi.txt').click({ modifiers: [MOD] });
  expect(await selectedNames(page)).toEqual(['Audio', 'Foto', 'Lavoro', 'Leggimi.txt']);
  // shift-click from the last ctrl-click anchor
  await item(page, 'Copertina.jpg').click({ modifiers: ['Shift'] });
  expect(await selectedNames(page)).toEqual(['Copertina.jpg', 'Foto profilo.jpg', 'Leggimi.txt']);
  await expect(page.getByRole('button', { name: 'Esporta', exact: true })).toBeEnabled();
  // click on empty space of the listbox clears
  const box = (await fileList(page).boundingBox())!;
  await page.mouse.click(box.x + box.width / 2, box.y + box.height - 20);
  expect(await selectedNames(page)).toEqual([]);
  await expect(page.getByRole('button', { name: 'Esporta', exact: true })).toBeDisabled();
  await item(page, 'Personale').dblclick();
  await expectFolder(page, 'Personale');
  await item(page, 'Tessera sanitaria.png').dblclick();
  await expect(page.getByRole('dialog', { name: 'Anteprima di Tessera sanitaria.png' })).toBeVisible();
  await page.getByRole('button', { name: 'Chiudi anteprima' }).click();
  await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
});

test('breadcrumb click navigates up several levels', async ({ page }) => {
  await item(page, 'Foto').dblclick();
  await item(page, 'Vacanze 2024').dblclick();
  await expectFolder(page, 'Vacanze 2024');
  await page.getByRole('navigation', { name: 'Percorso' }).getByRole('button', { name: 'Documenti' }).click();
  await expectFolder(page, 'Documenti');
});

test('context menu on a file: Anteprima, Rinomina, Esporta, Elimina', async ({ page }) => {
  await item(page, 'Copertina.jpg').click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Azioni elemento' });
  await expect(menu.getByRole('menuitem')).toHaveText([/Anteprima/, /Apri con app esterna/, /Rinomina/, /Tag…/, /Esporta…/, /Proprietà/, /Elimina/]);
  expect(await selectedNames(page)).toEqual(['Copertina.jpg']);
  await menu.getByRole('menuitem', { name: /Anteprima/ }).click();
  await expect(page.getByRole('dialog', { name: 'Anteprima di Copertina.jpg' })).toBeVisible();
  await page.keyboard.press('Escape');

  await item(page, 'Copertina.jpg').click({ button: 'right' });
  await menu.getByRole('menuitem', { name: /Rinomina/ }).click();
  await expect(page.getByRole('textbox', { name: 'Nuovo nome' })).toBeFocused();
  await page.keyboard.type('Cover');
  await page.keyboard.press('Enter');
  await expect(item(page, 'Cover.jpg')).toBeVisible();

  await item(page, 'Cover.jpg').click({ button: 'right' });
  await menu.getByRole('menuitem', { name: /Esporta/ }).click();
  await expect(jobTray(page)).toBeVisible();
  await expect(toasts(page).getByText('Esportato in /home/utente/Scrivania.')).toBeVisible();

  await item(page, 'Cover.jpg').click({ button: 'right' });
  await menu.getByRole('menuitem', { name: /Elimina/ }).click();
  const dlg = page.getByRole('dialog', { name: 'Eliminare “Cover.jpg”?' });
  await expect(dlg).toBeVisible();
  await dlg.getByRole('button', { name: 'Elimina' }).click();
  await expect(item(page, 'Cover.jpg')).toHaveCount(0);
});

test('context menu: right-click inside a selection keeps it; plural labels; folder Apri', async ({ page }) => {
  await item(page, 'Audio').click();
  await item(page, 'Foto').click({ modifiers: ['Shift'] });
  await item(page, 'Chiavi').click({ button: 'right' });
  expect(await selectedNames(page)).toEqual(['Audio', 'Chiavi', 'Foto']);
  const menu = page.getByRole('menu', { name: 'Azioni elemento' });
  await expect(menu.getByRole('menuitem')).toHaveText([/Tag…/, /Esporta 3 elementi…/, /Proprietà/, /Elimina 3 elementi/]);
  // outside click closes
  await page.mouse.click(5, 5);
  await expect(menu).toHaveCount(0);
  // right-click outside the selection replaces it
  await item(page, 'Video').click({ button: 'right' });
  expect(await selectedNames(page)).toEqual(['Video']);
  await menu.getByRole('menuitem', { name: /Apri/ }).click();
  await expectFolder(page, 'Video');
});

test('background context menu: Nuova cartella twice gives unique names; Importa file…; Seleziona tutto', async ({ page }) => {
  const bg = async () => {
    // the list's end padding is the background (a full folder fills the visible part with rows)
    await fileList(page).evaluate((el) => (el.scrollTop = el.scrollHeight));
    const box = (await fileList(page).boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height - 20, { button: 'right' });
    return page.getByRole('menu', { name: 'Azioni cartella' });
  };
  let menu = await bg();
  await expect(menu.getByRole('menuitem')).toHaveText([/Nuova cartella/, /Importa file…/, /Seleziona tutto/, /Proprietà/]);
  await menu.getByRole('menuitem', { name: /Nuova cartella/ }).click();
  await expect(page.getByRole('textbox', { name: 'Nuovo nome' })).toBeFocused();
  await page.keyboard.press('Enter'); // keep the default name
  await expect(item(page, 'Nuova cartella')).toBeVisible();
  menu = await bg();
  await menu.getByRole('menuitem', { name: /Nuova cartella/ }).click();
  await expect(page.getByRole('textbox', { name: 'Nuovo nome' })).toHaveValue('Nuova cartella 2');
  await page.keyboard.press('Escape');
  await expect(item(page, 'Nuova cartella 2')).toBeVisible(); // created, rename cancelled

  menu = await bg();
  await menu.getByRole('menuitem', { name: /Importa file…/ }).click();
  await expect(item(page, 'Bolletta luce.pdf')).toBeVisible({ timeout: 8000 });
  await expect(item(page, 'Spiaggia.jpg')).toBeVisible();
  await expect(toasts(page).getByText('Importato: 2 elementi.')).toBeVisible();

  menu = await bg();
  await menu.getByRole('menuitem', { name: /Seleziona tutto/ }).click();
  expect((await selectedNames(page)).length).toBe((await names(page)).length);
});

test('rename collisions and invalid names keep the editor open with an error', async ({ page }) => {
  await item(page, 'Leggimi.txt').click();
  await page.keyboard.press('F2');
  const input = page.getByRole('textbox', { name: 'Nuovo nome' });
  await input.fill('Copertina.jpg');
  await page.keyboard.press('Enter');
  await expect(toasts(page).getByRole('alert').getByText('Esiste già un elemento con questo nome.')).toBeVisible();
  await expect(input).toBeVisible();
  await expect(input).toBeFocused();
  // folder name collision with a file name in the same dir
  await input.fill('Foto');
  await page.keyboard.press('Enter');
  await expect(toasts(page).getByText('Esiste già un elemento con questo nome.')).toHaveCount(2);
  await expect(input).toBeFocused();
  // a slash is not a valid name
  await input.fill('a/b.txt');
  await page.keyboard.press('Enter');
  await expect(toasts(page).getByText('Nome non valido.').first()).toBeVisible();
  await expect(input).toBeFocused();
  // empty / whitespace = no change
  await input.fill('   ');
  await page.keyboard.press('Enter');
  await expect(input).toHaveCount(0);
  await expect(item(page, 'Leggimi.txt')).toBeVisible();
  // trailing spaces are trimmed
  await page.keyboard.press('F2');
  await input.fill('  Leggimi ora.txt  ');
  await page.keyboard.press('Enter');
  await expect(item(page, 'Leggimi ora.txt')).toBeVisible();
  // blur commits
  await page.keyboard.press('F2');
  await input.fill('Leggi dopo.txt');
  await page.getByRole('heading').or(page.getByRole('navigation', { name: 'Percorso' })).first().click();
  await expect(item(page, 'Leggi dopo.txt')).toBeVisible();
  // new folder colliding with an existing one gets a free name instead of failing
  await page.getByRole('button', { name: 'Nuova cartella' }).click();
  await input.fill('Lavoro');
  await page.keyboard.press('Enter');
  await expect(toasts(page).getByText('Esiste già un elemento con questo nome.')).toHaveCount(3);
  await page.keyboard.press('Escape');
  await expect(item(page, 'Nuova cartella')).toBeVisible();
  expect((await names(page)).filter((n) => n === 'Lavoro')).toHaveLength(1);
});

test('sort menu (toolbar) changes order and direction', async ({ page }) => {
  await page.getByRole('button', { name: 'Ordina per: Nome' }).click();
  const menu = page.getByRole('menu', { name: 'Ordina per' });
  await expect(menu.getByRole('menuitemradio', { name: 'Nome' })).toHaveAttribute('aria-checked', 'true');
  await menu.getByRole('menuitemradio', { name: 'Decrescente' }).click();
  expect((await names(page))[0]).toBe('Video');
  await page.getByRole('button', { name: 'Ordina per: Nome' }).click();
  await menu.getByRole('menuitemradio', { name: 'Tipo' }).click();
  await expect(page.getByRole('button', { name: 'Ordina per: Tipo' })).toBeVisible();
});

test('Esc while a (slow) failing rename is still in flight does not throw', async ({ page }) => {
  await item(page, 'Leggimi.txt').click();
  await page.keyboard.press('F2');
  const input = page.getByRole('textbox', { name: 'Nuovo nome' });
  await input.fill('Copertina.jpg');
  // slow the mock backend down so the rename is still pending when Esc arrives
  await page.evaluate(() => ((window as unknown as { __nv: { mock: { opts: { speed: number } } } }).__nv.mock.opts.speed = 40));
  await page.keyboard.press('Enter');
  await page.keyboard.press('Escape');
  await expect(input).toHaveCount(0);
  await expect(toasts(page).getByText('Esiste già un elemento con questo nome.')).toBeVisible();
  await page.waitForTimeout(300);
  await expect(item(page, 'Leggimi.txt')).toBeVisible();
  // (the auto fixture fails the test on the TypeError the old code threw here)
});

test('an empty folder: right-click on its notice opens the folder menu', async ({ page }) => {
  await boot(page, 'screen=vault&dir=Lavoro/Archivio 2023');
  await page.locator('[data-state="empty-folder"] .empty-title').click({ button: 'right' });
  await expect(page.getByRole('menu', { name: 'Azioni cartella' })).toBeVisible();
});
