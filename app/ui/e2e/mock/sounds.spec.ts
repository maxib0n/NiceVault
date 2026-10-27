import type { Page } from '@playwright/test';
import { boot, expect, expectScreen, test } from './helpers';

const played = (page: Page) => page.evaluate(() => [...((window as unknown as { __nvSounds: string[] }).__nvSounds ?? [])]);

test('wrong password, unlock and copy play their sounds; the switch silences them', async ({ page }) => {
  test.slow();
  await boot(page, 'screen=welcome');
  await page.getByRole('button', { name: /Documenti/ }).first().click();
  await expectScreen(page, 'unlock');
  const pw = page.getByLabel('Password', { exact: true });
  await pw.fill('sbagliata');
  await page.keyboard.press('Enter');
  await expect.poll(() => played(page)).toEqual(['wrong']);
  await pw.fill('demo');
  await page.keyboard.press('Enter');
  await expectScreen(page, 'vault');
  await expect.poll(() => played(page)).toEqual(['wrong', 'unlock']);
});

test('copy ticks, TOTP ticks differently; off in Impostazioni means silence', async ({ page }) => {
  await boot(page, 'screen=passwords&entry=Posta personale');
  const d = page.getByTestId('pm-detail');
  await d.getByTestId('pm-copy-password').click();
  await d.getByTestId('pm-copy-totp').click();
  await expect.poll(() => played(page)).toEqual(['unlock', 'copy', 'totp']);
});

test('the Suoni switch is on by default and toggles', async ({ page }) => {
  await boot(page, 'screen=settings');
  const sw = page.getByTestId('sounds');
  await expect(sw).toHaveAttribute('aria-checked', 'true');
  await sw.click();
  await expect(sw).toHaveAttribute('aria-checked', 'false');
  await page.evaluate(() => ((window as unknown as { __nvSounds: string[] }).__nvSounds.length = 0));
  await page.getByTestId('settings-back').click();
  await expectScreen(page, 'welcome');
  await page.getByRole('button', { name: /Documenti/ }).first().click();
  await page.getByLabel('Password', { exact: true }).fill('sbagliata');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert').filter({ hasText: 'Password errata' })).toBeVisible();
  expect(await played(page)).toEqual([]);
});
