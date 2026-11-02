import { boot, expect, expectScreen, test, type Page } from './helpers';

const lastFixed = (page: Page) =>
  page.evaluate(() => (window as unknown as { __nv: { mock: { lastCreate?: Record<string, unknown> } } }).__nv.mock.lastCreate?.fixed_size ?? null);

async function createWith(page: Page, name: string) {
  await page.getByRole('button', { name: 'Avanti' }).click();
  const strong = 'cavallo batteria graffetta corretta';
  await page.getByLabel('Password', { exact: true }).fill(strong);
  await page.getByLabel('Ripeti la password').fill(strong);
  await page.getByRole('button', { name: 'Crea volume' }).click();
  await expect(page.locator('[data-step]')).toHaveAttribute('data-step', '3');
  await expect(page.getByText(`/run/media/utente/CHIAVETTA/${name}.nvault`)).toBeVisible();
}

test('fixed size: free space shown and enforced, created at that size, resizable in Dettagli', async ({ page }) => {
  await boot(page, 'screen=welcome');
  await page.getByRole('button', { name: 'Nuovo volume' }).click();
  await expectScreen(page, 'create');
  await page.getByLabel('Nome').fill('Fisso');

  const mode = page.getByRole('radiogroup', { name: 'Dimensione', exact: true });
  await expect(mode.getByRole('radio', { name: 'A crescita' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.getByText('Il file cresce man mano che lo riempi.')).toBeVisible();
  await expect(page.getByTestId('create-size-value')).toHaveCount(0);

  await mode.getByRole('radio', { name: 'Dimensione fissa' }).click();
  await expect(page.getByText(/da fuori non si capisce quanto è pieno/)).toBeVisible();
  const size = page.getByRole('spinbutton', { name: 'Dimensione del volume' });
  const free = page.getByTestId('create-size-free');
  const next = page.getByRole('button', { name: 'Avanti' });
  await expect(size).toHaveValue('1');
  await expect(page.getByTestId('create-size-unit').getByRole('radio', { name: 'GB' })).toHaveAttribute('aria-checked', 'true');
  await expect(free).toContainText('22 GB liberi su CHIAVETTA');

  await size.fill('30');
  await expect(free).toHaveText('Su CHIAVETTA ci sono solo 22 GB liberi.');
  await expect(size).toHaveAttribute('aria-invalid', 'true');
  await expect(next).toBeDisabled();
  await page.getByTestId('drive').filter({ hasText: 'Questo computer' }).click();
  await expect(free).toContainText('188 GB liberi su Questo computer');
  await expect(next).toBeEnabled();
  await page.getByTestId('drive').filter({ hasText: 'CHIAVETTA' }).click();
  await expect(next).toBeDisabled();

  await size.fill('1');
  await page.getByTestId('create-size-unit').getByRole('radio', { name: 'MB' }).click();
  await expect(size).toHaveValue('1024');
  await size.fill('1');
  await expect(free).toHaveText('Serve almeno 4 MB.');
  await size.fill('2048');
  await page.getByTestId('create-size-unit').getByRole('radio', { name: 'GB' }).click();
  await expect(size).toHaveValue('2');
  await expect(next).toBeEnabled();

  await createWith(page, 'Fisso');
  await expect(page.getByTestId('create-done-size')).toHaveText('Dimensione fissa: 2 GB');
  expect(await lastFixed(page)).toBe(2 * 1024 ** 3);

  await page.getByRole('button', { name: 'Apri il volume' }).click();
  await expectScreen(page, 'vault');
  await page.getByRole('button', { name: 'Dettagli' }).click();
  await page.getByTestId('resize-open').click();
  const dlg = page.getByRole('dialog', { name: 'Dimensione del volume' });
  await expect(dlg.getByTestId('resize-capacity')).toHaveText('2 GB');
  await dlg.getByTestId('resize-size').fill('3');
  await dlg.getByRole('button', { name: 'Estendi' }).click();
  await expect(dlg.getByTestId('resize-done')).toHaveText('Fatto: il volume ora è di 3 GB.');
});

test('growable stays the default: no fixed_size sent', async ({ page }) => {
  await boot(page, 'screen=welcome');
  await page.getByRole('button', { name: 'Nuovo volume' }).click();
  await page.getByLabel('Nome').fill('Cresce');
  await page.getByRole('radio', { name: 'Dimensione fissa' }).click();
  await page.getByRole('radio', { name: 'A crescita' }).click();
  await createWith(page, 'Cresce');
  await expect(page.getByTestId('create-done-size')).toHaveCount(0);
  expect(await lastFixed(page)).toBeNull();
});

test('English', async ({ page }) => {
  await boot(page, 'screen=create&lang=en');
  await page.getByRole('radio', { name: 'Fixed size' }).click();
  await expect(page.getByText(/nobody can tell how full it is/)).toBeVisible();
  await expect(page.getByTestId('create-size-free')).toContainText('free on CHIAVETTA');
});
