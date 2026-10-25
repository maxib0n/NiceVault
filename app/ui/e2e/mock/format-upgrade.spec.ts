// Dettagli > Volume: "Aggiorna formato" (single-file journal), opt-in with a confirm step.
import { boot, expect, test } from './helpers';

test('format upgrade: warning, cancel, confirm, then shown as single file', async ({ page }) => {
  await boot(page, 'screen=details');
  const box = page.getByTestId('format-upgrade');
  await expect(box).toContainText('Formato precedente');
  await box.getByTestId('format-upgrade-open').click();
  await expect(box.getByRole('alert')).toContainText('non potranno più aprire il volume');
  await box.getByRole('button', { name: 'Annulla' }).click();
  await expect(box.getByRole('alert')).toHaveCount(0);
  await box.getByTestId('format-upgrade-open').click();
  await box.getByTestId('format-confirm').click();
  await expect(box.getByTestId('format-single')).toBeVisible();
  await expect(page.getByText(/ora è un solo file/)).toBeVisible();
  await expect(box.getByTestId('format-upgrade-open')).toHaveCount(0);
});

test('format upgrade: an older fixed-size volume is upgraded the same way', async ({ page }) => {
  await boot(page, 'screen=details&fixed');
  const box = page.getByTestId('format-upgrade');
  await expect(box).toContainText('Formato precedente');
  await box.getByTestId('format-upgrade-open').click();
  await box.getByTestId('format-confirm').click();
  await expect(box.getByTestId('format-single')).toBeVisible();
});

test('format upgrade: a nearly full fixed-size volume asks to extend it first', async ({ page }) => {
  await boot(page, 'screen=details&fixed&fmt_full');
  const box = page.getByTestId('format-upgrade');
  await expect(box).toContainText('quasi pieno');
  await expect(box.getByTestId('format-upgrade-open')).toHaveCount(0);
});
