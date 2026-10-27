import { boot, expect, fileList, item, listed, selectedNames, test } from './helpers';

const dialog = (page: import('@playwright/test').Page, name: string) => page.getByRole('dialog', { name: `Anteprima di ${name}` });

test.describe('full-screen preview (desktop)', () => {
  test.beforeEach(async ({ page }) => {
    await boot(page, 'screen=vault');
    await listed(page);
  });

  test('Schermo intero: dock, auto-hide, keys, Esc back to the window-sized preview', async ({ page }) => {
    await item(page, 'Copertina.jpg').dblclick();
    const ql = dialog(page, 'Copertina.jpg');
    await expect(ql).toBeVisible();
    await expect(page.getByTestId('ql-dock')).toHaveCount(0);
    await ql.getByTestId('ql-fullscreen').click();
    await expect(page.locator('dialog[data-full="true"]')).toHaveCount(1);
    const dock = page.getByTestId('ql-dock');
    await expect(dock).toHaveAttribute('data-shown', 'true');
    await expect(dock.getByRole('button', { name: 'Copertina.jpg' })).toHaveAttribute('aria-current', 'true');
    await expect(dock).toHaveAttribute('data-shown', 'false', { timeout: 5000 });
    await page.mouse.move(400, 600);
    await page.mouse.move(420, 760);
    await expect(dock).toHaveAttribute('data-shown', 'true');
    await page.keyboard.press('ArrowRight');
    await expect(dialog(page, 'Foto profilo.jpg')).toBeVisible();
    await expect(dock.getByRole('button', { name: 'Foto profilo.jpg' })).toHaveAttribute('aria-current', 'true');
    await page.mouse.move(640, 700);
    await page.mouse.move(640, 770);
    await expect(dock).toHaveAttribute('data-shown', 'true');
    await dock.getByRole('button', { name: 'Copertina.jpg' }).click();
    await expect(dialog(page, 'Copertina.jpg')).toBeVisible();
    expect(await selectedNames(page)).toEqual(['Copertina.jpg']);
    await page.keyboard.press('Escape');
    await expect(page.locator('dialog[data-full="true"]')).toHaveCount(0);
    await expect(dialog(page, 'Copertina.jpg')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
    await expect(fileList(page)).toBeFocused();
  });

  test('a long document scrolls in full screen; the scroll never brings the chrome back', async ({ page }) => {
    for (const [dir, name] of [
      ['Personale/Anteprime', 'server.log'],
      ['Lavoro', 'Contratto affitto.pdf'],
    ]) {
      await boot(page, `screen=vault&dir=${dir}&preview=${name}`);
      const ql = page.getByRole('dialog', { name: /^Anteprima di/ });
      await expect(ql).toBeVisible();
      await page.keyboard.press('f');
      await expect(page.locator('dialog[data-full="true"]')).toHaveCount(1);
      const scroller = page.getByTestId('viewer-scroll');
      await expect(scroller).toBeVisible();
      await expect.poll(() => scroller.evaluate((el) => el.scrollHeight > el.clientHeight + 40)).toBe(true);
      await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1, { timeout: 5000 });
      const box = (await scroller.boundingBox())!;
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.move(box.x + box.width / 2 + 4, box.y + box.height / 2 + 4);
      await page.mouse.wheel(0, 600);
      await expect.poll(() => scroller.evaluate((el) => el.scrollTop)).toBeGreaterThan(100);
      await page.keyboard.press('PageDown');
      await page.waitForTimeout(300);
      await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1);
    }
  });

  test('F toggles full screen', async ({ page }) => {
    await item(page, 'Copertina.jpg').dblclick();
    await expect(dialog(page, 'Copertina.jpg')).toBeVisible();
    await page.keyboard.press('f');
    await expect(page.locator('dialog[data-full="true"]')).toHaveCount(1);
    await page.keyboard.press('f');
    await expect(page.locator('dialog[data-full="true"]')).toHaveCount(0);
  });
});

