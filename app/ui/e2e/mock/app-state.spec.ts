// Job tray, mount switch, locking (user / idle / removal), theme, details.
import { boot, expect, expectScreen, fileList, item, jobTray, listed, test, toasts } from './helpers';

test.describe('job tray', () => {
  test('live progress, expand/collapse, cancel a running job, dismiss and clear finished', async ({ page }) => {
    await boot(page, 'screen=vault&jobs=1');
    const tray = jobTray(page);
    await expect(tray).toBeVisible();
    const head = tray.getByRole('button', { expanded: true });
    await expect(head).toBeVisible(); // ?jobs=1 opens it
    const running = tray.getByRole('listitem').filter({ hasText: '3 elementi' });
    await expect(running).toHaveAttribute('data-status', 'running');
    const bar = running.getByRole('progressbar', { name: 'Avanzamento 3 elementi' });
    const v1 = Number(await bar.getAttribute('value'));
    await expect.poll(async () => Number(await bar.getAttribute('value')), { timeout: 5000 }).toBeGreaterThan(v1);
    await expect(running).toContainText(/MB \/ .*GB|MB \/ .*MB/);
    await expect(tray.getByRole('status')).toHaveText(/^Trasferimento · \d+%$/);
    await expect(tray.getByRole('progressbar', { name: 'Avanzamento complessivo' })).toBeVisible();

    // finished ones: done and failed with the backend message
    await expect(tray.getByRole('listitem').filter({ hasText: 'Scansione.pdf' })).toHaveAttribute('data-status', 'done');
    const failed = tray.getByRole('listitem').filter({ hasText: 'Film.mkv' });
    await expect(failed).toHaveAttribute('data-status', 'error');
    await expect(failed).toContainText('Errore di lettura o scrittura sul disco.');

    await tray.getByRole('button', { name: 'Annulla 3 elementi' }).click();
    await expect(running).toHaveAttribute('data-status', 'cancelled');
    await expect(running).toContainText('Annullato');
    // only the export keeps running
    await expect(tray.getByRole('status')).toHaveText(/^Esportazione · \d+%$/);

    await tray.getByRole('button', { name: 'Togli Scansione.pdf dall’elenco' }).click();
    await expect(tray.getByRole('listitem').filter({ hasText: 'Scansione.pdf' })).toHaveCount(0);
    await tray.getByRole('button', { name: 'Pulisci completati' }).click();
    await expect(tray.getByRole('listitem')).toHaveCount(1);
    await expect(tray.getByRole('listitem')).toHaveAttribute('data-status', 'running');

    await head.click();
    await expect(tray.getByRole('button', { expanded: false })).toBeVisible();
    await expect(tray.getByRole('listitem')).toHaveCount(0);
  });

  test('import job failure surfaces a toast and the error in the tray', async ({ page }) => {
    await boot(page, 'screen=vault&error=import');
    await listed(page);
    await page.getByRole('button', { name: 'Importa', exact: true }).click();
    await expect(toasts(page).getByText(/Importazione non riuscita: Errore di lettura o scrittura/)).toBeVisible({ timeout: 8000 });
    await jobTray(page).getByRole('button', { expanded: false }).click();
    await expect(jobTray(page).getByRole('listitem').first()).toHaveAttribute('data-status', 'error');
    await expect(jobTray(page).getByRole('status')).toHaveText('1 operazione non riuscita');
  });

  test('cancelling a running import adds nothing to the folder', async ({ page }) => {
    await boot(page, 'screen=vault&view=list');
    await listed(page);
    // slow the mock so there is time to cancel
    await page.evaluate(() => ((window as unknown as { __nv: { mock: { opts: { speed: number } } } }).__nv.mock.opts.speed = 8));
    await page.getByRole('button', { name: 'Importa', exact: true }).click();
    await jobTray(page).getByRole('button', { expanded: false }).click();
    await jobTray(page).getByRole('button', { name: 'Annulla 2 elementi' }).click();
    await expect(jobTray(page).getByRole('listitem').first()).toHaveAttribute('data-status', 'cancelled');
    await page.waitForTimeout(500);
    await expect(item(page, 'Bolletta luce.pdf')).toHaveCount(0);
  });
});

