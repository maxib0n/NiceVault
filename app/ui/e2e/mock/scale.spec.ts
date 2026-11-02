import { boot, expect, test } from './helpers';
import { probe } from './layout-probe';
import type { Page } from '@playwright/test';

const at = (w: number, h: number, z: number) => ({ width: Math.round((w * 100) / z), height: Math.round((h * 100) / z), label: `${w}x${h} at ${z}%` });
const SIZES = [...[100, 110, 120, 130].map((z) => at(900, 600, z)), at(1280, 800, 130), at(900, 600, 150)];

async function longUnlock(page: Page) {
  await boot(page, 'screen=welcome&recents=long&error=wrong-password');
  await page.getByText('Dichiarazioni dei redditi', { exact: false }).first().click();
  await expect(page.locator('[data-ready="true"]')).toHaveAttribute('data-screen', 'unlock');
  await page.getByLabel('Password', { exact: true }).fill('sbagliata');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('alert')).toBeVisible();
}

async function mrzClear(page: Page) {
  const r = await page.evaluate(() => {
    const box = (s: string) => document.querySelector(s)!.getBoundingClientRect();
    const mrz = document.querySelector('.unlock .mrz')!;
    const z = mrz.getBoundingClientRect();
    const lines = [...mrz.children].map((l) => l.getBoundingClientRect());
    return { panel: box('.unlock .data-panel').bottom, top: z.top, left: z.left, right: z.right, lines: lines.map((l) => [l.left, l.right]) };
  });
  expect(r.panel, 'data panel above the MRZ').toBeLessThanOrEqual(r.top + 1);
  for (const [l, rt] of r.lines) {
    expect(l).toBeGreaterThanOrEqual(r.left - 1);
    expect(rt).toBeLessThanOrEqual(r.right + 1);
  }
}

test.describe('Sblocca at high scale', () => {
  test.use({ reducedMotion: 'reduce' });
  for (const size of SIZES) {
    test(`long name and place, ${size.label}`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await longUnlock(page);
      await mrzClear(page);
      expect(await probe(page)).toEqual([]);
    });
    test(`removed drive stamp, ${size.label}`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await boot(page, 'screen=unlock&locked=removed');
      await mrzClear(page);
      expect(await probe(page)).toEqual([]);
    });
  }

  test('the MRZ follows a resize (no stale spacing)', async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 480 });
    await longUnlock(page);
    for (const size of [{ width: 1280, height: 800 }, { width: 900, height: 600 }, { width: 1400, height: 880 }]) {
      await page.setViewportSize({ width: size.width, height: size.height });
      await mrzClear(page);
    }
  });
});

test.describe('fragile screens at high scale', () => {
  test.use({ reducedMotion: 'reduce' });
  const SCREENS = ['screen=welcome&recents=long', 'screen=vault', 'screen=vault&view=list', 'screen=passwords', 'screen=settings', 'screen=details', 'screen=create&step=2'];
  for (const q of SCREENS) {
    for (const size of [SIZES[0], SIZES[3], SIZES[5]]) {
      test(`${q} at ${size.label}`, async ({ page }) => {
        await page.setViewportSize({ width: size.width, height: size.height });
        await boot(page, q);
        expect(await probe(page)).toEqual([]);
      });
    }
  }
});

test.describe('own title bar', () => {
  test('Linux / Windows: the strip with minimise, maximise, close; the screen under it', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 600 });
    await boot(page, 'screen=unlock&chrome=linux');
    const bar = page.getByTestId('titlebar');
    const controls = bar.getByRole('group', { name: 'Controlli della finestra' });
    await expect(controls.getByRole('button')).toHaveCount(3);
    await controls.getByRole('button', { name: 'Ingrandisci la finestra' }).click();
    await expect(controls.getByRole('button', { name: 'Ripristina la finestra' })).toBeVisible();
    await bar.dblclick({ position: { x: 200, y: 10 } });
    await expect(controls.getByRole('button', { name: 'Ingrandisci la finestra' })).toBeVisible();
    const [b, top] = await Promise.all([bar.boundingBox(), page.getByRole('navigation').first().boundingBox()]);
    expect(top!.y).toBeGreaterThanOrEqual(b!.y + b!.height);
    expect(await probe(page)).toEqual([]);
  });
  test('macOS: room for the traffic lights, no buttons of ours', async ({ page }) => {
    await boot(page, 'screen=vault&chrome=macos');
    const bar = page.getByTestId('titlebar');
    await expect(bar).toBeVisible();
    await expect(bar.getByRole('button')).toHaveCount(0);
  });
  test('no strip in a browser or on a phone', async ({ page }) => {
    await boot(page, 'screen=vault');
    await expect(page.getByTestId('titlebar')).toHaveCount(0);
    await boot(page, 'screen=vault&chrome=linux&platform=mobile');
    await expect(page.getByTestId('titlebar')).toHaveCount(0);
  });
});
