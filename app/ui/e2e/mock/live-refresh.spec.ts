// Live refresh: files changed outside the app (through the mounted drive) appear in the
// open folder by themselves; selection and focus survive. Driven by the mock's
// __nv.mock.simulateExternalChange(dir, opts), the stand-in for fs_events.rs.
import type { Page } from '@playwright/test';
import { boot, breadcrumb, expect, expectFolder, fileList, item, listed, modKey, names, nodeId, selectedNames, test } from './helpers';

type Opts = { add?: string[]; remove?: string[]; emit?: boolean };
const external = (page: Page, dir: number, opts: Opts = {}) =>
  page.evaluate(
    ([d, o]) => (window as unknown as { __nv: { mock: { simulateExternalChange(d: number, o: Opts): number[] } } }).__nv.mock.simulateExternalChange(d, o),
    [dir, opts] as const,
  );

test.describe('live refresh from the mounted drive', () => {
  test.beforeEach(async ({ page }) => {
    await boot(page, 'screen=vault&view=list');
    await listed(page);
  });

  test('a file copied into the open folder appears; selection and focus are kept', async ({ page }) => {
    await item(page, 'Leggimi.txt').click();
    await item(page, 'Foto').click({ modifiers: [await modKey(page)] });
    expect((await selectedNames(page)).sort()).toEqual(['Foto', 'Leggimi.txt']);
    const focused = await fileList(page).getAttribute('aria-activedescendant');

    await external(page, 1, { add: ['Da Dolphin.pdf'] });
    await expect(item(page, 'Da Dolphin.pdf')).toBeVisible();
    expect((await selectedNames(page)).sort()).toEqual(['Foto', 'Leggimi.txt']);
    expect(await fileList(page).getAttribute('aria-activedescendant')).toBe(focused);

    await external(page, 1, { remove: ['Leggimi.txt'] });
    await expect(item(page, 'Leggimi.txt')).toHaveCount(0);
    expect(await selectedNames(page)).toEqual(['Foto']);
  });

  test('changes elsewhere are ignored; a change in a crumb of the path is followed', async ({ page }) => {
    const foto = await nodeId(page, 'Foto');
    const lavoro = await nodeId(page, 'Lavoro');
    await item(page, 'Foto').dblclick();
    await expectFolder(page, 'Foto');
    const before = await names(page);

    // the file manager adds to a folder that is not on screen: nothing to do
    await external(page, lavoro, { add: ['altrove.txt'] });
    await page.waitForTimeout(150);
    expect(await names(page)).toEqual(before);

    // the open folder changes
    await external(page, foto, { add: ['Nuova foto.jpg'] });
    await expect(item(page, 'Nuova foto.jpg')).toBeVisible();

    // the open folder is deleted from the file manager: back to its parent
    await external(page, 1, { remove: ['Foto'] });
    await expectFolder(page, 'Documenti');
    await expect(item(page, 'Foto')).toHaveCount(0);
    await expect(breadcrumb(page).getByText('Foto')).toHaveCount(0);
  });
});
