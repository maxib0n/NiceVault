import { boot, expect, expectScreen, listed, test } from './helpers';
import type { Page } from '@playwright/test';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
test.describe.configure({ timeout: 60_000 });

async function lockAndPick(page: Page) {
  await page.evaluate(async () => {
    const nv = (window as unknown as { __nv: { mock: { invoke: (c: string, a: unknown) => Promise<unknown>; open: Map<string, unknown> } } }).__nv;
    for (const id of [...nv.mock.open.keys()]) await nv.mock.invoke('vault_lock', { vault: id, reason: 'user' });
  });
  await expectScreen(page, 'unlock');
  await expect(page.getByRole('heading', { level: 1, name: 'Documenti' })).toHaveCount(1);
}

async function turnOn(page: Page) {
  const bio = page.getByTestId('bio');
  await expect(bio).toContainText('Sblocca con l’impronta');
  await expect(page.getByTestId('bio-switch')).toHaveAttribute('aria-checked', 'false');
  await page.getByTestId('bio-switch').click();
  await expect(page.getByTestId('bio-switch')).toHaveAttribute('aria-checked', 'true');
  await expect(page.getByTestId('slots').locator('[data-kind="device"]')).toContainText('Pixel 9 Pro · impronta');
}

test('turn on in Dettagli, unlock with the fingerprint, the password still works', async ({ page }) => {
  await boot(page, 'screen=details&platform=mobile&slots=1');
  await turnOn(page);
  const slots = page.getByTestId('slots');
  await expect(slots.locator('[data-kind="password"]').getByRole('button', { name: 'Rimuovi' })).toBeDisabled();
  await expect(slots.locator('[data-kind="device"]').getByRole('button', { name: 'Rimuovi' })).toBeEnabled();

  await lockAndPick(page);
  await page.getByTestId('bio-unlock').click();
  await expectScreen(page, 'vault');
  await listed(page);

  await lockAndPick(page);
  await expect(page.getByTestId('bio-unlock')).toBeVisible();
  await page.getByLabel('Password', { exact: true }).fill('demo');
  await page.keyboard.press('Enter');
  await expectScreen(page, 'vault');
});

test('turned off: the device key leaves the volume; removed in the list: the switch goes off', async ({ page }) => {
  await boot(page, 'screen=details&platform=mobile&slots=1');
  await turnOn(page);
  await page.getByTestId('bio-switch').click();
  await expect(page.getByTestId('bio-switch')).toHaveAttribute('aria-checked', 'false');
  await expect(page.getByTestId('slots').locator('[data-kind="device"]')).toHaveCount(0);
  await turnOn(page);
  await page.getByTestId('slots').locator('[data-kind="device"]').getByRole('button', { name: 'Rimuovi' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Rimuovi' }).click();
  await expect(page.getByTestId('slots').locator('[data-kind="device"]')).toHaveCount(0);
  await expect(page.getByTestId('bio-switch')).toHaveAttribute('aria-checked', 'false');
});

test('changed fingerprints: an error, the key goes, the password opens', async ({ page }) => {
  await boot(page, 'screen=details&platform=mobile&slots=1');
  await turnOn(page);
  await page.evaluate(() => history.replaceState(null, '', location.pathname + location.search + '&bio=changed'));
  await lockAndPick(page);
  await page.getByTestId('bio-unlock').click();
  await expect(page.getByRole('alert')).toContainText('Le impronte o il volto sono cambiati');
  await expect(page.getByTestId('bio-unlock')).toHaveCount(0);
  await page.getByLabel('Password', { exact: true }).fill('demo');
  await page.keyboard.press('Enter');
  await expectScreen(page, 'vault');
});

test('a cancelled prompt is no error; Face ID words; nothing enrolled = switch off limits', async ({ page }) => {
  await boot(page, 'screen=details&platform=mobile&slots=1&bio=face');
  const sw = page.getByTestId('bio-switch');
  await expect(page.getByTestId('bio')).toContainText('Sblocca con Face ID');
  await sw.click();
  await expect(page.getByTestId('slots').locator('[data-kind="device"]')).toContainText('iPhone · Face ID');
  await page.evaluate(() => history.replaceState(null, '', location.pathname + location.search.replace('bio=face', 'bio=cancel')));
  await lockAndPick(page);
  await page.getByTestId('bio-unlock').click();
  await expect(page.locator('[data-phase]')).toHaveAttribute('data-phase', 'idle');
  await expect(page.getByRole('alert')).toHaveCount(0);

  await page.goto('/?screen=details&platform=mobile&slots=1&bio=none');
  await expect(page.getByTestId('bio-switch')).toBeDisabled();
  await expect(page.getByTestId('bio')).toContainText('Registra prima un’impronta');
});

test('desktop: no switch, no key', async ({ page }) => {
  await boot(page, 'screen=details&platform=desktop');
  await expect(page.getByTestId('slots')).toBeVisible();
  await expect(page.getByTestId('bio')).toHaveCount(0);
});
