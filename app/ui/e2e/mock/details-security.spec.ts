// Dettagli: auto-lock options per volume and the self-destruct password.
import { boot, expect, test } from './helpers';

test('auto-lock: minutes incl. Mai, sleep and screen switches, scope', async ({ page }) => {
  await boot(page, 'screen=details');
  const box = page.getByTestId('autolock');
  await expect(page.getByText(/Blocco automatico tra/)).toBeVisible();
  await box.getByTestId('autolock-minutes').getByRole('radio', { name: 'Mai' }).click();
  await expect(box.getByTestId('autolock-minutes').getByRole('radio', { name: 'Mai' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.getByText(/Blocco automatico tra/)).toHaveCount(0); // sidebar countdown hidden
  await box.getByTestId('lock-on-sleep').click();
  await expect(box.getByTestId('lock-on-sleep')).toHaveAttribute('aria-checked', 'false');
  await box.getByTestId('lock-on-screen').click();
  await expect(box.getByTestId('lock-on-screen')).toHaveAttribute('aria-checked', 'false');
  await box.getByTestId('lock-scope').getByRole('radio', { name: 'Tutti i volumi' }).click();
  // stored in the volume: back again after leaving the page
  await page.getByTestId('nav-files').click();
  await page.getByTestId('nav-details').click();
  await expect(box.getByTestId('lock-on-sleep')).toHaveAttribute('aria-checked', 'false');
  await expect(box.getByTestId('lock-scope').getByRole('radio', { name: 'Tutti i volumi' })).toHaveAttribute('aria-checked', 'true');
  await box.getByTestId('autolock-minutes').getByRole('radio', { name: '15 min' }).click();
  await expect(page.getByText(/Blocco automatico tra/)).toBeVisible();
});

test('self-destruct password: warning, confirm, conflict, added as its own kind, removable', async ({ page }) => {
  await boot(page, 'screen=details&slots=1');
  const slots = page.getByTestId('slots');
  await expect(slots.locator('[data-kind="duress"]')).toHaveCount(0);
  await page.getByTestId('add-duress').click();
  const form = page.getByTestId('duress-form');
  await expect(form).toHaveAttribute('data-step', '1');
  await expect(form).toContainText('copie del file fatte prima');
  const go = page.getByRole('button', { name: 'Continua' });
  await expect(go).toBeDisabled();
  await page.getByTestId('duress-understood').check();
  await go.click();
  await expect(form).toHaveAttribute('data-step', '2');
  // the volume's own password is refused (duress_conflict)
  await form.getByLabel('Password', { exact: true }).fill('demo');
  await form.getByLabel('Ripeti').fill('demo');
  await page.getByRole('button', { name: 'Aggiungi', exact: true }).click();
  await expect(page.getByTestId('duress-error')).toHaveText('Deve essere diversa da tutte le altre password del volume.');
  await form.getByLabel('Password', { exact: true }).fill('tutto-perduto');
  await form.getByLabel('Ripeti').fill('tutto-perduto');
  await page.getByRole('button', { name: 'Aggiungi', exact: true }).click();
  await expect(form).toHaveCount(0);
  const duress = slots.locator('[data-kind="duress"]');
  await expect(duress).toHaveCount(1);
  await expect(duress).toContainText('Distrugge il volume se usata');
  // the only real key still cannot go; the self-destruct one can
  await expect(slots.locator('[data-kind="password"]').getByRole('button', { name: 'Rimuovi' })).toBeDisabled();
  await duress.getByRole('button', { name: 'Rimuovi' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Rimuovi' }).click();
  await expect(duress).toHaveCount(0);
});

test('rotate the volume key: what it needs, which password stays, wrong password, done', async ({ page }) => {
  await boot(page, 'screen=details');
  await page.getByTestId('rotate-open').click();
  const form = page.getByTestId('rotate-key');
  await expect(form).toHaveAttribute('data-step', '1');
  await page.getByTestId('rotate-next').click();
  await expect(form).toHaveAttribute('data-step', '2');
  const confirm = page.getByTestId('rotate-confirm');
  await expect(confirm).toBeDisabled();
  await form.locator('input[type="password"]').fill('not the password');
  await confirm.click();
  await expect(form.getByRole('alert')).toBeVisible({ timeout: 10000 });
  await form.locator('input[type="password"]').fill('demo');
  await confirm.click();
  await expect(page.getByTestId('rotate-done')).toBeVisible({ timeout: 10000 });
  await page.getByRole('button', { name: 'Chiudi' }).click();
  // only the kept password is left
  await expect(page.getByTestId('slots').locator('[data-kind="password"]')).toHaveCount(1);
});

test('emblem frame: chosen in the picker, drawn on the volume, kept after leaving the page', async ({ page }) => {
  await boot(page, 'screen=details');
  await page.getByTestId('emblem-open').click();
  const frames = page.getByTestId('emblem-frames');
  await expect(frames.getByRole('radio')).toHaveCount(6);
  await frames.locator('button[data-frame="laurel"]').click();
  await expect(frames.locator('button[data-frame="laurel"]')).toHaveAttribute('aria-checked', 'true');
  await page.getByTestId('emblem-save').click();
  await expect(page.getByTestId('emblem-picker')).toHaveCount(0);
  await page.getByTestId('nav-files').click();
  await page.getByTestId('nav-details').click();
  await expect(page.locator('.rosette[data-frame="laurel"]').first()).toBeVisible();
});
