import { boot, expect, fileList, item, listed, test } from './helpers';

const N = 240;
const name = (i: number) => `nota ${String(i).padStart(3, '0')}.txt`;

test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

test(`phone list of ${N}: the first rows are on screen at the top, and after scrolling back`, async ({ page }) => {
  await boot(page, 'screen=vault&platform=mobile&view=list');
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
  await item(page, 'Grande').click();
  await listed(page);
  await expect(fileList(page)).toHaveAttribute('aria-label', /Grande/);

  const scroller = fileList(page);
  const firstOnTop = async () => {
    const box = (await item(page, name(1)).boundingBox())!;
    const view = (await scroller.boundingBox())!;
    expect(box, 'first row rendered').not.toBeNull();
    expect(box.y).toBeGreaterThanOrEqual(view.y - 1);
    expect(box.y).toBeLessThan(view.y + 120);
  };
  await expect(item(page, name(1))).toBeVisible();
  await expect(async () => {
    expect(await scroller.evaluate((el) => el.scrollTop)).toBe(0);
    await firstOnTop();
    await expect(item(page, name(2))).toBeVisible();
    await expect(item(page, name(5))).toBeInViewport();
  }).toPass();

  await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight / 2));
  await expect(item(page, name(N / 2 + 2))).toBeInViewport();
  await scroller.evaluate((el) => (el.scrollTop = 0));
  await expect(async () => {
    await expect(item(page, name(1))).toBeInViewport();
    await expect(item(page, name(4))).toBeInViewport();
    await firstOnTop();
  }).toPass();

  await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight - el.clientHeight));
  await expect(item(page, name(N))).toBeInViewport();
  await page.getByRole('button', { name: 'Cartella superiore' }).first().click();
  await listed(page);
  await item(page, 'Grande').click();
  await listed(page);
  await expect(async () => {
    await expect(item(page, name(1))).toBeInViewport();
    await expect(item(page, name(4))).toBeInViewport();
    await firstOnTop();
  }).toPass();
});
