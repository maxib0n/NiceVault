import type { Page } from '@playwright/test';
import { boot, expect, expectFolder, fileList, item, listed, test } from './helpers';

const N = 400;
const name = (i: number) => `nota ${String(i).padStart(3, '0')}.txt`;

async function openBigFolder(page: Page) {
  await boot(page, 'screen=vault');
  await listed(page);
  await page.evaluate(async (n) => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const mock = (window as any).__nv.mock;
    mock.opts.speed = 0;
    const [v] = await mock.invoke('vault_list_open');
    const d = await mock.invoke('fs_mkdir', { vault: v.id, parent: 1, name: 'Grande' });
    await mock.invoke('fs_import', { vault: v.id, parent: d.id, host_paths: Array.from({ length: n }, (_, i) => `/host/nota ${String(i + 1).padStart(3, '0')}.txt`) });
    mock.opts.speed = 1;
    mock.simulateExternalChange(1, { vault: v.id });
  }, N);
  await item(page, 'Grande').dblclick();
  await expectFolder(page, 'Grande');
  await expect(page.getByText(`${N} elementi`).first()).toBeVisible();
}

for (const layout of ['grid', 'list'] as const) {
  test(`${layout}: a folder of ${N} renders only the rows in view; End, Home and the scrollbar reach every row`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openBigFolder(page);
    if (layout === 'list') {
      await page.getByRole('button', { name: 'Elenco' }).click();
      await expect(page.getByRole('button', { name: 'Elenco' })).toHaveAttribute('aria-pressed', 'true');
    }
    const options = fileList(page).getByRole('option');
    await expect(item(page, name(1))).toBeVisible();
    const rendered = await options.count();
    expect(rendered).toBeGreaterThan(5);
    expect(rendered).toBeLessThan(150);

    await item(page, name(1)).click();
    await page.keyboard.press('End');
    await expect(item(page, name(N))).toBeInViewport();
    await expect(item(page, name(N))).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Home');
    await expect(item(page, name(1))).toBeInViewport();

    await fileList(page).evaluate((el) => (el.scrollTop = el.scrollHeight));
    await expect(item(page, name(N))).toBeInViewport();
    expect(await options.count()).toBeLessThan(150);
    await fileList(page).evaluate((el) => (el.scrollTop = el.scrollHeight / 2 - el.clientHeight / 2));
    await expect(item(page, name(N / 2))).toBeAttached();
    await expect(item(page, name(1))).toHaveCount(0);
  });
}

test('Impostazioni opens over the volume: Indietro shows the same page, not a new one', async ({ page }) => {
  await boot(page, 'screen=vault');
  await listed(page);
  await item(page, 'Foto').click();
  await fileList(page).evaluate((el) => ((el as unknown as { __mark: number }).__mark = 1));
  await page.keyboard.press('Control+,');
  await expect(page.getByTestId('settings')).toBeVisible();
  await expect(fileList(page)).toBeHidden();
  await page.getByTestId('settings-back').click();
  await expect(fileList(page)).toBeVisible();
  expect(await fileList(page).evaluate((el) => (el as unknown as { __mark?: number }).__mark)).toBe(1);
  await expect(item(page, 'Foto')).toHaveAttribute('aria-selected', 'true');
});
