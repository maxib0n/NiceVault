import type { CDPSession, Locator, Page } from '@playwright/test';
import { boot, expect, test } from './helpers';

test.use({ viewport: { width: 412, height: 915 }, hasTouch: true, isMobile: true });
test.skip(({ browserName }) => browserName !== 'chromium', 'multi-touch through CDP');

type Pt = { x: number; y: number };
const touch = (cdp: CDPSession, type: 'touchStart' | 'touchMove' | 'touchEnd', pts: Pt[]) =>
  cdp.send('Input.dispatchTouchEvent', { type, touchPoints: pts.map((p, i) => ({ x: p.x, y: p.y, id: i + 1 })) });

async function tap(cdp: CDPSession, p: Pt) {
  await touch(cdp, 'touchStart', [p]);
  await touch(cdp, 'touchEnd', []);
}
async function doubleTap(page: Page, cdp: CDPSession, p: Pt) {
  await tap(cdp, p);
  await page.waitForTimeout(90);
  await tap(cdp, p);
}
async function pinch(cdp: CDPSession, c: Pt, gap0: number, gap1: number, steps = 8) {
  const at = (g: number) => [{ x: c.x - g / 2, y: c.y }, { x: c.x + g / 2, y: c.y }];
  await touch(cdp, 'touchStart', at(gap0));
  for (let k = 1; k <= steps; k++) await touch(cdp, 'touchMove', at(gap0 + ((gap1 - gap0) * k) / steps));
  await touch(cdp, 'touchEnd', []);
}
async function drag(cdp: CDPSession, a: Pt, b: Pt, steps = 10) {
  await touch(cdp, 'touchStart', [a]);
  for (let k = 1; k <= steps; k++) await touch(cdp, 'touchMove', [{ x: a.x + ((b.x - a.x) * k) / steps, y: a.y + ((b.y - a.y) * k) / steps }]);
  await touch(cdp, 'touchEnd', []);
}
const centre = async (l: Locator): Promise<Pt> => {
  const b = (await l.boundingBox())!;
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
};
const zoomOf = (l: Locator) => l.getAttribute('data-zoom').then(Number);

test('a picture: double tap in and out, pinch, pan while zoomed, no swipe while zoomed', async ({ page }) => {
  await boot(page, 'screen=vault&dir=Foto&preview=image&platform=mobile');
  const ql = page.getByRole('dialog', { name: /^Anteprima di/ });
  const name = await ql.getAttribute('aria-label');
  const viewer = page.getByTestId('viewer-image');
  const pic = viewer.getByRole('img');
  await expect(pic).toBeVisible();
  await expect.poll(() => zoomOf(viewer)).toBeLessThan(100);
  await page.waitForTimeout(700);
  const fit = await zoomOf(viewer);
  await expect(ql).not.toHaveAttribute('data-zoomed', 'true');
  const c = await centre(pic);

  await doubleTap(page, cdp(page), { x: c.x - 60, y: c.y - 30 });
  await expect.poll(() => zoomOf(viewer)).toBeGreaterThan(fit * 1.5);
  await expect(ql).toHaveAttribute('data-zoomed', 'true');
  await doubleTap(page, cdp(page), c);
  await expect.poll(() => zoomOf(viewer)).toBe(fit);

  await pinch(cdp(page), c, 80, 260);
  await expect.poll(() => zoomOf(viewer)).toBeGreaterThan(fit * 2);
  const zoomed = await zoomOf(viewer);

  const before = (await pic.boundingBox())!;
  await drag(cdp(page), c, { x: c.x + 70, y: c.y + 50 });
  await expect.poll(async () => (await pic.boundingBox())!.x).toBeGreaterThan(before.x + 8);
  expect((await pic.boundingBox())!.y).toBeGreaterThan(before.y + 10);
  expect(await zoomOf(viewer)).toBe(zoomed);

  await drag(cdp(page), { x: c.x + 120, y: c.y }, { x: c.x - 160, y: c.y }, 6);
  await drag(cdp(page), { x: c.x, y: c.y - 100 }, { x: c.x, y: c.y + 220 }, 6);
  await page.waitForTimeout(500);
  await expect(ql).toHaveAttribute('aria-label', name!);

  await pinch(cdp(page), c, 300, 40);
  await expect.poll(() => zoomOf(viewer)).toBe(fit);
  await expect(ql).not.toHaveAttribute('data-zoomed', 'true');
  await drag(cdp(page), { x: c.x + 140, y: c.y }, { x: c.x - 160, y: c.y }, 6);
  await expect(ql).not.toHaveAttribute('aria-label', name!);
});

