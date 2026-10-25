// The on-screen keyboard on a phone (2026-10-25, iPhone: "appena premo un volume, mette tastiera,
// si alza tutto e poi torna indietro da solo"). iOS never shrinks the page for the keys: the
// visual viewport gets shorter, and iOS slides it up to show the field, then back. Playwright
// can't raise a real keyboard, so window.visualViewport is replaced by one we drive: its height
// shrinks by the keys' height, its offsetTop slides up and back as iOS does. The field with the
// caret (and its form's key) must stay in the visible part throughout, on the same screen.
import { type Page } from '@playwright/test';
import { boot, expect, expectScreen, test } from './helpers';

const W = 393;
const H = 852;
/** an iPhone's keys with the QuickType bar */
const KEYS = 336;

test.use({ viewport: { width: W, height: H }, hasTouch: true });

test.beforeEach(async ({ page }) => {
  await page.addInitScript(
    ([w, h]) => {
      const vv = new EventTarget() as EventTarget & Record<string, number>;
      Object.assign(vv, { width: w, height: h, offsetTop: 0, offsetLeft: 0, pageTop: 0, pageLeft: 0, scale: 1 });
      Object.defineProperty(window, 'visualViewport', { configurable: true, get: () => vv });
      (window as unknown as { __kb: (keys: number, slid: number) => void }).__kb = (keys, slid) => {
        vv.height = h - keys;
        vv.offsetTop = slid;
        vv.dispatchEvent(new Event('resize'));
        vv.dispatchEvent(new Event('scroll'));
      };
    },
    [W, H],
  );
});

/** the keys up (`keys` px), the viewport slid up by `slid` px, as iOS does round a focus */
const keyboard = (page: Page, keys: number, slid = 0) => page.evaluate(([k, s]) => (window as unknown as { __kb: (k: number, s: number) => void }).__kb(k, s), [keys, slid]);

/** `loc` lies wholly inside the visible part of the screen (above the keys, under any slide) */
async function aboveKeys(page: Page, loc: ReturnType<Page['locator']>, slid = 0) {
  await expect(async () => {
    const b = (await loc.boundingBox())!;
    expect(b.y).toBeGreaterThanOrEqual(slid - 1);
    expect(b.y + b.height).toBeLessThanOrEqual(slid + H - KEYS + 1);
  }).toPass({ timeout: 3000 });
}

test('Sblocca on a phone: no keyboard on arrival; with the keys up the field and Sblocca stay above them, on Sblocca', async ({ page }) => {
  await boot(page, 'screen=welcome&platform=mobile');
  await page.getByTestId('passport').first().getByRole('button').first().click();
  await expectScreen(page, 'unlock');
  const field = page.getByLabel('Password', { exact: true });
  const submit = page.getByRole('button', { name: 'Sblocca', exact: true });
  await expect(field).toBeVisible();
  // the keyboard does not rise by itself while the page arrives
  await page.waitForTimeout(300);
  await expect(field).not.toBeFocused();
  // a tap on the field: the keys come up, iOS slides the page up, then back down
  await field.tap();
  await expect(field).toBeFocused();
  await keyboard(page, KEYS, 0);
  await aboveKeys(page, field);
  await aboveKeys(page, submit);
  await keyboard(page, KEYS, 180);
  await aboveKeys(page, field, 180);
  await keyboard(page, KEYS, 0);
  await aboveKeys(page, field);
  await aboveKeys(page, submit);
  await page.waitForTimeout(400);
  await expectScreen(page, 'unlock');
  await expect(field).toBeFocused();
  // typing and Sblocca work with the keys up
  await field.fill('demo');
  await submit.tap();
  await expectScreen(page, 'vault');
});

test('the keys go down: the scroller gives back its room', async ({ page }) => {
  await boot(page, 'screen=unlock&platform=mobile');
  const main = page.locator('main.unlock');
  const before = await main.evaluate((el) => getComputedStyle(el).paddingBottom);
  const field = page.getByLabel('Password', { exact: true });
  await field.tap();
  await keyboard(page, KEYS, 0);
  await aboveKeys(page, field);
  await expect.poll(() => main.evaluate((el) => getComputedStyle(el).paddingBottom)).not.toBe(before);
  await field.blur();
  await keyboard(page, 0, 0);
  await expect.poll(() => main.evaluate((el) => getComputedStyle(el).paddingBottom)).toBe(before);
  await expectScreen(page, 'unlock');
});


for (const [name, query, pick, prep] of [
  ['Nuovo volume', 'screen=create&platform=mobile', (p: Page) => p.locator('#vol-name'), null],
  ['Password editor', 'screen=passwords&dialog=pm-new&platform=mobile', (p: Page) => p.locator('dialog[open]').locator('input:not([type=checkbox]):not([type=radio]), textarea').last(), null],
  // (a phone's head shows a Cerca key; the field opens across the head)
  ['Search', 'screen=vault&platform=mobile', (p: Page) => p.getByRole('searchbox').first(), (p: Page) => p.getByTestId('phone-search').click()],
] as const) {
  test(`${name}: the field with the caret stays above the keys, on the same screen`, async ({ page }) => {
    await boot(page, query);
    if (prep) await prep(page);
    const screenNow = await page.locator('[data-ready="true"]').getAttribute('data-screen');
    const field = pick(page);
    await field.scrollIntoViewIfNeeded();
    await expect(field).toBeVisible();
    await field.tap();
    await expect(field).toBeFocused();
    await keyboard(page, KEYS, 0);
    await aboveKeys(page, field);
    await keyboard(page, KEYS, 200);
    await keyboard(page, KEYS, 0);
    await aboveKeys(page, field);
    await page.waitForTimeout(300);
    await expect(field).toBeFocused();
    await expectScreen(page, screenNow!);
  });
}
