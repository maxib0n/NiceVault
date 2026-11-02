import { boot, expect, expectScreen, listed, test, toasts } from './helpers';

test.describe.configure({ timeout: 90_000 });

const osDrop = (page: import('@playwright/test').Page, paths: string[]) =>
  page.evaluate((p) => (window as unknown as { __nv: { mock: { simulateOsDrag: (e: unknown) => void } } }).__nv.mock.simulateOsDrag({ type: 'drop', paths: p }), paths);

test('La mia chiave: create, public key and fingerprint, export, remove, import', async ({ page }) => {
  await boot(page, 'screen=settings');
  const box = page.getByTestId('mykey');
  await expect(box).toHaveAttribute('data-state', 'none');
  await expect(page.getByTestId('mykey-portable')).toBeVisible();
  await page.getByTestId('mykey-create').click();
  const form = page.getByTestId('mykey-create-form');
  const go = page.getByTestId('mykey-create-submit');
  await form.getByLabel('Password della chiave').fill('corta');
  await expect(go).toBeDisabled();
  await form.getByLabel('Password della chiave').fill('cavallo batteria graffetta 42');
  await form.getByLabel('Ripeti').fill('cavallo batteria graffetta 42');
  await expect(go).toBeEnabled({ timeout: 10000 });
  await go.click();
  await expect(box).toHaveAttribute('data-state', 'ready', { timeout: 10000 });
  await expect(page.getByTestId('mykey-public')).toHaveAttribute('data-public', /^nvpk1[A-Za-z0-9_-]{1627}$/);
  await expect(page.getByTestId('mykey-fingerprint')).toHaveText(/^([0-9a-f]{4} ){7}[0-9a-f]{4}$/);

  await page.getByTestId('mykey-save-public').click();
  await expect(toasts(page)).toContainText('la-mia-chiave.nvpub');
  await page.getByTestId('mykey-export').click();
  await expect(toasts(page)).toContainText('la-mia-chiave.nvkey');

  await page.getByTestId('mykey-remove').click();
  await page.getByTestId('mykey-remove-confirm').click();
  await expect(box).toHaveAttribute('data-state', 'none');

  await page.getByTestId('mykey-import').click();
  const imp = page.getByTestId('mykey-import-form');
  await expect(page.getByTestId('mykey-import-submit')).toBeDisabled();
  await page.getByTestId('mykey-import-file').click();
  await expect(imp).toContainText('export.nvkey');
  await imp.getByLabel('Password della chiave').fill('sbagliata');
  await page.getByTestId('mykey-import-submit').click();
  await expect(imp.getByRole('alert')).toHaveText('Password errata.', { timeout: 10000 });
  await imp.getByLabel('Password della chiave').fill('chiave');
  await page.getByTestId('mykey-import-submit').click();
  await expect(box).toHaveAttribute('data-state', 'ready', { timeout: 10000 });
});

test('Aggiungi destinatario: from a file with its fingerprint, paste checks, .nvkey refused, duplicate, remove', async ({ page }) => {
  await boot(page, 'screen=details');
  const slots = page.getByTestId('slots');
  await expect(slots.locator('[data-kind="recipient"]')).toHaveCount(0);
  await page.getByTestId('add-recipient').click();
  const add = page.getByTestId('rcp-submit');
  await expect(add).toBeDisabled();

  await page.getByTestId('rcp-key').fill('nvpk1QUESTA-NON-È-UNA-CHIAVE');
  await expect(page.getByTestId('rcp-key-error')).toHaveText('Non è una chiave pubblica NiceVault valida (nvpk1…).');
  await expect(add).toBeDisabled();
  await osDrop(page, ['/home/utente/Scaricati/anna.nvkey']);
  await expect(page.getByTestId('rcp-key-error')).toContainText('file di chiave segreta');

  await page.getByTestId('rcp-file').click();
  await expect(page.getByTestId('rcp-key-error')).toHaveCount(0);
  await expect(page.getByTestId('rcp-fingerprint')).toHaveText(/^Impronta: ([0-9a-f]{4} ){7}[0-9a-f]{4}$/);
  const key = await page.getByTestId('rcp-key').inputValue();
  expect(key).toMatch(/^nvpk1/);
  await page.getByTestId('rcp-label').fill('Anna');
  await add.click();
  await expect(page.getByTestId('rcp-form')).toHaveCount(0);
  const anna = slots.locator('[data-kind="recipient"]');
  await expect(anna).toHaveCount(1);
  await expect(anna).toContainText('Anna');
  await expect(anna.getByTestId('slot-fingerprint')).toHaveText(/^([0-9a-f]{4} ){3}[0-9a-f]{4} …$/);

  await page.getByTestId('add-recipient').click();
  await page.getByTestId('rcp-key').fill(`${key.slice(0, 800)}\n${key.slice(800)}`);
  await expect(page.getByTestId('rcp-fingerprint')).toBeVisible();
  await page.getByTestId('rcp-submit').click();
  await expect(page.getByTestId('rcp-error')).toHaveText('Questa persona può già aprire il volume.');
  await page.getByRole('dialog').getByRole('button', { name: 'Annulla' }).click();

  await anna.getByRole('button', { name: 'Rimuovi' }).click();
  await expect(page.getByRole('dialog')).toContainText('non potrà più aprire il volume con la sua chiave');
  await page.getByTestId('remove-confirm').click();
  await expect(anna).toHaveCount(0);
});

