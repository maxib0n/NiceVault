import type { Page } from '@playwright/test';
import { boot, expect, fileList, item, listed, nodeId, test } from './helpers';

const columns = (page: Page) =>
  page.locator('.file-view .items[data-layout="grid"]').evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(' ').filter(Boolean).length);

async function setGrid(page: Page, name: string) {
  await page.locator('button.sort').click();
  await page.getByRole('menuitemradio', { name: `Icone: ${name}` }).click();
}

test.describe('phone portrait', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test('S M L XL give 4 3 2 1 columns; rows and tiles reach the right edge', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&view=grid');
    await listed(page);
    const want: [string, string, number][] = [
      ['Piccole', 's', 4],
      ['Medie', 'm', 3],
      ['Grandi', 'l', 2],
      ['Molto grandi', 'xl', 1],
    ];
    for (const [name, size, n] of want) {
      await setGrid(page, name);
      await expect(page.locator('.file-view-wrap')).toHaveAttribute('data-grid-size', size);
      await expect.poll(() => columns(page)).toBe(n);
    }
    const pad = await fileList(page).evaluate((el) => {
      const v = el.closest('.file-view')!;
      return parseFloat(getComputedStyle(v).paddingRight);
    });
    expect(pad).toBeLessThanOrEqual(16);
    const items = await page.locator('.file-view .items').boundingBox();
    expect(items!.x + items!.width).toBeGreaterThan(390 - 20);
  });

  for (const view of ['list', 'grid']) {
    test(`${view}: Importa floats over the content, the last row scrolls clear of it`, async ({ page }) => {
      await boot(page, `screen=vault&platform=mobile&view=${view}`);
      await listed(page);
      const fab = page.locator('.folder-head .actions .btn.primary');
      await expect(fab).toBeVisible();
      const k = (await fab.boundingBox())!;
      const view_ = (await page.locator('.file-view').boundingBox())!;
      expect(k.x).toBeLessThan(view_.x + view_.width);
      expect(k.y + k.height).toBeLessThanOrEqual(view_.y + view_.height);
      await fileList(page).evaluate((el) => (el.closest('.file-view')!.scrollTop = 1e6));
      await expect(async () => {
        const last = (await page.locator('.file-view .file-item').last().boundingBox())!;
        expect(last.y + last.height).toBeLessThanOrEqual(k.y);
      }).toPass();
    });
  }

  test('the full-screen dock asks for the thumbnails of the videos it shows', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&dir=Video&view=list&preview=Drone vacanze.mov');
    const dock = page.getByTestId('ql-dock');
    await expect(dock).toHaveAttribute('data-shown', 'true');
    for (const name of ['Compleanno Giulia.webm', 'Tramonto timelapse.mp4']) {
      const frame = dock.getByRole('button', { name });
      await expect(frame.getByTestId('video-thumb')).toHaveCount(1, { timeout: 20_000 });
      const img = frame.getByTestId('thumbnail-image');
      await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0 && getComputedStyle(i).opacity), { timeout: 8000 }).toBe('1');
    }
  });

  test('an original too big to be a thumbnail (a HEIC the backend cannot scale) is shown plainly in the grid', async ({ page, consoleErrors }) => {
    await page.route('**/mock/photo-14.jpg', (r) =>
      r.fulfill({ contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="4000" height="3000"><rect width="4000" height="3000" fill="#c84"/></svg>' }),
    );
    await boot(page, 'screen=vault&platform=mobile&view=grid');
    await listed(page);
    const img = item(page, 'Copertina.jpg').getByTestId('thumbnail-image');
    await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.naturalWidth)).toBe(4000);
    const id = await nodeId(page, 'Copertina.jpg');
    await expect
      .poll(() => page.evaluate((k) => ((window as unknown as { __nvReveal?: { key: string; what: string }[] }).__nvReveal ?? []).filter((e) => e.key.endsWith(`:${k}`) && e.key.startsWith('thumb:')).map((e) => e.what), id))
      .toContain('large');
    await expect.poll(() => img.evaluate((i) => getComputedStyle(i).opacity)).toBe('1');
    const rest = consoleErrors.filter((e) => e !== 'console: Failed to load resource');
    consoleErrors.splice(0, consoleErrors.length, ...rest);
  });
});

test.describe('phone landscape', () => {
  test.use({ viewport: { width: 844, height: 390 }, hasTouch: true, isMobile: true });

  test('more columns on its side, still distinct per size', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&view=grid');
    await listed(page);
    const got: number[] = [];
    for (const name of ['Piccole', 'Medie', 'Grandi', 'Molto grandi']) {
      await setGrid(page, name);
      await page.waitForTimeout(100);
      got.push(await columns(page));
    }
    expect(got[0]).toBeGreaterThan(got[1]);
    expect(got[1]).toBeGreaterThan(got[2]);
    expect(got[2]).toBeGreaterThan(got[3]);
    expect(got[3]).toBeGreaterThanOrEqual(2);
  });
});
