import { boot, expect, jobTray, test } from './helpers';

test('too big for a fixed volume: the dialog offers Estendi, prefilled to a size that fits', async ({ page }) => {
  await boot(page, 'screen=vault&fixed&import_pick=huge.bin');
  await page.getByRole('button', { name: 'Importa', exact: true }).click();
  const dlg = page.getByRole('dialog', { name: 'Non c’è abbastanza spazio' }).or(page.getByRole('dialog', { name: "Non c'è abbastanza spazio" }));
  await expect(dlg.getByTestId('space-body')).toContainText('servono 6 GB');
  await expect(dlg.getByTestId('space-body')).toContainText('liberi');
  await dlg.getByTestId('space-extend').click();
  const rsz = page.getByRole('dialog', { name: 'Dimensione del volume' });
  const size = rsz.getByTestId('resize-size');
  await expect(size).toBeVisible();
  expect(Number(await size.inputValue())).toBeGreaterThanOrEqual(8);
  await expect(rsz.getByTestId('resize-plan')).toHaveAttribute('data-plan', 'grow');
  await expect(jobTray(page)).toHaveCount(0);
});

test('too big for the drive of a growable volume: the message only', async ({ page }) => {
  await boot(page, 'screen=vault&import_pick=huge.bin,huge2.bin,huge3.bin,huge4.bin,huge5.bin,huge6.bin,huge7.bin');
  await page.getByRole('button', { name: 'Importa', exact: true }).click();
  const body = page.getByTestId('space-body');
  await expect(body).toContainText('Libera spazio sul disco');
  await expect(page.getByTestId('space-extend')).toHaveCount(0);
});

test('a slow import shows speed, then the time left once stable', async ({ page }) => {
  await boot(page, 'screen=vault&import_pick=slow.bin');
  await page.getByRole('button', { name: 'Importa', exact: true }).click();
  const tray = jobTray(page);
  await tray.getByRole('button').first().click();
  const pace = tray.getByTestId('job-pace');
  await expect(pace).toContainText('/s');
  await expect(pace).not.toContainText('~');
  await expect(pace).toContainText('~', { timeout: 6000 });
});
