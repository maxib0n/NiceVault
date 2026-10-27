import type { Page } from '@playwright/test';
import { boot, expect, expectFolder, fileList, item, listed, selectedNames, test } from './helpers';

const side = (page: Page, button: 3 | 4) =>
  page.evaluate((b) => {
    window.dispatchEvent(new MouseEvent('mousedown', { button: b, bubbles: true, cancelable: true }));
    window.dispatchEvent(new MouseEvent('mouseup', { button: b, bubbles: true, cancelable: true }));
  }, button);

test('mouse back / forward walk the folder history; the selection comes back', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await item(page, 'Foto').dblclick();
  await expectFolder(page, 'Foto');
  await item(page, 'Vacanze 2024').dblclick();
  await expectFolder(page, 'Vacanze 2024');

  await side(page, 3);
  await expectFolder(page, 'Foto');
  expect(await selectedNames(page)).toEqual(['Vacanze 2024']);
  await side(page, 3);
  await expectFolder(page, 'Documenti');
  expect(await selectedNames(page)).toEqual(['Foto']);
  await side(page, 3);
  await expectFolder(page, 'Documenti');

  await side(page, 4);
  await expectFolder(page, 'Foto');
  await side(page, 4);
  await expectFolder(page, 'Vacanze 2024');
  await side(page, 4);
  await expectFolder(page, 'Vacanze 2024');
});

test('Alt+← / Alt+→ go back and forward; a new folder drops the forward places', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await item(page, 'Lavoro').dblclick();
  await expectFolder(page, 'Lavoro');
  await fileList(page).focus();
  await page.keyboard.press('Alt+ArrowLeft');
  await expectFolder(page, 'Documenti');
  await page.keyboard.press('Alt+ArrowRight');
  await expectFolder(page, 'Lavoro');
  await page.keyboard.press('Alt+ArrowLeft');
  await expectFolder(page, 'Documenti');
  await item(page, 'Audio').dblclick();
  await expectFolder(page, 'Audio');
  await fileList(page).focus();
  await page.keyboard.press('Alt+ArrowRight');
  await expectFolder(page, 'Audio');
});

test('in quick look the back button closes it, and only that', async ({ page }) => {
  await boot(page, 'screen=vault&dir=Foto&preview=image');
  const ql = page.locator('dialog[open]');
  await expect(ql).toHaveCount(1);
  await side(page, 3);
  await expect(page.locator('dialog[open]')).toHaveCount(0);
  await expectFolder(page, 'Foto');
});

test('the ink chosen in Dettagli is the passport on Benvenuto at once', async ({ page }) => {
  await boot(page, 'screen=details');
  await page.getByTestId('accent-swatches').locator('[data-accent="bottiglia"]').click();
  await expect(page.locator('html')).toHaveAttribute('data-accent', 'bottiglia');
  await page.getByRole('complementary').getByRole('button', { name: /altro volume/i }).click();
  const passport = page.locator('[data-testid="passport"][data-name="Documenti"]');
  await expect(passport).toHaveAttribute('data-accent', 'bottiglia');
});

test('no browser context menu where the app has none; text fields keep theirs', async ({ page }) => {
  await boot(page, 'screen=details');
  const blocked = (sel: string) =>
    page.locator(sel).first().evaluate((el) => {
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
      el.dispatchEvent(e);
      return e.defaultPrevented;
    });
  expect(await blocked('main h1')).toBe(true);
  expect(await blocked('aside')).toBe(true);
  await boot(page, 'screen=create');
  expect(await blocked('input#vol-name')).toBe(false);
});
