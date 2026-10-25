// Dettagli > Estendi / Riduci (ResizeDialog): size facts, what a size will do before asking,
// progress, done and error states; growable volumes only show their size.
import { boot, expect, test } from './helpers';

test('fixed volume: extend, then shrink with compaction, stats follow', async ({ page }) => {
  await boot(page, 'screen=details&fixed');
  await page.getByTestId('resize-open').click();
  const dlg = page.getByRole('dialog', { name: 'Dimensione del volume' });
  await expect(dlg.getByTestId('resize-capacity')).toHaveText('4 GB');
  const size = dlg.getByTestId('resize-size');
  const plan = dlg.getByTestId('resize-plan');
  await size.fill('6');
  await expect(plan).toHaveAttribute('data-plan', 'grow');
  await expect(plan).toContainText('servono 2 GB liberi sul disco');
  await dlg.getByRole('button', { name: 'Estendi' }).click();
  await expect(dlg.getByTestId('resize-progress')).toBeVisible();
  await expect(dlg.getByTestId('resize-done')).toHaveText('Fatto: il volume ora è di 6 GB.');
  await dlg.getByRole('button', { name: 'Chiudi' }).click();
  await expect(dlg).toHaveCount(0);
  await expect(page.getByText('6 GB', { exact: true }).first()).toBeVisible();

  // too small: refused before asking
  await page.getByTestId('resize-open').click();
  await size.fill('0.001');
  await expect(plan).toHaveAttribute('data-plan', 'too_small');
  await expect(dlg.getByTestId('resize-submit')).toBeDisabled();
  // the smallest size needs compaction (free pages in the tail of the store)
  await dlg.getByRole('button', { name: 'Al minimo' }).click();
  await expect(plan).toHaveAttribute('data-plan', 'compact');
  await expect(plan).toContainText('compattati');
  await dlg.getByRole('button', { name: 'Riduci' }).click();
  await expect(dlg.getByTestId('resize-progress').locator('[data-phase="compact"]')).toBeVisible();
  await expect(dlg.getByTestId('resize-progress').getByRole('progressbar')).toBeVisible();
  await expect(dlg.getByTestId('resize-done')).toContainText('Fatto: il volume ora è di');
  await dlg.getByRole('button', { name: 'Chiudi' }).click();
});

test('growable volume: size facts only', async ({ page }) => {
  await boot(page, 'screen=details&dialog=resize');
  const dlg = page.getByRole('dialog', { name: 'Dimensione del volume' });
  await expect(dlg.getByTestId('resize-growable')).toContainText('cresce da solo');
  await expect(dlg.getByTestId('resize-submit')).toHaveCount(0);
  await expect(dlg.getByTestId('resize-size')).toHaveCount(0);
  await dlg.getByRole('button', { name: 'Chiudi' }).click();
  await expect(dlg).toHaveCount(0);
});

test('a failed resize says the volume is unchanged and can be retried', async ({ page }) => {
  await boot(page, 'screen=details&fixed&resize_fail=io&dialog=resize');
  const dlg = page.getByRole('dialog', { name: 'Dimensione del volume' });
  await dlg.getByTestId('resize-size').fill('5');
  await dlg.getByRole('button', { name: 'Estendi' }).click();
  const error = dlg.getByTestId('resize-error');
  await expect(error).toContainText('Non è stato possibile cambiare la dimensione.');
  await expect(error).toContainText('Il volume e i suoi dati sono come prima.');
  await dlg.getByRole('button', { name: 'Riprova' }).click();
  await expect(dlg.getByTestId('resize-capacity')).toHaveText('4 GB');
  await expect(dlg.getByTestId('resize-size')).toBeVisible();
});

test('English', async ({ page }) => {
  await boot(page, 'screen=details&fixed&dialog=resize&lang=en');
  const dlg = page.getByRole('dialog', { name: 'Volume size' });
  await dlg.getByTestId('resize-size').fill('99999');
  await expect(dlg.getByTestId('resize-plan')).toContainText('Too large');
});