test('my own key on my volume, then Sblocca → Apri con la mia chiave', async ({ page }) => {
  await boot(page, 'screen=details&mykey=mine');
  await page.getByTestId('add-recipient').click();
  await page.getByTestId('rcp-use-mine').click();
  await expect(page.getByTestId('rcp-fingerprint')).toContainText('La tua chiave');
  await expect(page.getByTestId('rcp-label')).toHaveValue('Io');
  await page.getByTestId('rcp-submit').click();
  const mine = page.getByTestId('slots').locator('[data-kind="recipient"]');
  await expect(mine.getByTestId('slot-mine')).toHaveText('La tua chiave');

  await page.getByRole('button', { name: 'Blocca' }).click();
  await expectScreen(page, 'unlock');
  const keyPw = page.getByLabel('Password della mia chiave');
  await expect(keyPw).toBeVisible();
  await keyPw.fill('demo');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert').filter({ hasText: 'Password errata' })).toBeVisible();
  await page.getByTestId('unlock-key-toggle').click();
  await expect(page.getByLabel('Password', { exact: true })).toBeVisible();
  await page.getByTestId('unlock-key-toggle').click();
  await keyPw.fill('chiave');
  await page.keyboard.press('Enter');
  await expectScreen(page, 'vault');
  await listed(page);
  await page.getByTestId('nav-details').click();
  await expect(page.getByTestId('slots').locator('[data-kind="recipient"][data-in-use="true"]')).toHaveCount(1);
});

test('a volume not shared with my key offers only its password', async ({ page }) => {
  await boot(page, 'screen=welcome&mykey=mine');
  await page.getByRole('button', { name: /Documenti/ }).first().click();
  await expectScreen(page, 'unlock');
  await expect(page.getByLabel('Password', { exact: true })).toBeVisible();
  await expect(page.getByTestId('unlock-key-toggle')).toHaveCount(0);
});

test('revoke with rotation: the other recipients stay under the new key', async ({ page }) => {
  await boot(page, 'screen=details&mykey=shared');
  const slots = page.getByTestId('slots');
  await expect(slots.locator('[data-kind="recipient"]')).toHaveCount(2);
  await slots.locator('[data-kind="recipient"]').filter({ hasText: 'Anna' }).getByRole('button', { name: 'Rimuovi' }).click();
  await page.getByTestId('remove-rotate').click();
  const form = page.getByTestId('rotate-key');
  await expect(form).toContainText('i destinatari');
  await page.getByTestId('rotate-next').click();
  await form.locator('input[type="password"]').fill('demo');
  await page.getByTestId('rotate-confirm').click();
  await expect(page.getByTestId('rotate-done')).toBeVisible({ timeout: 10000 });
  await page.getByText('Chiudi', { exact: true }).click();
  const left = slots.locator('[data-kind="recipient"]');
  await expect(left).toHaveCount(1);
  await expect(left).toContainText('Io');
  await expect(left.getByTestId('slot-mine')).toBeVisible();
});
