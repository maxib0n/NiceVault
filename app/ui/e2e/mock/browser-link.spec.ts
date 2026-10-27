import { boot, expect, test } from './helpers';

test('Impostazioni → Browser: link, paired browsers, default volume, recent accesses', async ({ page }) => {
  await boot(page, 'screen=settings&installed=1&browser=paired');
  const s = page.getByTestId('settings-browser');
  await expect(s.getByRole('heading', { name: 'Browser', exact: true })).toBeVisible();
  const link = s.getByRole('switch', { name: 'Collega i browser' });
  await expect(link).toHaveAttribute('aria-checked', 'false');
  await link.click();
  await expect(link).toHaveAttribute('aria-checked', 'true');
  await expect(link).toHaveAccessibleDescription(/Registrato per: Google Chrome, Firefox/);
  await expect(s.getByTestId('browser-paired')).toContainText('Firefox 139 · Linux');
  await expect(s.getByTestId('browser-accesses')).toContainText('mail.example.com');
  await expect(s.getByTestId('browser-accesses')).toContainText('codice 2FA');

  const vol = s.getByTestId('browser-default-volume');
  await vol.selectOption({ label: 'Archivio fiscale' });
  await page.getByTestId('settings-back').click();
  await page.getByTestId('open-settings').click();
  await expect(page.getByTestId('browser-default-volume')).toHaveValue(/Archivio fiscale\.nvault$/);

  await page.getByTestId('browser-unpair').click();
  await expect(page.getByTestId('browser-paired')).toHaveCount(0);
  await expect(page.getByTestId('settings-browser')).toContainText('Nessun browser collegato');

  await page.getByRole('switch', { name: 'Collega i browser' }).click();
  await expect(page.getByRole('switch', { name: 'Collega i browser' })).toHaveAttribute('aria-checked', 'false');
});

test('from a stick: one line instead of the switch; sandboxed browsers are named', async ({ page }) => {
  await boot(page, 'screen=settings&browser=sandbox');
  const s = page.getByTestId('settings-browser');
  await expect(s.getByTestId('browser-portable')).toBeVisible();
  await expect(s.getByRole('switch', { name: 'Collega i browser' })).toHaveCount(0);
  await expect(s.getByTestId('browser-sandboxed')).toContainText('Firefox (Flatpak)');
});

test('a browser asks to pair: the code is shown and the user decides here', async ({ page }) => {
  await boot(page, 'screen=welcome&browser=pair');
  const dlg = page.getByRole('dialog', { name: 'Collegare Chrome 156 · Linux a NiceVault?' });
  await expect(dlg).toBeVisible();
  await expect(dlg.getByTestId('browser-pair-code')).toHaveText('482 913');
  await dlg.getByRole('button', { name: 'Collega' }).click();
  await expect(dlg).toBeHidden();
  await page.getByTestId('open-settings').click();
  await expect(page.getByTestId('browser-paired')).toContainText('Chrome 156 · Linux');
});

test('Disponibile nel browser, per volume', async ({ page }) => {
  await boot(page, 'screen=details');
  const sw = page.getByRole('switch', { name: 'Disponibile nel browser' });
  await expect(sw).toHaveAttribute('aria-checked', 'true');
  await sw.click();
  await expect(sw).toHaveAttribute('aria-checked', 'false');
});

test('the compact unlock window: the volume password, typed in NiceVault', async ({ page }) => {
  await page.goto('/?compact=unlock&path=' + encodeURIComponent('/run/media/utente/CHIAVETTA/Documenti.nvault'));
  const w = page.getByTestId('unlock-compact');
  await expect(w.getByRole('heading', { level: 1 })).toHaveText('Sblocca «Documenti»');
  const pw = page.getByLabel('Password del volume', { exact: true });
  await expect(pw).toBeFocused();
  await pw.fill('sbagliata');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert')).toContainText('Password errata');
  await pw.fill('demo');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('unlock-compact-go')).toBeDisabled();
});