test('a PDF: double tap zooms in where it lands, pinch, back to fit width', async ({ page }) => {
  await boot(page, 'screen=vault&dir=Lavoro&preview=Contratto affitto.pdf&platform=mobile');
  const pdf = page.getByTestId('viewer-pdf');
  await expect(pdf).toHaveAttribute('data-status', 'ready');
  await expect(pdf).toHaveAttribute('data-zoom', '100');
  const sheet = pdf.getByRole('img').first();
  await page.waitForTimeout(700);
  const c = await centre(sheet);
  await doubleTap(page, cdp(page), c);
  await expect(pdf).toHaveAttribute('data-zoom', '200');
  const scroller = page.getByTestId('viewer-scroll');
  await expect.poll(() => scroller.evaluate((el) => el.scrollWidth > el.clientWidth + 40)).toBe(true);
  expect(await scroller.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
  await doubleTap(page, cdp(page), c);
  await expect(pdf).toHaveAttribute('data-zoom', '100');
  await pinch(cdp(page), c, 60, 200);
  await expect.poll(() => pdf.getAttribute('data-zoom').then(Number)).toBeGreaterThan(150);
  await expect(page.getByRole('dialog', { name: 'Anteprima di Contratto affitto.pdf' })).toBeVisible();
});

async function stroke(page: Page, cdp: CDPSession, a: Pt, b: Pt, steps = 12) {
  await touch(cdp, 'touchStart', [a]);
  for (let k = 1; k <= steps; k++) {
    await touch(cdp, 'touchMove', [{ x: a.x + ((b.x - a.x) * k) / steps, y: a.y + ((b.y - a.y) * k) / steps }]);
    await page.waitForTimeout(16);
  }
  await touch(cdp, 'touchEnd', []);
}

for (const size of [
  { width: 412, height: 915 },
  { width: 820, height: 1180 },
]) {
  test.describe(`a long document under a finger (${size.width}×${size.height})`, () => {
    test.use({ viewport: size });
    for (const [dir, name] of [
      ['Personale/Anteprime', 'server.log'],
      ['Lavoro', 'Contratto affitto.pdf'],
    ]) {
      test(`${name}: strokes scroll it, the chrome stays away, a pull down at the top closes`, async ({ page }) => {
        await boot(page, `screen=vault&dir=${dir}&preview=${name}&platform=mobile`);
        const ql = page.getByRole('dialog', { name: `Anteprima di ${name}` });
        const sc = page.getByTestId('viewer-scroll');
        await expect.poll(() => sc.evaluate((el) => el.scrollHeight > el.clientHeight + 40)).toBe(true);
        await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1, { timeout: 5000 });
        const top = () => sc.evaluate((el) => el.scrollTop);
        const x = size.width / 2;
        const h = size.height;
        await stroke(page, cdp(page), { x, y: h * 0.75 }, { x: x + 5, y: h * 0.3 });
        await expect.poll(top).toBeGreaterThan(150);
        const mid = await top();
        await stroke(page, cdp(page), { x, y: h * 0.4 }, { x: x + 60, y: h * 0.55 });
        await expect.poll(top).toBeLessThan(mid);
        await stroke(page, cdp(page), { x, y: 30 }, { x, y: h * 0.5 });
        await page.waitForTimeout(600);
        await expect(ql).toBeVisible();
        await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1);
        await tap(cdp(page), { x, y: h * 0.5 });
        await expect(page.locator('dialog[data-chrome="shown"]')).toHaveCount(1);
        await page.waitForTimeout(400);
        await tap(cdp(page), { x, y: h * 0.5 });
        await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1);
        await sc.evaluate((el) => (el.scrollTop = 0));
        await page.waitForTimeout(250);
        await stroke(page, cdp(page), { x, y: h * 0.4 }, { x, y: h * 0.8 });
        await expect(ql).toHaveCount(0);
      });
    }
  });
}

test('a PDF zoomed: one finger pans both ways, no step, no close; back at fit the swipe steps', async ({ page }) => {
  await boot(page, 'screen=vault&dir=Lavoro&preview=Contratto affitto.pdf&platform=mobile');
  const ql = page.getByRole('dialog', { name: 'Anteprima di Contratto affitto.pdf' });
  const pdf = page.getByTestId('viewer-pdf');
  await expect(pdf).toHaveAttribute('data-status', 'ready');
  await page.waitForTimeout(700);
  const c = await centre(pdf.getByRole('img').first());
  await doubleTap(page, cdp(page), c);
  await expect(pdf).toHaveAttribute('data-zoom', '200');
  await expect(ql).toHaveAttribute('data-zoomed', 'true');
  const sc = page.getByTestId('viewer-scroll');
  const pos = () => sc.evaluate((el) => [el.scrollLeft, el.scrollTop]);
  const [l0, t0] = await pos();
  await stroke(page, cdp(page), { x: 300, y: 600 }, { x: 120, y: 380 });
  await expect.poll(async () => (await pos())[0]).toBeGreaterThan(l0 + 40);
  expect((await pos())[1]).toBeGreaterThan(t0 + 40);
  await stroke(page, cdp(page), { x: 80, y: 500 }, { x: 380, y: 500 }, 6);
  await sc.evaluate((el) => (el.scrollTop = 0));
  await stroke(page, cdp(page), { x: 200, y: 300 }, { x: 200, y: 700 }, 6);
  await page.waitForTimeout(500);
  await expect(ql).toBeVisible();
  await doubleTap(page, cdp(page), { x: 200, y: 400 });
  await expect(pdf).toHaveAttribute('data-zoom', '100');
  await expect(ql).not.toHaveAttribute('data-zoomed', 'true');
  await page.waitForTimeout(400);
  await stroke(page, cdp(page), { x: 360, y: 500 }, { x: 60, y: 505 }, 6);
  await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).not.toHaveAttribute('aria-label', 'Anteprima di Contratto affitto.pdf');
});

const sessions = new WeakMap<Page, CDPSession>();
function cdp(page: Page): CDPSession {
  const s = sessions.get(page);
  if (!s) throw new Error('no CDP session');
  return s;
}
test.beforeEach(async ({ page }) => {
  sessions.set(page, await page.context().newCDPSession(page));
});
