// View sizes: grid tiles S/M/L/XL and list density, from the toolbar, Ctrl+= / Ctrl+- / Ctrl+0
// and Ctrl+wheel; remembered per volume (Settings.grid_size / list_density, inside the volume);
// resizing never plays the glyph decode again.
import type { Page } from '@playwright/test';
import { boot, expect, expectScreen, fileList, item, listed, modKey, test } from './helpers';

const wrap = (page: Page) => page.locator('.file-view-wrap');
const tileWidth = async (page: Page) => (await item(page, 'Foto').boundingBox())!.width;
const rowHeight = async (page: Page) => (await item(page, 'Foto').boundingBox())!.height;
const plays = (page: Page) => page.evaluate(() => ((window as unknown as { __nvReveal?: { what: string }[] }).__nvReveal ?? []).filter((e) => e.what === 'play').length);

test('grid: toolbar sizes, shortcuts, clamped, back to Medie', async ({ page }) => {
  await boot(page, 'screen=vault&view=grid');
  await listed(page);
  const sizes = page.getByRole('radiogroup', { name: 'Dimensione delle icone' });
  await expect(sizes.getByRole('radio')).toHaveCount(4);
  await expect(sizes.getByRole('radio', { name: 'Medie' })).toHaveAttribute('aria-checked', 'true');
  const m = await tileWidth(page);
  // the folder's first decode is done, and the thumbnails' after it (reveal.ts: they wait for it)
  await expect
    .poll(() =>
      page.evaluate(() => {
        const log = (window as unknown as { __nvReveal?: { t: number; kind: string; key: string; what: string }[] }).__nvReveal ?? [];
        const open = log.filter((e) => e.kind === 'decode' && e.what === 'play').some((p) => !log.some((d) => d.key === p.key && d.what === 'done'));
        return !open && performance.now() - (log[log.length - 1]?.t ?? 0) > 500;
      }),
    )
    .toBe(true);
  const before = await plays(page);

  await sizes.getByRole('radio', { name: 'Molto grandi' }).click();
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'xl');
  expect(await tileWidth(page)).toBeGreaterThan(m * 1.5);

  const mod = await modKey(page);
  await fileList(page).focus();
  await page.keyboard.press(`${mod}+=`); // already the largest: stays
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'xl');
  await page.keyboard.press(`${mod}+-`);
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'l');
  await page.keyboard.press(`${mod}+-`);
  await page.keyboard.press(`${mod}+-`);
  await page.keyboard.press(`${mod}+-`);
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 's');
  expect(await tileWidth(page)).toBeLessThan(m);
  await page.keyboard.press(`${mod}+0`);
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'm');
  await expect(sizes.getByRole('radio', { name: 'Medie' })).toHaveAttribute('aria-checked', 'true');

  // arrows inside the radiogroup
  await sizes.getByRole('radio', { name: 'Medie' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'l');
  await expect(sizes.getByRole('radio', { name: 'Grandi', exact: true })).toBeFocused();

  // no glyph decode on resize
  expect(await plays(page)).toBe(before);
});

test('Ctrl+wheel over the files changes the size; plain wheel scrolls', async ({ page }) => {
  await boot(page, 'screen=vault&view=grid');
  await listed(page);
  const box = (await fileList(page).boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const mod = await modKey(page);
  await page.keyboard.down(mod);
  await page.mouse.wheel(0, -120);
  await page.keyboard.up(mod);
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'l');
  await page.keyboard.down(mod);
  await page.mouse.wheel(0, 120);
  await page.waitForTimeout(450);
  await page.mouse.wheel(0, 120);
  await page.keyboard.up(mod);
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 's');
  await page.mouse.wheel(0, 120);
  await page.waitForTimeout(200);
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 's');
});

test('list: density radios and shortcuts change the row height', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  const dens = page.getByRole('radiogroup', { name: 'Densità delle righe' });
  await expect(dens.getByRole('radio', { name: 'Normale' })).toHaveAttribute('aria-checked', 'true');
  const normal = await rowHeight(page);
  await dens.getByRole('radio', { name: 'Compatta' }).click();
  await expect(wrap(page)).toHaveAttribute('data-density', 'compact');
  expect(await rowHeight(page)).toBeLessThan(normal);
  await fileList(page).focus();
  await page.keyboard.press(`${await modKey(page)}+=`);
  await page.keyboard.press(`${await modKey(page)}+=`);
  await expect(wrap(page)).toHaveAttribute('data-density', 'comfortable');
  expect(await rowHeight(page)).toBeGreaterThan(normal);
  // the grid keeps its own size
  await page.getByRole('button', { name: 'Griglia' }).click();
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'm');
});

test('remembered per volume: kept across lock + unlock, another volume has its own', async ({ page }) => {
  await boot(page, 'screen=vault&view=grid');
  await listed(page);
  await page.getByRole('radio', { name: 'Grandi', exact: true }).click();
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'l');

  // another volume: its own (default) size
  await page.getByRole('button', { name: 'Apri un altro volume' }).click();
  await expectScreen(page, 'welcome');
  await page.getByRole('button', { name: /^(Chiuso|Aperto) Archivio fiscale/ }).click();
  await page.getByLabel('Password', { exact: true }).fill('demo');
  await page.keyboard.press('Enter');
  await expectScreen(page, 'vault');
  await expect(item(page, '730 2023.pdf')).toBeVisible();
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'm');
  await page.getByRole('button', { name: 'Blocca' }).click();

  // back on Documenti, then lock and unlock it: still Grandi
  await expect(item(page, 'Foto')).toBeVisible();
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'l');
  await page.getByRole('button', { name: 'Blocca' }).click();
  await expectScreen(page, 'unlock');
  await page.getByLabel('Password', { exact: true }).fill('demo');
  await page.keyboard.press('Enter');
  await expectScreen(page, 'vault');
  await listed(page);
  await expect(wrap(page)).toHaveAttribute('data-grid-size', 'l');
});

test('English labels', async ({ page }) => {
  await boot(page, 'screen=vault&view=grid&lang=en');
  const sizes = page.getByRole('radiogroup', { name: 'Icon size' });
  await expect(sizes.getByRole('radio')).toHaveText(['', '', '', '']);
  await expect(sizes.getByRole('radio', { name: 'Extra large' })).toHaveAttribute('title', /also (Ctrl|⌘)\+wheel/);
});
