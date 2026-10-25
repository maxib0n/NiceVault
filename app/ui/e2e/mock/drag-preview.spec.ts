// The picture under the pointer while dragging files: a compact preview (lib/dragPreview.ts)
// handed to dataTransfer.setDragImage, never the browser's snapshot of the whole tile / row.
// One item: its picture + its name; several: up to 5 pictures fanned + a count.
import type { Page } from '@playwright/test';
import { boot, expect, item, listed, modKey, test } from './helpers';

/** Starts a drag on `name` and returns what setDragImage received. */
async function dragImageOf(page: Page, name: string) {
  return item(page, name).evaluate((el) => {
    const calls: { tag: string; testid?: string; count?: string; w: number; h: number; x: number; y: number; cards: number; label: string | null }[] = [];
    const orig = DataTransfer.prototype.setDragImage;
    DataTransfer.prototype.setDragImage = function (img: Element, x: number, y: number) {
      const r = img.getBoundingClientRect();
      const e = img as HTMLElement;
      calls.push({ tag: e.tagName, testid: e.dataset.testid, count: e.dataset.count, w: r.width, h: r.height, x, y, cards: e.querySelectorAll('.drag-card').length, label: e.querySelector('.label')?.textContent ?? null });
      return orig.call(this, img, x, y);
    };
    el.dispatchEvent(new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() }));
    DataTransfer.prototype.setDragImage = orig;
    return calls;
  });
}

for (const view of ['grid', 'list'] as const) {
  test(`${view}: one item drags as a small picture with its name`, async ({ page }) => {
    await boot(page, `screen=vault&view=${view}`);
    await listed(page);
    const calls = await dragImageOf(page, 'Foto');
    expect(calls).toHaveLength(1);
    const c = calls[0];
    expect(c.testid).toBe('drag-preview');
    expect(c.count).toBe('1');
    expect(c.cards).toBe(1);
    expect(c.label).toBe('Foto');
    expect(c.w).toBeLessThan(220);
    expect(c.h).toBeLessThan(110);
    expect(c.x).toBeGreaterThan(0);
  });
}

test('several items: fanned pictures (at most 5) and the count', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await page.keyboard.press(`${await modKey(page)}+a`);
  const total = await page.getByRole('option', { selected: true }).count();
  expect(total).toBeGreaterThan(5);
  const [c] = await dragImageOf(page, 'Foto');
  expect(c.count).toBe(String(total));
  expect(c.cards).toBe(5);
  expect(c.label).toBeNull();
  expect(c.w).toBeLessThan(160);
  // the preview is gone again shortly after the drag started
  await expect(page.getByTestId('drag-preview')).toHaveCount(0, { timeout: 4000 });
});
