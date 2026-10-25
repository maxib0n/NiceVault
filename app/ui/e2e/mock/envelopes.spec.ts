// Folder envelopes (FolderPeek): whatever a folder holds (files only, folders only,
// mixed, many, long names, nothing), its tile shows a tidy envelope with at most
// three small pieces peeking out, all inside the tile. Geometry sanity check via
// data-* hooks and bounding boxes, not pixels.
import { boot, expect, fileList, listed, test } from './helpers';
import type { Page } from '@playwright/test';

const DIR = 'screen=vault&view=grid&dir=Personale/Buste';
const INKS = ['velluto', 'prugna', 'prussia', 'bottiglia', 'seppia', 'ardesia', 'ottanio'];

/** expected pieces per folder (print / sheet / envelope, in that order) */
const EXPECT: Record<string, string[]> = {
  'Solo file': ['sheet', 'sheet', 'sheet'],
  'Solo cartelle': ['envelope', 'envelope', 'envelope'],
  Misto: ['sheet', 'sheet', 'sheet'],
  'Un file': ['sheet'],
  'Una cartella': ['envelope'],
  'Due cartelle': ['envelope', 'envelope'],
  'Un file e cartelle': ['sheet', 'envelope', 'envelope'],
  'Foto e cartelle': ['print', 'envelope', 'envelope'],
  'Molti elementi': ['sheet', 'sheet', 'sheet'],
  'Due file': ['sheet', 'sheet'],
  'Una cartella dal nome lunghissimo che non sta in nessuna riga della griglia e va troncata': ['sheet', 'envelope'],
  Vuota: [],
};

async function checkGrid(page: Page) {
  await listed(page);
  const tiles = fileList(page).locator('[role="option"][data-kind="dir"]');
  await expect(tiles).toHaveCount(12);
  // the listings behind the peeks load asynchronously
  await expect(fileList(page).locator('[data-peek]')).toHaveCount(Object.values(EXPECT).flat().length);

  const sizes = new Set<string>();
  for (let i = 0; i < 12; i++) {
    const tile = tiles.nth(i);
    const name = (await tile.getByTestId('item-name').textContent())!.trim();
    const env = tile.locator('[data-envelope]');
    const visual = (await env.boundingBox())!;
    const box = (await tile.boundingBox())!;
    sizes.add(`${Math.round(visual.width)}x${Math.round(visual.height)}`);
    const peeks = tile.locator('[data-peek]');
    expect(await peeks.evaluateAll((els) => els.map((e) => e.getAttribute('data-peek'))), name).toEqual(EXPECT[name]);
    for (const b of await peeks.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON() as DOMRect))) {
      const where = `${name}: peek ${JSON.stringify(b)} vs tile ${JSON.stringify(box)} / envelope ${JSON.stringify(visual)}`;
      // inside the tile, and inside the envelope's picture box (plus a hair for rotation/shadows)
      expect(b.left, where).toBeGreaterThanOrEqual(box.x - 1);
      expect(b.right, where).toBeLessThanOrEqual(box.x + box.width + 1);
      expect(b.top, where).toBeGreaterThanOrEqual(visual.y - 4);
      expect(b.bottom, where).toBeLessThanOrEqual(visual.y + visual.height);
      // a small piece, never a sheet towering over the envelope
      expect(b.height, where).toBeLessThan(visual.height * 0.75);
      expect(b.width, where).toBeLessThan(visual.width * 0.8);
    }
  }
  // every folder tile has the same picture box
  expect([...sizes], 'folder tiles differ in size').toHaveLength(1);
}

for (const width of [900, 1280, 1680]) {
  for (const theme of ['dark', 'light']) {
    test(`envelopes stay tidy (${width}px, ${theme})`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await boot(page, `${DIR}&theme=${theme}`);
      await checkGrid(page);
    });
  }
}

test('envelopes stay tidy in every ink', async ({ page }) => {
  test.slow(); // seven boots in a row
  for (const ink of INKS) {
    await boot(page, `${DIR}&accent=${ink}`);
    await checkGrid(page);
  }
});
