import type { Page } from '@playwright/test';
import { boot, expect, expectScreen, listed, test } from './helpers';

const drive = (page: Page, action: 'add' | 'remove', d?: unknown) =>
  page.evaluate(
    ([a, x]) => (window as unknown as { __nv: { mock: { simulateDrive(a: string, d?: unknown): unknown } } }).__nv.mock.simulateDrive(a, x),
    [action, d] as const,
  );

const passport = (page: Page, name: string) => page.getByTestId('passport').and(page.locator(`[data-name="${name}"]`));

test.describe('live drives', () => {
  test('Benvenuto shows the volumes of a drive plugged in, and drops them when it is pulled out', async ({ page }) => {
    await boot(page, 'screen=welcome');
    await expect(passport(page, 'Documenti')).toBeVisible();
    await expect(passport(page, 'Fotografie')).toHaveCount(0);

    await drive(page, 'add');
    await expect(passport(page, 'Fotografie')).toBeVisible();
    await expect(passport(page, 'Fotografie')).toContainText('Su un’unità esterna');

    await drive(page, 'remove', '/dev/sdc1');
    await expect(passport(page, 'Fotografie')).toHaveCount(0);
    await expect(passport(page, 'Documenti')).toBeVisible();
  });

  test('an open volume on a pulled drive locks (reason removed)', async ({ page }) => {
    await boot(page, 'screen=welcome');
    await drive(page, 'add');
    await passport(page, 'Fotografie').getByRole('button', { name: /Fotografie/ }).first().click();
    await expectScreen(page, 'unlock');
    await page.getByLabel('Password', { exact: true }).fill('demo');
    await page.keyboard.press('Enter');
    await expectScreen(page, 'vault');
    await listed(page);

    await drive(page, 'remove', '/dev/sdc1');
    await expectScreen(page, 'unlock');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Fotografie');
  });

  test('Nuovo volume: drive cards, live hot-plug, other folder', async ({ page }) => {
    await boot(page, 'screen=create');
    const picker = page.getByRole('radiogroup', { name: 'Posizione' });
    const card = (name: string) => picker.getByRole('radio').filter({ has: page.getByTestId('drive-name').getByText(name, { exact: true }) });
    await expect(card('CHIAVETTA')).toHaveAttribute('aria-checked', 'true');
    await expect(card('CHIAVETTA')).toContainText('NiceVault è qui');
    await expect(card('CHIAVETTA')).toContainText('Rimovibile');
    await expect(card('CHIAVETTA')).toContainText('liberi su');
    await expect(card('CHIAVETTA')).toContainText('exfat');
    await expect(page.getByText('/run/media/utente/CHIAVETTA/Nuovo volume.nvault')).toBeVisible();

    await card('Questo computer').click();
    await expect(card('Questo computer')).toHaveAttribute('aria-checked', 'true');
    await expect(card('Questo computer')).toContainText('Interno');
    await expect(page.getByText('/home/utente/Documenti/Nuovo volume.nvault')).toBeVisible();

    await card('Questo computer').focus();
    await page.keyboard.press('ArrowLeft');
    await expect(card('CHIAVETTA')).toHaveAttribute('aria-checked', 'true');
    await expect(card('CHIAVETTA')).toBeFocused();

    await drive(page, 'add');
    await card('BACKUP').click();
    await expect(page.getByText('/run/media/utente/BACKUP/Nuovo volume.nvault')).toBeVisible();

    await drive(page, 'remove', '/dev/sdc1');
    await expect(card('BACKUP')).toHaveCount(0);
    await expect(page.getByTestId('drive-gone')).toContainText('BACKUP');
    await expect(card('CHIAVETTA')).toHaveAttribute('aria-checked', 'true');

    await picker.getByTestId('drive-other').click();
    await expect(picker.getByTestId('drive-other')).toHaveAttribute('aria-checked', 'true');
    await expect(picker.getByTestId('drive-other')).toContainText('/home/utente/Scrivania');
    await expect(page.getByText('/home/utente/Scrivania/Nuovo volume.nvault')).toBeVisible();
    await expect(page.getByTestId('drive-gone')).toHaveCount(0);

    await drive(page, 'add');
    await card('BACKUP').click();
    await page.getByRole('button', { name: 'Avanti' }).click();
    await page.getByLabel('Password', { exact: true }).fill('una password lunga e sicura 2026');
    await page.getByLabel('Ripeti la password').fill('una password lunga e sicura 2026');
    await page.getByRole('button', { name: 'Crea volume' }).click();
    await expect(page.getByText('/run/media/utente/BACKUP/Nuovo volume.nvault')).toBeVisible();
    await expect(page.getByText('Volume creato.')).toBeVisible();
  });

  test('English', async ({ page }) => {
    await boot(page, 'screen=create&lang=en');
    const picker = page.getByRole('radiogroup', { name: 'Location' });
    await expect(picker.getByRole('radio').first()).toContainText('NiceVault is here');
    await expect(picker).toContainText('This computer');
    await expect(picker).toContainText('free of');
    await expect(picker.getByTestId('drive-other')).toContainText('Other folder…');
  });
});
