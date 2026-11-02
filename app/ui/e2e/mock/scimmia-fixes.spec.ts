import type { Locator } from '@playwright/test';
import { boot, expect, item, listed, test } from './helpers';

const roomy = (name: Locator) => name.evaluate((el) => el.getBoundingClientRect().width > 75 && el.scrollHeight <= el.clientHeight + 1);

test('a narrow list drops the type, then the date, so names keep their room', async ({ page }) => {
  await page.setViewportSize({ width: 800, height: 560 });
  await boot(page, 'screen=vault&view=list&dir=Lavoro');
  await listed(page);
  const row = item(page, 'Fattura 2024-031.pdf');
  const name = row.getByTestId('item-name');
  await expect(row.locator('.type')).toBeHidden();
  await expect(page.locator('.list-header [data-col=type]')).toBeHidden();
  expect(await roomy(name)).toBe(true);
  await page.setViewportSize({ width: 640, height: 560 });
  await expect(row.locator('.mtime')).toBeHidden();
  expect(await roomy(name)).toBe(true);
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(row.locator('.type')).toBeVisible();
  await expect(row.locator('.mtime')).toBeVisible();
});