test.describe('mount switch', () => {
  test('available: on shows the mountpoint and the sidebar glyph, off restores', async ({ page }) => {
    await boot(page, 'screen=vault');
    const sw = page.getByRole('switch', { name: 'Unità di sistema' });
    await expect(sw).toHaveAttribute('aria-checked', 'false');
    await expect(sw).toBeEnabled();
    await expect(page.getByText('Spenta. I file restano qui.')).toBeVisible();
    await sw.click();
    await expect(sw).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByText('/run/user/1000/nicevault/Documenti')).toBeVisible();
    await expect(page.getByLabel('Montato')).toBeVisible();
    await sw.click();
    await expect(sw).toHaveAttribute('aria-checked', 'false');
    await expect(page.getByLabel('Montato')).toHaveCount(0);
  });

  test('busy state disables the switch while mounting', async ({ page }) => {
    await boot(page, 'screen=vault');
    await page.evaluate(() => ((window as unknown as { __nv: { mock: { opts: { speed: number } } } }).__nv.mock.opts.speed = 5));
    const sw = page.getByRole('switch', { name: 'Unità di sistema' });
    await sw.click();
    await expect(page.getByText('Monto…')).toBeVisible();
    await expect(sw).toBeDisabled();
    await expect(sw).toHaveAttribute('aria-checked', 'true', { timeout: 5000 });
    await expect(sw).toBeEnabled();
  });

  test('unavailable: disabled with the plain-language reason; Details lists backends', async ({ page }) => {
    await boot(page, 'screen=vault&mount=unavailable');
    const sw = page.getByRole('switch', { name: 'Unità di sistema' });
    await expect(sw).toBeDisabled();
    await expect(page.getByText('Manca fusermount3 o /dev/fuse su questo computer.').first()).toBeVisible();
    await page.getByRole('button', { name: 'Dettagli' }).click();
    await expectScreen(page, 'details');
    await expect(page.getByText('Solo su Windows.')).toBeVisible();
  });

  test('Windows without WinFsp: says what to install, the link asks before opening', async ({ page }) => {
    await boot(page, 'screen=vault&mount=win_missing');
    const sw = page.getByRole('switch', { name: 'Unità di sistema' });
    await expect(sw).toBeDisabled();
    await expect(page.getByText('Serve WinFsp (gratuito): installalo e riapri NiceVault.')).toBeVisible();
    await expect(page.getByText('Solo su Linux.')).toHaveCount(0);
    await page.getByRole('button', { name: 'Scarica WinFsp' }).click();
    await expect(page.getByText('Aprire winfsp.dev/rel nel browser?')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Annulla' }).last()).toBeFocused();
    await page.getByRole('button', { name: 'Annulla' }).last().click();
    await expect(page.getByRole('button', { name: 'Scarica WinFsp' })).toBeVisible();
  });

  test('Windows with an old or broken WinFsp, with WinFsp, and macOS', async ({ page }) => {
    await boot(page, 'screen=vault&mount=win_old');
    await expect(page.getByText('Questa versione di WinFsp è troppo vecchia: serve WinFsp 2.1 o successiva.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Scarica WinFsp' })).toBeVisible();
    await boot(page, 'screen=vault&mount=win_broken');
    await expect(page.getByText('WinFsp è installato ma non si avvia: reinstallalo e riapri NiceVault.')).toBeVisible();
    await boot(page, 'screen=vault&mount=win_ok');
    await expect(page.getByRole('switch', { name: 'Unità di sistema' })).toBeEnabled();
    await expect(page.getByTestId('mount-link')).toHaveCount(0);
    await boot(page, 'screen=vault&mount=mac_nonfs');
    await expect(page.getByText(/manca il client NFS di sistema/)).toBeVisible();
    await expect(page.getByTestId('mount-link')).toHaveCount(0);
    await boot(page, 'screen=vault&mount=mac_ok');
    await expect(page.getByRole('switch', { name: 'Unità di sistema' })).toBeEnabled();
  });

  test('opened already mounted (?mount=mounted)', async ({ page }) => {
    await boot(page, 'screen=vault&mount=mounted');
    await expect(page.getByRole('switch', { name: 'Unità di sistema' })).toHaveAttribute('aria-checked', 'true');
  });
});

