// The phone's own shell (?platform=mobile at phone width, styles/mobile.css "pocket edition"):
// the flap at the foot with the clasp, menus drawn up from it as sheets, the shelf of volumes above
// a volume's root, the passports as a deck.
import { boot, expect, item, listed, test } from './helpers';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

test('the flap: five places at the foot, the clasp in the middle', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  // measured once the volume's page has settled (it rises in on arrival)
  await expect(async () => {
    const files = (await page.getByTestId('nav-files').boundingBox())!;
    const lock = (await page.getByRole('button', { name: 'Blocca', exact: true }).boundingBox())!;
    const details = (await page.getByTestId('nav-details').boundingBox())!;
    const settings = (await page.getByTestId('open-settings').boundingBox())!;
    // all on the bottom line, in the order File · Password · Blocca · Dettagli · Impostazioni
    for (const b of [files, lock, details, settings]) expect(b.y + b.height).toBeGreaterThan(844 - 90);
    expect(files.x).toBeLessThan(lock.x);
    expect(lock.x).toBeLessThan(details.x);
    expect(details.x).toBeLessThan(settings.x);
    // every place is a finger wide
    for (const b of [files, lock, details, settings]) expect(b.width).toBeGreaterThanOrEqual(44);
  }).toPass();
});

test("an item's menu is a sheet drawn up from the bottom", async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await item(page, 'Copertina.jpg').click({ button: 'right' });
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await expect(async () => {
    const box = (await menu.boundingBox())!;
    expect(Math.round(box.x)).toBe(0);
    expect(Math.round(box.width)).toBe(390);
    expect(Math.round(box.y + box.height)).toBe(844);
  }).toPass();
  // a row a finger can hit
  const row = (await menu.getByRole('menuitem').first().boundingBox())!;
  expect(row.height).toBeGreaterThanOrEqual(44);
});

test("above a volume's root is the shelf; its passport brings it back", async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await page.getByTestId('mobile-shelf').click();
  const passport = page.getByTestId('passport').first();
  await expect(passport).toBeVisible();
  await passport.getByRole('button').first().click();
  await listed(page);
});

test('the passports scroll sideways as a deck, one at a time', async ({ page }) => {
  await boot(page, 'screen=welcome&platform=mobile');
  const list = page.getByTestId('passport').first().locator('..');
  await expect(page.getByTestId('passport').first()).toBeVisible();
  const style = await list.evaluate((el) => {
    const s = getComputedStyle(el);
    return { snap: s.scrollSnapType, wraps: s.flexWrap, scrolls: el.scrollWidth > el.clientWidth };
  });
  expect(style.snap).toContain('x');
  expect(style.wraps).toBe('nowrap');
  expect(style.scrolls).toBe(true);
});

/** A finger resting on an element (Pointer Events, as WebKit / Chromium send for a touch). */
async function longPress(target: import('@playwright/test').Locator, ms = 700) {
  await target.evaluate(async (node, ms) => {
    const r = node.getBoundingClientRect();
    const o = { bubbles: true, cancelable: true, pointerType: 'touch', pointerId: 11, isPrimary: true, clientX: r.left + 24, clientY: r.top + r.height / 2 };
    node.dispatchEvent(new PointerEvent('pointerdown', o));
    await new Promise((res) => setTimeout(res, ms));
    node.dispatchEvent(new PointerEvent('pointerup', o));
    node.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, clientX: o.clientX, clientY: o.clientY }));
  }, ms);
}

test('the page never zooms; the viewport runs under the notch', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  const vp = await page.locator('meta[name=viewport]').getAttribute('content');
  expect(vp).toContain('user-scalable=no');
  expect(vp).toContain('maximum-scale=1');
  expect(vp).toContain('viewport-fit=cover');
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).touchAction)).toBe('manipulation');
});

test("a long press opens the item's sheet; the lift does not open the item", async ({ page }) => {
  await boot(page, 'screen=vault&view=list&platform=mobile');
  await listed(page);
  await longPress(item(page, 'Foto'));
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Rinomina' })).toBeVisible();
  // still in Documenti: the click after the lift was swallowed
  await expect(item(page, 'Audio')).toBeVisible();
  // a short touch is a tap, not a press
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
});

