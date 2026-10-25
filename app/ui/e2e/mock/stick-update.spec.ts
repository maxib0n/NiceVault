// "Aggiorna i programmi sulla chiavetta" (stickMock.ts): Dettagli > Volume card, the notices on
// Benvenuto and on the cover, offline, not mounted, a failure.
import { boot, expect, test } from './helpers';

test('details: the stick card lists each OS, updates and shows the results', async ({ page }) => {
  await boot(page, 'screen=details&stick=missing');
  const card = page.getByTestId('stick-programs');
  await expect(card).toHaveAttribute('data-state', 'outdated');
  await expect(card).toContainText('SanDisk Ultra');
  await expect(card.locator('[data-os="linux"]')).toContainText('0.0.5');
  await expect(card.locator('[data-os="linux"]')).toContainText('0.1.0');
  await expect(card.locator('[data-os="macos"]')).toContainText('manca');
  await expect(card).toContainText('i volumi e VaultData non vengono toccati');
  // the cover offers it too
  await expect(page.getByTestId('stick-notice')).toBeVisible();
  await card.getByTestId('stick-update').click();
  await expect(card).toHaveAttribute('data-state', 'running');
  await expect(card).toHaveAttribute('data-state', 'done', { timeout: 15_000 });
  await expect(card).toContainText('Programmi aggiornati.');
  for (const os of ['linux', 'windows', 'macos']) await expect(card.locator(`[data-os="${os}"]`)).toHaveAttribute('data-result', 'updated');
  await expect(card.locator('[data-os="windows"]')).toContainText('0.2.0');
  await expect(page.getByTestId('stick-notice')).toHaveAttribute('data-phase', 'done');
});

test('offline: only this OS is updated, the others wait for the network', async ({ page }) => {
  await boot(page, 'screen=details&stick=old&stick_offline');
  const card = page.getByTestId('stick-programs');
  await card.getByTestId('stick-update').click();
  await expect(card).toHaveAttribute('data-state', 'done', { timeout: 15_000 });
  await expect(card).toContainText('gli altri quando sarai in rete');
  await expect(card.locator('[data-os="linux"]')).toHaveAttribute('data-result', 'updated');
  await expect(card.locator('[data-os="windows"]')).toHaveAttribute('data-result', 'offline');
  await expect(card.locator('[data-os="windows"]')).toContainText('da scaricare quando online');
});

test('not mounted: says so, mounts, then offers the update', async ({ page }) => {
  await boot(page, 'screen=details&stick=unmounted');
  const card = page.getByTestId('stick-programs');
  await expect(card).toHaveAttribute('data-state', 'unmounted');
  await expect(card).toContainText('non è montata');
  await expect(page.getByTestId('stick-notice')).toHaveCount(0);
  await card.getByTestId('stick-mount').click();
  await expect(card).toHaveAttribute('data-state', 'outdated');
  await expect(card.getByTestId('stick-update')).toBeEnabled();
});

test('a failed update keeps the old programs and says why', async ({ page }) => {
  await boot(page, 'screen=details&stick=old&stick_fail=stick_full');
  const card = page.getByTestId('stick-programs');
  await card.getByTestId('stick-update').click();
  await expect(card.getByRole('alert')).toContainText('Non c\'è abbastanza spazio');
  await expect(card.getByRole('alert')).toContainText('sono rimasti al loro posto');
});

test('up to date or not a device: no notice', async ({ page }) => {
  await boot(page, 'screen=details&stick=current');
  await expect(page.getByTestId('stick-programs')).toHaveAttribute('data-state', 'current');
  await expect(page.getByTestId('stick-notice')).toHaveCount(0);
  await boot(page, 'screen=details');
  await expect(page.getByTestId('stick-programs')).toHaveCount(0);
  await expect(page.getByTestId('stick-notice')).toHaveCount(0);
});

test('welcome: the notice updates the stick, "Non ora" puts it away', async ({ page }) => {
  await boot(page, 'stick=old');
  const n = page.getByTestId('stick-notice');
  await expect(n).toContainText('Programmi della chiavetta da aggiornare');
  await n.getByRole('button', { name: 'Non ora' }).click();
  await expect(n).toHaveCount(0);
  await boot(page, 'stick=old');
  await page.getByTestId('stick-notice-update').click();
  await expect(page.getByTestId('stick-notice')).toContainText('Programmi aggiornati.', { timeout: 15_000 });
});

test('tight space (128 MiB): says so with the numbers, replaces one at a time, a failed rewrite is retried', async ({ page }) => {
  await boot(page, 'screen=details&stick=tight&stick_fail=rewrite');
  const card = page.getByTestId('stick-programs');
  const space = card.getByTestId('stick-space');
  await expect(space).toHaveAttribute('data-tight', 'true');
  await expect(space).toContainText('Spazio stretto: i programmi verranno sostituiti uno alla volta.');
  await expect(space).toContainText('Partizione NiceVault 128 MB');
  await expect(space).toContainText('avanzi tolti prima');
  await card.getByTestId('stick-update').click();
  await expect(card).toHaveAttribute('data-state', 'done', { timeout: 15_000 });
  await expect(card.locator('[data-os="macos"]')).toHaveAttribute('data-result', 'rewrite');
  await expect(card.locator('[data-os="macos"]')).toContainText('da riscrivere');
  await expect(card.locator('[data-os="windows"]')).toHaveAttribute('data-result', 'updated');
  // again: macOS is written in place of nothing, the others are current
  await card.getByTestId('stick-update').click();
  await expect(card).toHaveAttribute('data-state', 'done', { timeout: 15_000 });
  await expect(card.locator('[data-os="macos"]')).toHaveAttribute('data-result', 'updated');
  await expect(card).toContainText('Programmi aggiornati.');
});
