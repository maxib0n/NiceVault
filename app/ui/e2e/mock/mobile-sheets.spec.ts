// Phone, iPhone engine with a touch screen: Impostazioni is centred in its window, the Vista sheet's
// keys line up, and every bottom sheet is dismissed by pulling it down (a short pull springs back).
// Playwright cannot drag with WebKit's touchscreen, so the strokes are Pointer Events (touch) dispatched
// on the sheet, which is what lib/sheetSwipe.ts listens to.
import type { Locator, Page } from '@playwright/test';
import { boot, expect, item, listed, test } from './helpers';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

/** a one-finger stroke down from (x, y0) by `dy`, in `steps`, `ms` apart, on `target` */
async function pull(target: Locator, x: number, y0: number, dy: number, ms = 16, steps = 8) {
  await target.evaluate(
    async (el, a) => {
      const fire = (type: string, y: number) =>
        el.dispatchEvent(new PointerEvent(type, { pointerId: 7, pointerType: 'touch', isPrimary: true, bubbles: true, cancelable: true, clientX: a.x, clientY: y, button: 0 }));
      fire('pointerdown', a.y0);
      for (let k = 1; k <= a.steps; k++) {
        await new Promise((r) => setTimeout(r, a.ms));
        fire('pointermove', a.y0 + (a.dy * k) / a.steps);
      }
      fire('pointerup', a.y0 + a.dy);
    },
    { x, y0, dy, ms, steps },
  );
}

async function openView(page: Page) {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await page.getByRole('button', { name: 'Vista', exact: true }).click();
  const sheet = page.getByTestId('phone-view-sheet');
  await expect(sheet).toBeVisible();
  await page.waitForTimeout(600); // it has risen
  return page.getByRole('dialog', { name: 'Vista' });
}

test('Impostazioni: the column is centred, equal gutters', async ({ page }) => {
  await boot(page, 'screen=settings&platform=mobile');
  const pages = page.locator('.settings .pages');
  await expect(pages).toBeVisible();
  await page.waitForTimeout(500);
  const b = (await pages.boundingBox())!;
  const left = b.x;
  const right = 390 - (b.x + b.width);
  expect(Math.abs(left - right)).toBeLessThanOrEqual(1);
  expect(left).toBeGreaterThanOrEqual(4);
  const bar = (await page.locator('.settings .bar').boundingBox())!;
  expect(Math.abs(bar.x - (390 - bar.x - bar.width))).toBeLessThanOrEqual(1);
});

test('the Vista sheet: icon and label of Griglia / Elenco on one line, keys of one height', async ({ page }) => {
  await openView(page);
  const sheet = page.getByTestId('phone-view-sheet');
  const keys = sheet.getByRole('radio');
  const modes = [keys.nth(0), keys.nth(1)];
  const bs = [];
  for (const k of modes) {
    const kb = (await k.boundingBox())!;
    const ib = (await k.locator('svg').boundingBox())!;
    const tb = await k.evaluate((el) => {
      const r = document.createRange();
      const t = [...el.childNodes].find((n) => n.nodeType === 3 && n.textContent?.trim())!;
      r.selectNodeContents(t);
      const b = r.getBoundingClientRect();
      return { y: b.y, h: b.height };
    });
    // the icon's centre and the text's centre sit on one line, both centred in the key
    expect(Math.abs(ib.y + ib.height / 2 - (tb.y + tb.h / 2))).toBeLessThanOrEqual(5);
    expect(Math.abs(ib.y + ib.height / 2 - (kb.y + kb.height / 2))).toBeLessThanOrEqual(2);
    bs.push(kb);
  }
  expect(Math.abs(bs[0].height - bs[1].height)).toBeLessThanOrEqual(0.5);
  expect(Math.abs(bs[0].y - bs[1].y)).toBeLessThanOrEqual(0.5);
  const size = (await keys.nth(2).boundingBox())!;
  expect(Math.abs(size.height - bs[0].height)).toBeLessThanOrEqual(2);
});

test('a bottom sheet (Vista): pulled down by its head it closes; a short pull springs back', async ({ page }) => {
  const dlg = await openView(page);
  const b = (await dlg.boundingBox())!;
  const x = b.x + b.width / 2;
  // short: back where it was
  await pull(dlg, x, b.y + 12, 40);
  await expect(dlg).toBeVisible();
  await expect.poll(async () => Math.round((await dlg.boundingBox())!.y)).toBe(Math.round(b.y));
  // far: gone
  await pull(dlg, x, b.y + 12, Math.round(b.height * 0.6));
  await expect(page.getByTestId('phone-view-sheet')).toBeHidden();
});

test('a flick closes a sheet too', async ({ page }) => {
  const dlg = await openView(page);
  const b = (await dlg.boundingBox())!;
  await pull(dlg, b.x + b.width / 2, b.y + 12, 60, 4, 5);
  await expect(page.getByTestId('phone-view-sheet')).toBeHidden();
});

test('the item menu is pulled down the same way', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await item(page, 'Copertina.jpg').click({ button: 'right' });
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await page.waitForTimeout(400);
  const mb = (await menu.boundingBox())!;
  await pull(menu, mb.x + mb.width / 2, mb.y + 12, Math.round(mb.height * 0.6));
  await expect(menu).toBeHidden();
});

test('the rename slip is pulled down the same way', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await item(page, 'Copertina.jpg').click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Rinomina/ }).click();
  const slip = page.getByTestId('mobile-rename');
  await expect(slip).toBeVisible();
  await page.waitForTimeout(400);
  const sb = (await slip.boundingBox())!;
  await pull(slip, sb.x + sb.width / 2, sb.y + 10, Math.round(sb.height * 0.6));
  await expect(slip).toBeHidden();
});

test('a scrolled menu keeps its scroll: a pull inside it does not close it', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await item(page, 'Copertina.jpg').click({ button: 'right' });
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await page.waitForTimeout(400);
  const mb = (await menu.boundingBox())!;
  const last = menu.getByRole('menuitem').last();
  // a pull that starts low in the slip, on its content, while it is scrolled down, is the scroll's
  await menu.evaluate((el) => (el.scrollTop = 40));
  const scrolls = await menu.evaluate((el) => el.scrollHeight > el.clientHeight);
  if (scrolls) {
    await pull(last, mb.x + 20, mb.y + mb.height - 20, 300);
    await expect(menu).toBeVisible();
  }
});
