// WebKit (the iPhone's engine) with a touch screen: Playwright taps there but cannot drag, so the
// strokes are in touch-zoom.spec.ts (Chromium, CDP); here a double tap zooms a picture at the
// point and back, a single tap on a document brings the chrome, and the scrollers keep pan-y.
import { boot, expect, test } from "./helpers";

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });
test.skip(({ browserName }) => browserName !== "webkit", "the iPhone engine");

test("a picture: double tap in at the point, again back to fit", async ({
  page,
}) => {
  await boot(page, "screen=vault&dir=Foto&preview=image&platform=mobile");
  const ql = page.getByRole("dialog", { name: /^Anteprima di/ });
  const viewer = page.getByTestId("viewer-image");
  const pic = viewer.getByRole("img");
  await expect(pic).toBeVisible();
  await page.waitForTimeout(800);
  const fit = Number(await viewer.getAttribute("data-zoom"));
  const b = (await pic.boundingBox())!;
  const p = { x: b.x + b.width * 0.3, y: b.y + b.height * 0.4 };
  await page.touchscreen.tap(p.x, p.y);
  await page.waitForTimeout(80);
  await page.touchscreen.tap(p.x, p.y);
  await expect
    .poll(async () => Number(await viewer.getAttribute("data-zoom")))
    .toBeGreaterThan(fit * 1.5);
  await expect(ql).toHaveAttribute("data-zoomed", "true");
  // the point tapped stays about where it was (the zoom grew round it, not round the centre)
  const z = (await pic.boundingBox())!;
  const fx = (p.x - b.x) / b.width;
  expect(Math.abs(z.x + z.width * fx - p.x)).toBeLessThan(40);
  await page.waitForTimeout(400);
  await page.touchscreen.tap(p.x, p.y);
  await page.waitForTimeout(80);
  await page.touchscreen.tap(p.x, p.y);
  await expect
    .poll(async () => Number(await viewer.getAttribute("data-zoom")))
    .toBe(fit);
  await expect(ql).not.toHaveAttribute("data-zoomed", "true");
});

for (const [dir, name] of [
  ["Personale/Anteprime", "server.log"],
  ["Lavoro", "Contratto affitto.pdf"],
]) {
  test(`${name}: pan-y down to the scroller, a tap brings the chrome, scrolling does not`, async ({
    page,
  }) => {
    await boot(page, `screen=vault&dir=${dir}&preview=${name}&platform=mobile`);
    const sc = page.getByTestId("viewer-scroll");
    await expect
      .poll(() => sc.evaluate((el) => el.scrollHeight > el.clientHeight + 40))
      .toBe(true);
    expect(
      await sc.evaluate((el) => {
        for (let n: Element | null = el; n; n = n.parentElement) {
          const t = getComputedStyle(n).touchAction;
          if (!(t === "auto" || t.includes("pan-y") || t === "manipulation"))
            return false;
        }
        return true;
      }),
    ).toBe(true);
    await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1, {
      timeout: 5000,
    });
    await sc.evaluate((el) => el.scrollBy(0, 500));
    await page.waitForTimeout(400);
    await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1);
    const b = (await sc.boundingBox())!;
    await page.touchscreen.tap(b.x + b.width / 2, b.y + b.height / 2);
    await expect(page.locator('dialog[data-chrome="shown"]')).toHaveCount(1);
  });
}
