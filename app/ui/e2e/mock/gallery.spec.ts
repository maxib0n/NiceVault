import { boot, expect, item, listed, test, toasts } from './helpers';
import type { Locator, Page } from '@playwright/test';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

async function longPress(target: Locator, ms = 700) {
  await target.evaluate(async (node, ms) => {
    const r = node.getBoundingClientRect();
    const o = { bubbles: true, cancelable: true, pointerType: 'touch', pointerId: 11, isPrimary: true, clientX: r.left + 24, clientY: r.top + r.height / 2 };
    node.dispatchEvent(new PointerEvent('pointerdown', o));
    await new Promise((res) => setTimeout(res, ms));
    node.dispatchEvent(new PointerEvent('pointerup', o));
    node.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, clientX: o.clientX, clientY: o.clientY }));
  }, ms);
}

const saved = (page: Page) => page.evaluate(() => (window as unknown as { __nv: { mock: { gallerySaved: number[] } } }).__nv.mock.gallerySaved.length);
const setting = (page: Page) => page.evaluate(() => (window as unknown as { __nv: { mock: { gallerySave: boolean | null } } }).__nv.mock.gallerySave);

test("the sheet of a photo saves it to the gallery; a document's sheet has no such action", async ({ page }) => {
  await boot(page, 'screen=vault&view=list&platform=mobile');
  await listed(page);
  await longPress(item(page, 'Leggimi.txt'));
  await expect(page.getByRole('menu')).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Salva nella Galleria' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await longPress(item(page, 'Copertina.jpg'));
  await page.getByRole('menuitem', { name: 'Salva nella Galleria' }).click();
  await expect(toasts(page)).toContainText('Salvati nella Galleria: 1');
  expect(await saved(page)).toBe(1);
  expect(await setting(page)).toBeNull();
});

test('Esporta asks once; "Nella Galleria" is remembered and shown in Impostazioni', async ({ page }) => {
  await boot(page, 'screen=vault&view=list&platform=mobile');
  await listed(page);
  await longPress(item(page, 'Copertina.jpg'));
  await page.getByRole('menuitem', { name: /^Esporta/ }).click();
  const ask = page.getByRole('dialog', { name: 'Salvare foto e video nella Galleria?' });
  await expect(ask).toBeVisible();
  await ask.getByTestId('gallery-ask-yes').click();
  await expect(ask).toBeHidden();
  await expect(toasts(page)).toContainText('Salvati nella Galleria: 1');
  expect(await setting(page)).toBe(true);
  await longPress(item(page, 'Copertina.jpg'));
  await page.getByRole('menuitem', { name: /^Esporta/ }).click();
  await expect.poll(() => saved(page)).toBe(2);
  await expect(ask).toBeHidden();
});

test('"In File" exports as before and turns the switch off', async ({ page }) => {
  await boot(page, 'screen=vault&view=list&platform=mobile');
  await listed(page);
  await longPress(item(page, 'Copertina.jpg'));
  await page.getByRole('menuitem', { name: /^Esporta/ }).click();
  await page.getByTestId('gallery-ask-files').click();
  await expect.poll(() => setting(page)).toBe(false);
  expect(await saved(page)).toBe(0);
});

test('Impostazioni has the switch on a phone', async ({ page }) => {
  await boot(page, 'screen=welcome&platform=mobile');
  await page.getByTestId('open-settings').click();
  const sw = page.getByTestId('gallery-save');
  await expect(sw).toHaveAttribute('aria-checked', 'true');
  await sw.click();
  await expect(sw).toHaveAttribute('aria-checked', 'false');
  expect(await setting(page)).toBe(false);
});