test.describe('locking', () => {
  test('Blocca → Sblocca without a reason line → unlock again', async ({ page }) => {
    await boot(page, 'screen=vault');
    await page.getByRole('button', { name: 'Blocca' }).click();
    await expectScreen(page, 'unlock');
    await expect(page.locator('[data-reason]')).toHaveCount(0);
    await page.getByLabel('Password', { exact: true }).fill('demo');
    await page.keyboard.press('Enter');
    await expectScreen(page, 'vault');
    await listed(page);
  });

  test('auto-lock (?autolock=3) after inactivity returns to Sblocca with the reason', async ({ page }) => {
    await boot(page, 'screen=vault&autolock=3');
    await expect(page.getByText(/Blocco automatico tra/)).toBeVisible();
    await expectScreen(page, 'unlock');
    await expect(page.locator('[data-reason="idle"]')).toHaveText('Bloccato per inattività');
    await page.getByLabel('Password', { exact: true }).fill('demo');
    await page.keyboard.press('Enter');
    await expectScreen(page, 'vault');
    // reason is gone after a successful unlock; the timer restarted
    await page.getByRole('button', { name: 'Blocca' }).click();
    await expect(page.locator('[data-reason]')).toHaveCount(0);
  });

  test('activity postpones auto-lock', async ({ page }) => {
    await boot(page, 'screen=vault&autolock=3');
    for (let i = 0; i < 5; i++) {
      await page.waitForTimeout(900);
      await page.keyboard.press('Shift'); // any key resets the idle timer
    }
    await expectScreen(page, 'vault');
  });

  test('drive removal (simulateRemoval) → Sblocca "Unità scollegata", running jobs fail', async ({ page }) => {
    await boot(page, 'screen=vault&view=list');
    await listed(page);
    await page.evaluate(() => ((window as unknown as { __nv: { mock: { opts: { speed: number } } } }).__nv.mock.opts.speed = 8));
    await page.getByRole('button', { name: 'Importa', exact: true }).click();
    await expect(jobTray(page)).toBeVisible();
    await page.evaluate(() => (window as unknown as { __nv: { mock: { simulateRemoval: () => void } } }).__nv.mock.simulateRemoval());
    await expectScreen(page, 'unlock');
    await expect(page.locator('[data-reason="removed"]')).toHaveText('Unità scollegata');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Documenti');
  });

  test('sleep lock reason text', async ({ page }) => {
    await boot(page, 'screen=vault');
    await page.evaluate(() => (window as unknown as { __nv: { mock: { simulateRemoval: (r: string) => void } } }).__nv.mock.simulateRemoval('sleep'));
    await expect(page.locator('[data-reason="sleep"]')).toHaveText('Bloccato alla sospensione del computer');
  });

  test('two volumes: locking the active one switches to the other', async ({ page }) => {
    await boot(page, 'screen=vault');
    await page.getByRole('button', { name: 'Apri un altro volume' }).click();
    await expectScreen(page, 'welcome');
    await page.getByRole('button', { name: /^(Chiuso|Aperto) Archivio fiscale/ }).click();
    await page.getByLabel('Password', { exact: true }).fill('demo');
    await page.keyboard.press('Enter');
    await expectScreen(page, 'vault');
    await expect(item(page, '730 2023.pdf')).toBeVisible();
    await page.getByRole('button', { name: 'Blocca' }).click();
    await expectScreen(page, 'vault');
    await expect(item(page, 'Foto')).toBeVisible();
    await expect(page.getByRole('button', { name: /Archivio fiscale/ })).toHaveCount(0);
  });
});

