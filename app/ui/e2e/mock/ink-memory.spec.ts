// Phones: the ink (ornament canvases) at the full pixel ratio must not stay resident as canvas
// backing stores (the iPhone's web content process was killed for its memory, 2026-10-25). Each
// sheet is engraved once, kept as an encoded image and shown as one; the canvases it was drawn on
// give their pixels back. WebKit with a phone's pixel ratio 3 on the screens that crashed.
import { boot, expect, test } from './helpers';

test.use({ viewport: { width: 393, height: 852 }, deviceScaleFactor: 3, isMobile: true, hasTouch: true });
test.skip(({ browserName }) => browserName !== 'webkit', 'the iPhone engine');

test.beforeEach(async ({ page }) => {
  // every canvas a 2D context was taken on, and whether the ink took it (its call stack)
  await page.addInitScript(() => {
    const seen = new Map<HTMLCanvasElement, boolean>();
    (window as unknown as { __canvases: typeof seen }).__canvases = seen;
    const get = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, ...a: Parameters<typeof get>) {
      if (!seen.has(this)) seen.set(this, /\/ornament\/|components\/Emblem/.test(new Error().stack ?? ''));
      return get.apply(this, a);
    } as typeof get;
  });
});

async function held(page: import('@playwright/test').Page) {
  return page.evaluate(() => {
    const all = (window as unknown as { __canvases: Map<HTMLCanvasElement, boolean> }).__canvases;
    let inkPx = 0;
    let otherPx = 0;
    for (const [c, inkish] of all) {
      // the engraving plates' textures (engrave.ts): one 128×128 tile per pattern, kept for good
      if (inkish && c.width === 128 && c.height === 128) continue;
      if (inkish) inkPx += c.width * c.height;
      else otherPx += c.width * c.height;
    }
    const shown = [...document.querySelectorAll<HTMLElement>('.ink-canvas[data-inked="true"]')];
    const imagePx = shown.reduce((s, e) => s + (e.dataset.sheet ?? '0x0').split('x').map(Number).reduce((a, b) => a * b), 0);
    return { inkPx, otherPx, inked: shown.length, imagePx };
  });
}

for (const [label, q] of [
  ['Benvenuto', 'screen=welcome'],
  ['Sblocca', 'screen=unlock'],
  ['the files', 'screen=vault'],
  ['Dettagli', 'screen=details'],
  ['the emblem picker', 'screen=details&dialog=emblem'],
  ['the passwords', 'screen=passwords'],
  ['Impostazioni', 'screen=settings'],
  ['Crea dispositivo', 'screen=device'],
  ['Benvenuto, photo emblems', 'screen=welcome&emblem=image'],
  ['the envelopes', 'screen=vault&view=grid&dir=Personale/Buste'],
] as const) {
  test(`${label}: no ink canvas holds pixels once painted, at 3x`, async ({ page }) => {
    await boot(page, `${q}&platform=mobile`);
    await expect.poll(async () => (await held(page)).inked, { timeout: 15_000 }).toBeGreaterThan(0);
    await expect.poll(async () => (await held(page)).inkPx, { timeout: 15_000 }).toBe(0);
    const h = await held(page);
    console.log(
      `${label}: ink canvases ${(h.inkPx / 1e6).toFixed(2)} Mpx, ${h.inked} sheets as images ${(h.imagePx / 1e6).toFixed(2)} Mpx, other canvases ${(h.otherPx / 1e6).toFixed(2)} Mpx`,
    );
  });
}
