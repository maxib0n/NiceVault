import { boot, expect, expectScreen, test } from './helpers';

test('Impostazioni opens from Benvenuto, the sidebar and Ctrl+, ; back returns', async ({ page }) => {
  await boot(page, 'screen=welcome');
  await page.getByTestId('open-settings').click();
  await expectScreen(page, 'settings');
  await page.getByTestId('settings-back').click();
  await expectScreen(page, 'welcome');

  await boot(page, 'screen=vault');
  await page.getByRole('complementary').getByTestId('open-settings').click();
  await expectScreen(page, 'settings');
  await page.keyboard.press('Escape');
  await expectScreen(page, 'vault');
  await page.keyboard.press('Control+,');
  await expectScreen(page, 'settings');
  await expect(page.getByTestId('app-version')).toHaveText('0.1.0');
  await expect(page.getByTestId('settings-storage')).toContainText('niente viene scritto');
  await expect(page.getByTestId('check-updates')).toBeEnabled();
});

test('Impostazioni is a page of its own: the whole window, one column, mouse back and Alt+← return', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 640 });
  await boot(page, 'screen=welcome');
  await page.getByTestId('open-settings').click();
  await expectScreen(page, 'settings');
  const box = await page.getByTestId('settings').boundingBox();
  expect(box?.width).toBe(1280);
  expect(box?.height).toBe(640);
  await expect(page.getByRole('navigation', { name: 'Sezioni' })).toHaveCount(0);
  const app = await page.getByTestId('settings-app').boundingBox();
  const about = await page.getByTestId('settings-about').boundingBox();
  expect(app && about && Math.abs(app.x - about.x)).toBeLessThan(2);
  expect(app && Math.abs(app.x + app.width / 2 - 640)).toBeLessThan(40);
  await page.getByTestId('settings-about').scrollIntoViewIfNeeded();
  await expect(page.getByTestId('settings-about')).toBeInViewport();
  await page.evaluate(() => window.dispatchEvent(new MouseEvent('mouseup', { button: 3, bubbles: true })));
  await expectScreen(page, 'welcome');
  await page.getByTestId('open-settings').click();
  await expectScreen(page, 'settings');
  await page.keyboard.press('Alt+ArrowLeft');
  await expectScreen(page, 'welcome');
});

test('language: it -> en -> it switches every label in place, no reload', async ({ page }) => {
  await boot(page, 'screen=settings');
  await expect(page.locator('html')).toHaveAttribute('lang', 'it');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Impostazioni');
  await page.locator('[data-lang="en"]').click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Settings');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(page.locator('[data-lang="en"]')).toHaveAttribute('aria-checked', 'true');
  await page.getByTestId('settings-back').click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(/Your\s+volumes/);
  await page.getByTestId('open-settings').click();
  await page.locator('[data-lang="it"]').click();
  await page.getByTestId('settings-back').click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(/I tuoi\s+volumi/);
});

test('?lang=en: the volume, its files and its details in English', async ({ page }) => {
  await boot(page, 'screen=vault&lang=en&view=list');
  await expect(page.getByTestId('nav-files')).toHaveText('Files');
  await expect(page.getByRole('button', { name: 'Import', exact: true })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Path' })).toBeVisible();
  await page.getByTestId('nav-details').click();
  await expect(page.getByRole('heading', { name: 'Auto-lock' })).toBeVisible();
  await expect(page.getByTestId('autolock-minutes').getByRole('radio', { name: 'Never' })).toBeVisible();
});

test('defaults for new volumes: a volume created afterwards starts with them', async ({ page }) => {
  await boot(page, 'screen=settings');
  await page.getByTestId('new-like-app').click();
  await page.getByTestId('default-accent').locator('[data-accent="prussia"]').click();
  await page.getByTestId('default-minutes').getByRole('radio', { name: 'Mai' }).click();
  await page.getByTestId('settings-back').click();
  await page.getByRole('button', { name: 'Nuovo volume' }).click();
  await page.getByLabel('Nome').fill('Prussia');
  await page.getByRole('button', { name: 'Avanti' }).click();
  await page.getByLabel('Password', { exact: true }).fill('cavallo batteria graffetta corretta');
  await page.getByLabel('Ripeti la password').fill('cavallo batteria graffetta corretta');
  await page.getByRole('button', { name: 'Crea volume' }).click();
  await page.getByRole('button', { name: 'Apri il volume' }).click();
  await expectScreen(page, 'vault');
  await expect(page.locator('html')).toHaveAttribute('data-accent', 'prussia');
  await expect(page.getByText(/Blocco automatico tra/)).toHaveCount(0);
});

test('mounted drives are listed and can be unmounted from Impostazioni', async ({ page }) => {
  await boot(page, 'screen=vault&mount=mounted');
  await page.keyboard.press('Control+,');
  const drives = page.getByTestId('settings-drives');
  await expect(drives.getByRole('listitem')).toHaveCount(1);
  await drives.getByRole('button', { name: /^Smonta/ }).click();
  await expect(drives.getByRole('listitem')).toHaveCount(0);
});

test('licences: SOURCES.md is shown', async ({ page }) => {
  await boot(page, 'screen=settings');
  await page.getByTestId('open-licences').click();
  await expect(page.getByTestId('licences-text')).toContainText('ascii-reveal');
});
