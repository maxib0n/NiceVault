// Phone fixes (iPhone): the full-screen dock stays while touched / scrolled, its sheets
// are whole inside their tiles, the view size is in the sort sheet; video files get a frame as
// their thumbnail (made by the webview, lib/videoThumb.ts), with a play mark.
import { boot, expect, item, listed, test } from './helpers';

test.describe('phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test('the dock stays open while it is scrolled, and hides after', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&dir=Personale/Anteprime&preview=server.log');
    const dock = page.getByTestId('ql-dock');
    await expect(dock).toHaveAttribute('data-shown', 'true');
    // a finger on the strip, scrolling it now and then for longer than the 2 s idle
    await dock.dispatchEvent('touchstart', { touches: [], changedTouches: [] });
    for (let i = 0; i < 6; i++) {
      await page.getByTestId('ql-dock').evaluate((el, x) => el.querySelector('.strip')!.scrollTo({ left: x }), i * 30);
      await page.waitForTimeout(450);
    }
    await expect(dock).toHaveAttribute('data-shown', 'true');
    await dock.dispatchEvent('touchend', { touches: [], changedTouches: [] });
    await expect(dock).toHaveAttribute('data-shown', 'false', { timeout: 6000 });
  });

  test("the dock's sheets are whole inside their tiles", async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&dir=Personale/Anteprime&preview=server.log');
    const dock = page.getByTestId('ql-dock');
    await expect(dock).toHaveAttribute('data-shown', 'true');
    const bad = await dock.evaluate((d) => {
      const strip = d.querySelector('.strip')!.getBoundingClientRect();
      const out: string[] = [];
      for (const f of d.querySelectorAll<HTMLElement>('[data-node]')) {
        const fr = f.getBoundingClientRect();
        const sheet = f.querySelector('.paper-file');
        if (!sheet) continue;
        const r = sheet.getBoundingClientRect();
        if (r.top < fr.top - 0.5 || r.bottom > fr.bottom + 0.5 || r.left < fr.left - 0.5 || r.right > fr.right + 0.5) out.push(`${f.ariaLabel} outside its tile`);
        if (r.top < strip.top || r.bottom > strip.bottom) out.push(`${f.ariaLabel} clipped by the strip`);
        const ext = sheet.querySelector('.ext');
        if (ext && ext.scrollWidth > ext.clientWidth + 1) out.push(`${f.ariaLabel} type cut`);
      }
      return out;
    });
    expect(bad).toEqual([]);
  });

  test('the icon size is changed from the sort sheet and kept', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&view=grid');
    await listed(page);
    const wrap = page.locator('[data-grid-size]');
    await expect(wrap).toHaveAttribute('data-grid-size', 'm');
    await page.locator('button.sort').click();
    await page.getByRole('menuitemradio', { name: 'Icone: Grandi' }).click();
    await expect(wrap).toHaveAttribute('data-grid-size', 'l');
    await page.locator('button.sort').click();
    await expect(page.getByRole('menuitemradio', { name: 'Icone: Grandi' })).toHaveAttribute('aria-checked', 'true');
  });
});

test('a video gets a frame as its thumbnail, with a play mark', async ({ page }) => {
  await boot(page, 'screen=vault&dir=Video&view=grid');
  await listed(page);
  const tile = item(page, 'Compleanno Giulia.webm');
  await expect(tile.getByTestId('video-thumb')).toHaveCount(1, { timeout: 15_000 });
  const img = tile.getByTestId('thumbnail-image');
  await expect(img).toHaveAttribute('src', /^blob:/);
  await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth)).toBeGreaterThan(0);
});

test.describe('phone: Sposta…', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test('moves files into a folder picked on its own page, a new folder made there', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile');
    await listed(page);
    await item(page, 'Copertina.jpg').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Sposta…' }).click();
    const mv = page.getByTestId('mobile-move');
    await expect(mv).toBeVisible();
    // the page fills the screen
    const box = (await mv.boundingBox())!;
    expect(Math.round(box.width)).toBe(390);
    expect(Math.round(box.height)).toBe(844);
    // here is where it already is: nothing to do
    await expect(mv.getByRole('button', { name: 'Sposta qui' })).toBeDisabled();
    await mv.getByRole('button', { name: 'Lavoro', exact: true }).click();
    await expect(mv.getByTestId('move-where')).toHaveText('Lavoro');
    await mv.getByRole('button', { name: 'Nuova cartella' }).click();
    await page.getByRole('dialog', { name: 'Nuova cartella' }).getByRole('textbox').fill('Copertine');
    await page.keyboard.press('Enter');
    await expect(mv.getByTestId('move-where')).toHaveText('Copertine');
    await mv.getByRole('button', { name: 'Sposta qui' }).click();
    await expect(mv).toHaveCount(0);
    await expect(item(page, 'Copertina.jpg')).toHaveCount(0);
    await item(page, 'Lavoro').dblclick();
    await listed(page);
    await item(page, 'Copertine').dblclick();
    await expect(item(page, 'Copertina.jpg')).toBeVisible();
  });
});

test.describe('phone: compact head', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test('search and filters are keys that open what they need', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&dir=Lavoro');
    await listed(page);
    await expect(page.getByTestId('filter-line')).toHaveCount(0);
    await expect(page.getByTestId('search-field')).toHaveCount(0);
    await page.getByTestId('phone-filters').click();
    await expect(page.getByTestId('filter-line')).toBeVisible();
    await page.getByTestId('phone-filters').click();
    await expect(page.getByTestId('filter-line')).toHaveCount(0);
    await page.getByTestId('phone-search').click();
    const field = page.getByRole('searchbox', { name: 'Cerca nel volume' });
    await expect(field).toBeFocused();
    await field.fill('budget');
    await expect(item(page, 'Budget 2025.xlsx')).toBeVisible();
    await page.getByTestId('search-close').click();
    await expect(page.getByTestId('phone-search')).toBeVisible();
  });
});
