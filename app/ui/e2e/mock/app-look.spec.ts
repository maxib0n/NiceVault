// The app's own look (Impostazioni > Aspetto dell'app): worn live by Benvenuto, Impostazioni and
// Sblocca of an encrypted look; a volume's visible look keeps its ink. The interface size.
import { boot, expect, expectScreen, test } from './helpers';

test('app theme and ink apply live; a volume with its own look keeps it', async ({ page }) => {
  test.slow(); // four screens in a row (WebKit on a loaded machine)
  await boot(page, 'screen=welcome');
  const html = page.locator('html');
  await expect(html).toHaveAttribute('data-theme', 'dark');
  await page.getByTestId('open-settings').click();
  await expectScreen(page, 'settings');
  await page.getByTestId('app-theme').locator('[data-theme-id="light"]').click();
  await expect(html).toHaveAttribute('data-theme', 'light');
  await page.getByTestId('app-accent').locator('[data-accent="ottanio"]').click();
  await expect(html).toHaveAttribute('data-accent', 'ottanio');
  await page.getByTestId('settings-back').click();
  await expectScreen(page, 'welcome');
  await expect(html).toHaveAttribute('data-theme', 'light');
  await expect(html).toHaveAttribute('data-accent', 'ottanio');
  // the passports keep their own inks
  await expect(page.locator('.passport[data-name="Archivio fiscale"]')).toHaveAttribute('data-accent', 'prussia');

  // Sblocca of a visible look: its ink (the app's theme); of an encrypted look: the app's
  await page.locator('.passport[data-name="Archivio fiscale"] .book').click();
  await expectScreen(page, 'unlock');
  await expect(html).toHaveAttribute('data-accent', 'prussia');
  await expect(html).toHaveAttribute('data-theme', 'light');
  await page.locator('.unlock .back').click();
  await expectScreen(page, 'welcome');
  await page.locator('.passport[data-name="Diario"] .book').click();
  await expectScreen(page, 'unlock');
  await expect(html).toHaveAttribute('data-accent', 'ottanio');
});

test('an open volume wears its own look over the app look', async ({ page }) => {
  await boot(page, 'screen=vault&app_theme=light&app_accent=ottanio');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.locator('html')).not.toHaveAttribute('data-accent', 'ottanio');
});

test('interface size: buttons with their value, Ctrl+Shift+= / - / 0', async ({ page }) => {
  await boot(page, 'screen=settings');
  const html = page.locator('html');
  const scale = page.getByTestId('app-scale');
  await expect(scale.getByRole('radio')).toHaveText(['90%', '100%', '110%', '125%', '150%']);
  await expect(scale.getByRole('radio', { name: '100%' })).toHaveAttribute('aria-checked', 'true');
  await scale.getByRole('radio', { name: '125%' }).click();
  await expect(scale.getByRole('radio', { name: '125%' })).toHaveAttribute('aria-checked', 'true');
  await expect.poll(() => html.evaluate((h) => h.style.zoom)).toBe('1.25');
  // the pressed option is still on screen (re-anchored under the pointer)
  await expect(scale.getByRole('radio', { name: '125%' })).toBeInViewport();
  await page.keyboard.press('Control+Shift+Minus');
  await expect.poll(() => html.evaluate((h) => h.style.zoom)).toBe('1.1');
  await page.keyboard.press('Control+Shift+Equal');
  await page.keyboard.press('Control+Shift+Equal');
  await expect.poll(() => html.evaluate((h) => h.style.zoom)).toBe('1.5');
  await page.keyboard.press('Control+Shift+Digit0');
  await expect.poll(() => html.evaluate((h) => h.style.zoom)).toBe('');
  await expect(scale.getByRole('radio', { name: '100%' })).toHaveAttribute('aria-checked', 'true');
});