test('rename from the sheet: a slip above the keyboard, its own keys', async ({ page }) => {
  await boot(page, 'screen=vault&view=list&platform=mobile');
  await listed(page);
  await longPress(item(page, 'Copertina.jpg'));
  await page.getByRole('menuitem', { name: 'Rinomina' }).click();
  const sheet = page.getByTestId('mobile-rename');
  await expect(sheet).toBeVisible();
  const field = sheet.getByRole('textbox', { name: 'Nuovo nome' });
  await expect(field).toBeFocused();
  await expect(field).toHaveValue('Copertina.jpg');
  // the stem is selected: typing replaces it, the extension stays
  await page.keyboard.type('Retro');
  await expect(field).toHaveValue('Retro.jpg');
  // the sheet sits on the bottom edge (no keyboard in the test browser)
  await expect(async () => {
    const box = (await sheet.boundingBox())!;
    expect(Math.round(box.y + box.height)).toBe(844);
  }).toPass();
  await sheet.getByRole('button', { name: 'Rinomina' }).click();
  await expect(sheet).toBeHidden();
  await expect(item(page, 'Retro.jpg')).toBeVisible();
});

test('Seleziona: taps pick several, the slip acts on them, Fine ends it', async ({ page }) => {
  await boot(page, 'screen=vault&view=list&platform=mobile');
  await listed(page);
  await longPress(item(page, 'Copertina.jpg'));
  await page.getByRole('menuitem', { name: 'Seleziona' }).click();
  const bar = page.getByTestId('mobile-select');
  await expect(bar).toBeVisible();
  await expect(bar).toContainText('1 selezionato');
  await item(page, 'Leggimi.txt').click();
  await expect(bar).toContainText('2 selezionati');
  // a tap picks, it does not open (no preview)
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await bar.getByRole('button', { name: 'Elimina' }).click();
  await expect(page.getByRole('dialog')).toContainText('2');
  await page.getByRole('dialog').getByRole('button', { name: 'Annulla' }).click();
  await page.getByTestId('mobile-select-done').click();
  await expect(bar).toBeHidden();
});

test('Importa asks: Foto e video (the photo picker) or File', async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile');
  await listed(page);
  await page.getByRole('button', { name: 'Importa', exact: true }).click();
  const menu = page.getByRole('menu', { name: 'Importa da' });
  await expect(menu.getByRole('menuitem', { name: 'Foto e video' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'File' })).toBeVisible();
  await menu.getByRole('menuitem', { name: 'Foto e video' }).click();
  await expect(item(page, 'IMG_0412.HEIC')).toBeVisible();
  await expect(item(page, 'IMG_0413.MOV')).toBeVisible();
});

test('the preview covers the whole glass, the app chrome too', async ({ page }) => {
  await boot(page, 'screen=vault&dir=Foto&preview=image&platform=mobile');
  const ql = page.getByRole('dialog');
  await expect(ql).toBeVisible();
  // measured once it has settled (it rises in)
  await expect(async () => {
    const box = (await ql.boundingBox())!;
    expect(Math.round(box.x)).toBe(0);
    expect(Math.round(box.y)).toBe(0);
    expect(Math.round(box.width)).toBe(390);
    expect(Math.round(box.height)).toBe(844);
  }).toPass();
  // the picture zooms with fingers: no zoom strip on a phone
  await expect(page.getByRole('toolbar', { name: /immagine/i })).toBeHidden();
});

test('the board and the page paint their grain themselves (no blend across layers: iOS)', async ({ page }) => {
  for (const [q, tex] of [
    ['screen=welcome&theme=dark', 'felt-board-a62'],
    ['screen=settings&theme=dark', 'felt-board-a62'],
    ['screen=settings&theme=light', 'board-grain-a20'],
  ]) {
    await boot(page, `${q}&platform=mobile`);
    const look = await page.locator('main.ground').first().evaluate((el) => ({
      before: getComputedStyle(el, '::before').display,
      image: getComputedStyle(el).backgroundImage,
    }));
    expect(look.before).toBe('none');
    expect(look.image).toContain(tex);
  }
});
