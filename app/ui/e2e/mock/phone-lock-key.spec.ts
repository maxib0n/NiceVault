import { boot, expect, fileList, listed, test } from './helpers';

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

for (const view of ['list', 'grid']) {
  test(`phone ${view}: floating keys cover no size or caption, the last row is reachable`, async ({ page }) => {
    await boot(page, `screen=vault&platform=mobile&view=${view}`);
    await listed(page);
    const lock = page.locator('.vault-screen .sidebar .lock svg');
    const fab = page.locator('.folder-head .actions .btn.primary');
    await expect(lock).toBeVisible();
    for (const top of [0, 1e6]) {
      await fileList(page).evaluate((el, t) => (el.scrollTop = t), top);
      await expect(async () => {
        for (const key of top ? [lock, fab] : [lock]) {
          const k = (await key.boundingBox())!;
          const hit = await page.evaluate(
            ([x, y, w, h]) => {
              const out: string[] = [];
              const view = document.querySelector('.file-view')!.getBoundingClientRect();
              document.querySelectorAll('.file-item .size, .file-item .meta-text, .file-item .name').forEach((e) => {
                const r = e.getBoundingClientRect();
                if (r.width && r.top < view.bottom && r.bottom > view.top && r.right > x && r.left < x + w && r.bottom > y && r.top < y + h) out.push(e.className);
              });
              return out;
            },
            [k.x - 4, k.y - 4, k.width + 8, k.height + 8],
          );
          expect(hit).toEqual([]);
        }
      }).toPass();
    }
    const k = (await lock.boundingBox())!;
    const last = (await page.locator('.file-view .file-item').last().boundingBox())!;
    expect(last.y + last.height).toBeLessThanOrEqual(k.y);
    const shadow = await lock.evaluate((e) => getComputedStyle(e).boxShadow);
    expect(shadow).not.toMatch(/rgba?\(255, 255, 255/);
  });
}
