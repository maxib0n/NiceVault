import { boot, expect, expectScreen, fileList, listed, names, test } from './helpers';

test.describe('Benvenuto / Sblocca', () => {
  test('wrong password shows an error, clears the field, then the right one opens the vault', async ({ page }) => {
    await boot(page, 'screen=welcome');
    await expectScreen(page, 'welcome');
    await page.getByRole('button', { name: /Documenti/ }).first().click();
    await expectScreen(page, 'unlock');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Documenti');

    const pw = page.getByLabel('Password', { exact: true });
    const submit = page.getByRole('button', { name: 'Sblocca' });
    await expect(pw).toBeFocused();
    await expect(submit).toBeDisabled();

    await pw.fill('sbagliata');
    await expect(submit).toBeEnabled();
    const form = page.locator('[data-phase]');
    await page.keyboard.press('Enter');
    await expect(form).toHaveAttribute('data-phase', 'unlocking');
    await expect(page.getByRole('alert').filter({ hasText: 'Password errata' })).toBeVisible();
    await expect(form).toHaveAttribute('data-phase', 'error');
    await expect(form).toHaveAttribute('data-error-seq', '1');
    await expect(pw).toHaveValue('');
    await expect(pw).toBeFocused();
    await expect(pw).toHaveAttribute('aria-invalid', 'true');

    await pw.fill('ancora no');
    await page.keyboard.press('Enter');
    await expect(form).toHaveAttribute('data-error-seq', '2');

    await pw.fill('demo');
    await page.keyboard.press('Enter');
    await expectScreen(page, 'vault');
    await listed(page);
    expect(await names(page)).toContain('Foto');
  });

  test('show/hide password toggle and Altri volumi go back', async ({ page }) => {
    await boot(page, 'screen=unlock');
    const pw = page.getByLabel('Password', { exact: true });
    await pw.fill('segreto');
    await expect(pw).toHaveAttribute('type', 'password');
    await page.getByRole('button', { name: 'Mostra password' }).click();
    await expect(pw).toHaveAttribute('type', 'text');
    await page.getByRole('button', { name: 'Nascondi password' }).click();
    await expect(pw).toHaveAttribute('type', 'password');
    await page.getByRole('button', { name: 'Altri volumi' }).click();
    await expectScreen(page, 'welcome');
  });

  test('unreachable recent is disabled, forget removes it', async ({ page }) => {
    await boot(page, 'screen=welcome');
    const old = page.getByRole('button', { name: /^(Chiuso|Aperto) Vecchio/ });
    await expect(old).toBeDisabled();
    await page.getByRole('button', { name: 'Togli Vecchio dall’elenco' }).click();
    await expect(page.getByRole('button', { name: /^(Chiuso|Aperto) Vecchio/ })).toHaveCount(0);
  });

  test('Apri volume… goes to Sblocca for the picked file', async ({ page }) => {
    await boot(page, 'screen=welcome');
    await page.getByRole('button', { name: 'Apri volume…' }).click();
    await expectScreen(page, 'unlock');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Archivio fiscale');
  });
});

test.describe('Create wizard', () => {
  test('name validation on step 1', async ({ page }) => {
    await boot(page, 'screen=welcome');
    await page.getByRole('button', { name: 'Nuovo volume' }).click();
    await expectScreen(page, 'create');
    const name = page.getByLabel('Nome');
    const next = page.getByRole('button', { name: 'Avanti' });
    await expect(name).toHaveValue('Nuovo volume');
    await expect(next).toBeEnabled();

    await name.fill('');
    await expect(page.getByText('Serve un nome.')).toBeVisible();
    await expect(name).toHaveAttribute('aria-invalid', 'true');
    await expect(next).toBeDisabled();

    await name.fill('a/b');
    await expect(page.getByText(/Il nome non può contenere/)).toBeVisible();
    await expect(next).toBeDisabled();

    await name.fill('Documenti');
    await expect(page.getByText('Esiste già un volume con questo nome qui.')).toBeVisible();
    await expect(next).toBeDisabled();

    await name.fill('Ricevute.nvault');
    await expect(page.getByText('/run/media/utente/CHIAVETTA/Ricevute.nvault', { exact: true })).toBeVisible();
    await expect(next).toBeEnabled();

    await page.keyboard.press('Escape');
    await expectScreen(page, 'welcome');
  });

  test('weak password, mismatch, then strong + matching creates and opens an empty volume', async ({ page }) => {
    await boot(page, 'screen=welcome');
    await page.getByRole('button', { name: 'Nuovo volume' }).click();
    await page.getByLabel('Nome').fill('Ricevute');
    await page.keyboard.press('Enter');
    const wizard = page.locator('[data-step]');
    await expect(wizard).toHaveAttribute('data-step', '2');
    await expect(page.getByRole('listitem').filter({ hasText: 'Chiave' })).toHaveAttribute('aria-current', 'step');

    const pw = page.getByLabel('Password', { exact: true });
    const confirm = page.getByLabel('Ripeti la password');
    const create = page.getByRole('button', { name: 'Crea volume' });
    const meter = page.getByRole('meter', { name: 'Robustezza della password' });
    await expect(pw).toBeFocused();
    await expect(meter).toHaveAttribute('aria-valuetext', 'Nessuna password');

    await pw.fill('ciao');
    await confirm.fill('ciao');
    await expect(meter).not.toHaveAttribute('aria-valuetext', 'Nessuna password');
    await expect(page.getByText('Almeno 8 caratteri.')).toBeVisible();
    await expect(create).toBeDisabled();
    await pw.fill('password1');
    await confirm.fill('password1');
    await expect(meter).toHaveAttribute('aria-valuenow', /^[0-2]$/);
    await expect(create).toBeDisabled();
    await page.keyboard.press('Enter');
    await expect(wizard).toHaveAttribute('data-step', '2');

    const strong = 'cavallo batteria graffetta corretta';
    await pw.fill(strong);
    await confirm.fill(strong + 'x');
    await expect(meter).toHaveAttribute('aria-valuenow', '4');
    await expect(page.getByText('Le password non coincidono.')).toBeVisible();
    await expect(confirm).toHaveAttribute('aria-invalid', 'true');
    await expect(create).toBeDisabled();

    await page.getByRole('button', { name: 'Indietro' }).click();
    await expect(wizard).toHaveAttribute('data-step', '1');
    await expect(page.getByLabel('Nome')).toHaveValue('Ricevute');
    await page.getByRole('button', { name: 'Avanti' }).click();
    await expect(wizard).toHaveAttribute('data-step', '2');

    await page.getByLabel('Password', { exact: true }).fill(strong);
    await page.getByLabel('Ripeti la password').fill(strong);
    await expect(page.getByText('Le password non coincidono.')).toHaveCount(0);
    await expect(create).toBeEnabled();
    await page.keyboard.press('Enter');
    await expect(wizard).toHaveAttribute('data-step', '3');
    await expect(page.getByRole('status').filter({ hasText: 'Volume creato.' })).toBeVisible();
    await expect(page.getByText('/run/media/utente/CHIAVETTA/Ricevute.nvault')).toBeVisible();

    await page.getByRole('button', { name: 'Apri il volume' }).click();
    await expectScreen(page, 'vault');
    await expect(fileList(page)).toContainText('Volume vuoto');
    await expect(page.getByRole('button', { name: /Ricevute/ })).toHaveAttribute('aria-current', 'true');
  });
});
