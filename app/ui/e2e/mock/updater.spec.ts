// The updater: full controls in Impostazioni (settings-updates), a compact notice on
// Benvenuto and in the sidebar only while an update is pending.
import { boot, expect, expectScreen, test } from './helpers';

test('no update pending: no notice; Impostazioni checks, installs and offers Riavvia', async ({ page }) => {
  await boot(page, 'screen=welcome');
  await expect(page.getByTestId('update-notice')).toHaveCount(0);
  await page.getByTestId('open-settings').click();
  await expectScreen(page, 'settings');
  const sec = page.getByTestId('settings-updates');
  await expect(sec.getByTestId('update-on-start')).toHaveAttribute('aria-checked', 'true');
  await sec.getByTestId('check-updates').click();
  await expect(sec.getByTestId('update-status')).toHaveAttribute('data-phase', 'available');
  await expect(sec.getByTestId('update-status')).toContainText('0.2.0');
  await sec.getByTestId('install-update').click();
  await expect(sec.getByTestId('restart-update')).toBeVisible({ timeout: 10_000 });
  await sec.getByTestId('update-on-start').click();
  await expect(sec.getByTestId('update-on-start')).toHaveAttribute('aria-checked', 'false');
});

test('?update=available: compact notice on Benvenuto, Dettagli opens Impostazioni', async ({ page }) => {
  await boot(page, 'screen=welcome&update=available');
  const notice = page.getByTestId('update-notice');
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('0.2.0');
  await notice.getByRole('button').last().click();
  await expectScreen(page, 'settings');
  await expect(page.getByTestId('update-status')).toHaveAttribute('data-phase', 'available');
});

test('?update=downloads: read-only location saves the update and shows where', async ({ page }) => {
  await boot(page, 'screen=settings&update=downloads');
  const sec = page.getByTestId('settings-updates');
  await expect(sec.getByTestId('install-update')).toBeVisible();
  await sec.getByTestId('install-update').click();
  await expect(sec.getByTestId('update-status')).toHaveAttribute('data-phase', 'saved', { timeout: 10_000 });
  await expect(sec.getByTestId('update-status')).toContainText('Scaricati');
});