test.describe('phone layout', () => {
  test.use({ viewport: { width: 412, height: 915 }, hasTouch: true });

  test('the preview is full screen; swipe steps, swipe down closes', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile');
    await listed(page);
    await expect(page.getByTestId('nav-passwords')).toBeVisible();
    await item(page, 'Copertina.jpg').dblclick();
    await expect(dialog(page, 'Copertina.jpg')).toBeVisible();
    await expect(page.locator('dialog[data-full="true"]')).toHaveCount(1);
    await expect(page.getByTestId('ql-fullscreen')).toHaveCount(0);
    const body = page.locator('dialog[data-full="true"] .ql-body');
    const box = (await body.boundingBox())!;
    const stroke = async (dx: number, dy: number) => {
      const x = box.x + box.width / 2;
      const y = box.y + box.height / 2;
      await body.dispatchEvent('pointerdown', { pointerId: 7, pointerType: 'touch', clientX: x, clientY: y, isPrimary: true });
      await body.dispatchEvent('pointermove', { pointerId: 7, pointerType: 'touch', clientX: x + dx / 2, clientY: y + dy / 2, isPrimary: true });
      await body.dispatchEvent('pointerup', { pointerId: 7, pointerType: 'touch', clientX: x + dx, clientY: y + dy, isPrimary: true });
    };
    await stroke(-160, 4);
    await expect(dialog(page, 'Foto profilo.jpg')).toBeVisible();
    await stroke(160, -4);
    await expect(dialog(page, 'Copertina.jpg')).toBeVisible();
    await stroke(0, 220);
    await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
  });

  test('a long document: a vertical stroke is its scroll, not a swipe; the chrome stays away', async ({ page }) => {
    for (const [dir, name] of [
      ['Personale/Anteprime', 'server.log'],
      ['Lavoro', 'Contratto affitto.pdf'],
    ]) {
      await boot(page, `screen=vault&dir=${dir}&preview=${name}&platform=mobile`);
      const scroller = page.getByTestId('viewer-scroll');
      await expect(scroller).toBeVisible();
      await expect.poll(() => scroller.evaluate((el) => el.scrollHeight > el.clientHeight + 40)).toBe(true);
      expect(
        await scroller.evaluate((el) => {
          const out: string[] = [];
          for (let n: Element | null = el; n; n = n.parentElement) out.push(getComputedStyle(n).touchAction);
          return out.every((t) => t === 'auto' || t.includes('pan-y') || t === 'manipulation');
        }),
      ).toBe(true);
      await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1, { timeout: 5000 });
      const box = (await scroller.boundingBox())!;
      const x = box.x + box.width / 2;
      const y = box.y + box.height / 2;
      await scroller.evaluate((el) => (el.scrollTop = 400));
      for (const dy of [-300, 260]) {
        await scroller.dispatchEvent('pointerdown', { pointerId: 9, pointerType: 'touch', clientX: x, clientY: y, isPrimary: true });
        for (let k = 1; k <= 4; k++) await scroller.dispatchEvent('pointermove', { pointerId: 9, pointerType: 'touch', clientX: x + k, clientY: y + (dy * k) / 4, isPrimary: true });
        await scroller.dispatchEvent('pointerup', { pointerId: 9, pointerType: 'touch', clientX: x + 4, clientY: y + dy, isPrimary: true });
        await page.waitForTimeout(250);
        await expect(page.getByRole('dialog', { name: `Anteprima di ${name}` })).toBeVisible();
        await expect(page.locator('dialog[data-chrome="hidden"]')).toHaveCount(1);
      }
    }
  });

  test('Password: the list, then the entry with Indietro', async ({ page }) => {
    await boot(page, 'screen=passwords&platform=mobile');
    const back = page.getByTestId('mobile-back');
    await expect(page.getByTestId('pm-entry').first()).toBeVisible();
    await expect(back).toHaveCount(0);
    await page.getByTestId('pm-entry').first().click();
    await expect(back).toBeVisible();
    await expect(page.getByTestId('pm-entry').first()).toBeHidden();
    await back.click();
    await expect(page.getByTestId('pm-entry').first()).toBeVisible();
    await expect(back).toHaveCount(0);
  });
});

test.describe('preview motion', () => {
  test.beforeEach(async ({ page }) => {
    await boot(page, 'screen=vault');
    await listed(page);
  });

  test('the zoom bar rides over the dock and glides down when it hides; closing eases out', async ({ page }) => {
    await item(page, 'Copertina.jpg').dblclick();
    const ql = dialog(page, 'Copertina.jpg');
    await expect(ql).toBeVisible();
    await ql.getByTestId('ql-fullscreen').click();
    const dock = page.getByTestId('ql-dock');
    const bar = ql.locator('.vw-bar');
    const dy = () => bar.evaluate((e) => getComputedStyle(e).translate);
    await page.mouse.move(400, 600);
    await page.mouse.move(420, 760);
    await expect(dock).toHaveAttribute('data-shown', 'true');
    await expect.poll(dy).toMatch(/-\d/);
    await expect(dock).toHaveAttribute('data-shown', 'false', { timeout: 5000 });
    await expect.poll(dy).toMatch(/^(none|0px( 0px)?)$/);
    await expect(bar).toBeVisible();
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
  });

  test('reduced motion: closing and full screen are instant', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await item(page, 'Copertina.jpg').dblclick();
    const ql = dialog(page, 'Copertina.jpg');
    await expect(ql).toBeVisible();
    await ql.getByTestId('ql-fullscreen').click();
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    expect(await page.getByRole('dialog', { name: /^Anteprima di/ }).count()).toBe(0);
  });
});