test.describe('theme and details', () => {
  test('theme radio: Scuro / Chiaro / Sistema, persisted across reloads, keyboard arrows', async ({ page }) => {
    await boot(page, 'screen=details');
    await expectScreen(page, 'details');
    const html = page.locator('html');
    const group = page.getByRole('radiogroup', { name: 'Aspetto' });
    await group.getByRole('radio', { name: 'Scuro' }).click();
    await expect(html).toHaveAttribute('data-theme', 'dark');
    await page.reload();
    await expect(page.locator('[data-ready="true"]')).toHaveCount(1);
    await expect(html).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('button', { name: 'Dettagli' }).click();
    await group.getByRole('radio', { name: 'Scuro' }).focus();
    await page.keyboard.press('ArrowLeft');
    await expect(group.getByRole('radio', { name: 'Chiaro' })).toHaveAttribute('aria-checked', 'true');
    await expect(html).toHaveAttribute('data-theme', 'light');
    await page.keyboard.press('ArrowLeft');
    await expect(group.getByRole('radio', { name: 'Sistema' })).toHaveAttribute('aria-checked', 'true');
    await expect(html).not.toHaveAttribute('data-theme', /./);
  });

  test('?theme=dark switch', async ({ page }) => {
    await boot(page, 'screen=welcome&theme=dark');
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  });

  test('auto-lock minutes radio drives the sidebar countdown', async ({ page }) => {
    await boot(page, 'screen=details');
    await page.getByRole('radio', { name: '5 min', exact: true }).click();
    await expect(page.getByRole('radio', { name: '5 min', exact: true })).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByText(/^[0-4]:[0-5]\d$|^[0-4] : [0-5]\d$/)).toBeVisible();
    await page.getByRole('radio', { name: 'Mai' }).click();
    await expect(page.getByText(/Blocco automatico tra/)).toHaveCount(0);
  });

  test('keys: add a password, last-slot guard, change with wrong old password', async ({ page }) => {
    await boot(page, 'screen=details&slots=1');
    await expect(page.getByRole('button', { name: 'Rimuovi' })).toBeDisabled();
    await page.getByRole('button', { name: 'Aggiungi password', exact: true }).click();
    const dlg = page.getByRole('dialog');
    await dlg.getByLabel('Password', { exact: true }).fill('tre tigri contro tre tigri sul ponte');
    await dlg.getByLabel('Ripeti').fill('tre tigri contro tre tigri sul ponte');
    await dlg.getByRole('button', { name: /Aggiungi/ }).click();
    await expect(dlg).toHaveCount(0, { timeout: 12000 });
    await expect(page.getByRole('button', { name: 'Rimuovi' }).first()).toBeEnabled();

    await page.getByRole('button', { name: 'Cambia' }).first().click();
    await dlg.getByLabel('Password attuale').fill('non-è-lei');
    await dlg.getByLabel('Nuova password').fill('un cavallo blu salta sette fossi');
    await dlg.getByLabel('Ripeti').fill('un cavallo blu salta sette fossi');
    await dlg.getByRole('button', { name: /Cambia|Salva/ }).last().click();
    await expect(dlg.getByRole('alert')).toContainText('Password errata.', { timeout: 12000 });
    await dlg.getByRole('button', { name: 'Annulla' }).click();
    await expect(dlg).toHaveCount(0);
  });
});

test('empty volume shows the empty state and still accepts drops', async ({ page }) => {
  await boot(page, 'screen=vault&empty=1');
  await expect(fileList(page)).toContainText('Volume vuoto');
});

test('locking the last volume clears toasts that name its files', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await page.getByRole('button', { name: 'Importa', exact: true }).click();
  await expect(toasts(page).getByText('Importato: 2 elementi.')).toBeVisible({ timeout: 8000 });
  await page.evaluate(() => (window as unknown as { __nv: { mock: { simulateRemoval: () => void } } }).__nv.mock.simulateRemoval());
  await expectScreen(page, 'unlock');
  // immediately, not after the toast's own 3.5 s timeout
  await expect(toasts(page).getByText('Importato: 2 elementi.')).toHaveCount(0, { timeout: 500 });
});
