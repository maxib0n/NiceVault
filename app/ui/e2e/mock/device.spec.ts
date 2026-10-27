import { boot, expect, expectScreen, test } from './helpers';

test.describe('Crea dispositivo', () => {
  test('from Benvenuto: drive, options, summary with commands, typed name, progress, first volume', async ({ page }) => {
    await boot(page, 'screen=welcome');
    await page.getByTestId('open-device').click();
    await expectScreen(page, 'device');
    const disks = page.getByTestId('device-disk');
    await expect(disks).toHaveCount(2);
    await expect(disks.first()).toContainText('SanDisk Ultra');
    await expect(disks.first()).toContainText('USB');
    await expect(disks.first()).toContainText('…1103235');
    await expect(disks.first()).toContainText('CHIAVETTA');
    await expect(page.getByText('Sono elencate solo unità esterne o rimovibili.')).toBeVisible();
    await expect(page.getByTestId('device-next')).toBeDisabled();
    await disks.first().click();
    await page.getByTestId('device-next').click();

    const fsGroup = page.getByTestId('device-fs');
    await expect(fsGroup.getByRole('radio', { name: /exFAT/ })).toHaveAttribute('aria-checked', 'true');
    await expect(fsGroup.getByRole('radio', { name: /ext4/ })).toContainText('Non leggibile su Windows e macOS');
    await expect(fsGroup.getByRole('radio', { name: /btrfs/ })).toBeDisabled();
    await expect(fsGroup.getByRole('radio', { name: /btrfs/ })).toContainText('btrfs-progs');
    await expect(fsGroup.getByRole('radio', { name: /NTFS/ })).toContainText(/macOS\s*·\s*sola lettura/);
    await expect(page.getByTestId('device-table').getByRole('radio', { name: 'MBR' })).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByTestId('device-download')).toHaveAttribute('aria-checked', 'true');
    await page.getByTestId('device-first-volume').click();
    await expect(page.getByTestId('device-volume-mode').getByRole('radio', { name: 'Tutta la partizione' })).toHaveAttribute('aria-checked', 'true');
    await page.getByLabel('Nome del volume').fill('Dati');
    await page.getByTestId('device-next').click();

    const sum = page.getByTestId('device-summary');
    await expect(sum).toContainText('Tutto il contenuto di SanDisk Ultra');
    await expect(sum).toContainText('Non si può annullare.');
    await expect(sum).toContainText('VaultData');
    await page.getByTestId('device-show-commands').click();
    await expect(page.getByTestId('device-commands')).toContainText('org.freedesktop.UDisks2.Block.Format');
    await page.getByTestId('device-continue').click();

    await expect(page.getByText('Per confermare scrivi: SanDisk Ultra')).toBeVisible();
    await expect(page.getByTestId('device-erase')).toBeDisabled();
    await page.getByTestId('device-typed').fill('SanDisk');
    await expect(page.getByTestId('device-erase')).toBeDisabled();
    await page.getByTestId('device-typed').fill('  sandisk ultra ');
    await expect(page.getByTestId('device-erase')).toBeEnabled();
    await expect(page.getByText('Il sistema chiederà i permessi solo per formattare.')).toBeVisible();
    await page.getByTestId('device-erase').click();

    await expect(page.getByTestId('device-steps')).toBeVisible();
    await expect(page.getByTestId('device-done')).toContainText('Il dispositivo è pronto.', { timeout: 20_000 });
    const vol = page.getByTestId('device-volume');
    await expect(vol).toContainText('/run/media/utente/VaultData/Dati.nvault');
    await vol.getByLabel('Password', { exact: true }).fill('una passphrase lunga e sicura 2026');
    await vol.getByLabel('Ripeti la password').fill('una passphrase lunga e sicura 2026');
    await page.getByTestId('device-volume-create').click();
    await expect(page.getByTestId('device-volume-made')).toBeVisible();
    const fixed = await page.evaluate(() => (window as unknown as { __nv: { mock: { lastCreate?: Record<string, unknown> } } }).__nv.mock.lastCreate?.fixed_size);
    expect(Number(fixed)).toBeGreaterThan(29e9);
    await page.getByTestId('device-finish').click();
    await expectScreen(page, 'vault');
  });

  test('from Impostazioni; an unnamed drive takes CANCELLA; a refused prompt is reported', async ({ page }) => {
    await boot(page, 'screen=settings&device_fail=elevation_denied');
    await page.getByTestId('settings-open-device').click();
    await expectScreen(page, 'device');
    await page.getByTestId('device-disk').filter({ hasText: 'Unità senza nome' }).click();
    await page.getByTestId('device-next').click();
    await page.getByTestId('device-next').click();
    await page.getByTestId('device-continue').click();
    await expect(page.getByText('Per confermare scrivi: CANCELLA')).toBeVisible();
    await page.getByTestId('device-typed').fill('cancella');
    await page.getByTestId('device-erase').click();
    const e = page.getByTestId('device-error');
    await expect(e).toHaveAttribute('data-code', 'elevation_denied');
    await expect(e).toContainText("l'unità non è stata toccata");
    await expect(page.getByTestId('device-copy-log')).toBeVisible();
    await page.getByTestId('device-back-to-disks').click();
    await expect(page.getByTestId('device-disk')).toHaveCount(2);
  });

  test('an incomplete device offers Completa (no reformat)', async ({ page }) => {
    await boot(page, 'screen=device&device=incomplete');
    const sd = page.getByTestId('device-disk').filter({ hasText: 'Unità senza nome' });
    await expect(sd).toContainText('Incompleto');
    await page.getByTestId('device-complete').click();
    const steps = page.getByTestId('device-steps');
    await expect(steps.locator('[data-step="partition"]')).toHaveCount(0);
    await expect(page.getByTestId('device-done')).toBeVisible({ timeout: 20_000 });
  });

  test('made offline: the missing programs, then Scarica ora adds them once online', async ({ page }) => {
    await boot(page, 'screen=device&device=incomplete&device_offline');
    await page.getByTestId('device-complete').click();
    await expect(page.getByTestId('device-done')).toBeVisible({ timeout: 20_000 });
    await expect(page.getByTestId('device-missing')).toContainText('Scarica ora');
    const fetchNow = page.getByTestId('device-fetch-missing');
    await fetchNow.click();
    await expect(page.getByTestId('device-fetch-error')).toContainText('controlla la connessione');
    await expect(page.getByTestId('device-missing')).toBeVisible();
    await fetchNow.click();
    await expect(page.getByTestId('device-fetch-done')).toContainText('il dispositivo è completo');
    await expect(page.getByTestId('device-missing')).toHaveCount(0);
  });

  test('no drive plugged in: the empty state', async ({ page }) => {
    await boot(page, 'screen=device&device=empty');
    await expect(page.getByTestId('device-empty')).toContainText('Collega una chiavetta o un disco esterno.');
  });

  test('every step has a URL switch', async ({ page }) => {
    for (const [step, probe] of [
      ['options', 'device-fs'],
      ['confirm', 'device-summary'],
      ['type', 'device-typed'],
      ['progress', 'device-steps'],
      ['done', 'device-done'],
      ['error', 'device-error'],
    ] as const) {
      await boot(page, `screen=device&device_step=${step}`);
      await expect(page.getByTestId(probe)).toBeVisible();
    }
    await boot(page, 'screen=device&device_step=done&device_offline&lang=en');
    await expect(page.getByTestId('device-missing')).toContainText('Windows or macOS');
  });

  test('every wait is shown: the pressed key works, the status line counts, then offers Annulla / Riprova', async ({ page }) => {
    await boot(page, 'screen=device&device_slow=options&device_stuck_ms=2500');
    const next = page.getByTestId('device-next');
    await expect(next).toHaveAttribute('aria-busy', 'true');
    await expect(next).toContainText("Controllo l'unità…");
    await expect(next).toBeDisabled();
    const waiting = page.getByTestId('device-waiting');
    await expect(waiting).toContainText("Controllo l'unità…");
    await expect(page.getByTestId('device-waiting-hint')).toContainText('sta lavorando', { timeout: 5000 });
    await expect(page.getByTestId('device-waiting-secs')).toBeVisible();
    await expect(page.getByTestId('device-waiting-slow')).toContainText("l'unità non è stata toccata", { timeout: 5000 });
    await page.getByTestId('device-waiting-retry').click();
    await expect(next).toHaveAttribute('aria-busy', 'true');
    await expect(page.getByTestId('device-waiting-slow')).toHaveCount(0);
    await expect(page.getByTestId('device-waiting-cancel')).toBeVisible({ timeout: 5000 });
    await page.getByTestId('device-waiting-cancel').click();
    await expect(page.getByTestId('device-waiting')).toHaveCount(0);
    await expect(page.getByTestId('device-wizard')).toHaveAttribute('data-stage', 'disk');
    await expect(next).toBeEnabled();
    await expect(next).toHaveText('Avanti');
  });

  test('Indietro and Esc work while a step is pending', async ({ page }) => {
    await boot(page, 'screen=device&device_step=options&device_slow=plan');
    const next = page.getByTestId('device-next');
    await expect(next).toContainText('Preparo il piano…');
    const back = page.getByTestId('device-back');
    await expect(back).toBeEnabled();
    await back.click();
    await expect(page.getByTestId('device-wizard')).toHaveAttribute('data-stage', 'disk');
    await expect(page.getByTestId('device-wizard')).not.toHaveAttribute('data-pending', /.+/);
    await page.getByTestId('device-disk').first().click();
    await page.getByTestId('device-next').click();
    await expect(page.getByTestId('device-fs')).toBeVisible();
    await page.getByTestId('device-next').click();
    await expect(page.getByTestId('device-wizard')).toHaveAttribute('data-pending', 'plan');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('device-wizard')).toHaveAttribute('data-stage', 'disk');
  });

  test('the erase shows its progress at once and says where to authorise', async ({ page }) => {
    await boot(page, 'screen=device&device_step=type&device_slow=create');
    await expect(page.getByTestId('device-wizard')).toHaveAttribute('data-stage', 'progress');
    await expect(page.getByTestId('device-steps')).toBeVisible();
    await expect(page.getByTestId('device-waiting')).toContainText('Avvio la creazione…');
    await boot(page, 'screen=device&device_step=progress&device_slow=auth');
    await expect(page.getByTestId('device-auth')).toContainText('Autorizza nella finestra di sistema', { timeout: 10_000 });
    await expect(page.getByTestId('device-waiting')).toContainText("In attesa dell'autorizzazione…");
    await expect(page.getByTestId('device-steps').locator('[data-step="unmount"]')).toHaveAttribute('data-state', 'current');
  });

  test('a drive holding NiceVault volumes names them and needs one more consent', async ({ page }) => {
    await boot(page, 'screen=device&device_step=confirm&device_volumes');
    const w = page.getByTestId('device-volumes');
    await expect(w).toContainText('Su questa unità ci sono volumi NiceVault');
    await expect(w).toContainText('SH2RE.nvault');
    await expect(w).toContainText('Archivio.nvault');
    await expect(w).toContainText('verranno cancellati per sempre');
    await page.getByTestId('device-continue').click();
    await page.getByTestId('device-typed').fill('SanDisk Ultra');
    const erase = page.getByTestId('device-erase');
    await expect(erase).toBeDisabled();
    await expect(page.getByTestId('device-need-ack')).toBeVisible();
    await page.getByTestId('device-volumes-ack').check();
    await expect(page.getByTestId('device-need-ack')).toHaveCount(0);
    await expect(erase).toBeEnabled();
    await page.getByTestId('device-volumes-ack').uncheck();
    await expect(erase).toBeDisabled();
    await boot(page, 'screen=device&device_step=type');
    await expect(page.getByTestId('device-volumes')).toHaveCount(0);
    await page.getByTestId('device-typed').fill('SanDisk Ultra');
    await expect(page.getByTestId('device-erase')).toBeEnabled();
  });

  for (const [w, h, scale] of [
    [900, 600, 150],
    [1680, 1050, 100],
  ] as const) {
    test(`full-window sheet at ${w}x${h} @${scale}%: no page scroll, footer on screen`, async ({ page }) => {
      await page.setViewportSize({ width: w, height: h });
      await boot(page, `screen=device&device_step=type&device_volumes&scale=${scale}`);
      await expect(page.getByTestId('device-erase')).toBeVisible();
      const sheet = page.locator('[data-testid=device-wizard] .form-sheet');
      const box = (await sheet.boundingBox())!;
      expect(box.width).toBeGreaterThan(w * 0.9);
      expect(box.height).toBeGreaterThan(h * 0.9);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      await expect(page.getByTestId('device-erase')).toBeInViewport();
      await expect(page.getByTestId('device-back')).toBeInViewport();
    });
  }
});
