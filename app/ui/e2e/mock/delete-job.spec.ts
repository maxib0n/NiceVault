// "616 elementi, 1,1 GB — sembra non fare nulla": a delete is a job (src-tauri delete.rs). The
// confirmation stays; then the tray shows items and space freed, the listing follows each committed
// batch, Annulla stops it (what went stays gone), the end is a toast, a full volume says so.
// Mock: ?slow_delete=<ms per batch of 128>, ?error=delete.
import type { Page } from '@playwright/test';
import { boot, expect, item, listed, test } from './helpers';

const N = 600;

/** A folder "Tanti" of N files in the root; with `inside`, opened. */
async function bigFolder(page: Page, query: string, inside = false) {
  await boot(page, `screen=vault&view=list&${query}`);
  await listed(page);
  await page.evaluate(async (n) => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const mock = (window as any).__nv.mock;
    const [v] = await mock.invoke('vault_list_open');
    const d = await mock.invoke('fs_mkdir', { vault: v.id, parent: 1, name: 'Tanti' });
    mock.simulateExternalChange(d.id, { vault: v.id, add: Array.from({ length: n }, (_, i) => `foto ${String(i).padStart(3, '0')}.jpg`), emit: false });
    mock.simulateExternalChange(1, { vault: v.id, add: [] });
  }, N);
  await expect(item(page, 'Tanti')).toBeVisible();
  if (inside) {
    await item(page, 'Tanti').dblclick();
    await expect(page.getByText(`${N} elementi`).first()).toBeVisible();
  }
}

async function confirmDelete(page: Page, title: RegExp | string) {
  await page.keyboard.press('Delete');
  const dlg = page.getByRole('dialog', { name: title });
  await expect(dlg).toBeVisible();
  await dlg.getByRole('button', { name: 'Elimina' }).click();
  await expect(dlg).toHaveCount(0);
}

const tray = (page: Page) => page.getByRole('region', { name: 'Trasferimenti' });

test('a long delete shows in the tray with items and space freed, and ends in a toast', async ({ page }) => {
  await bigFolder(page, 'slow_delete=200');
  await item(page, 'Tanti').click();
  await confirmDelete(page, 'Eliminare “Tanti”?');
  await expect(tray(page)).toBeVisible();
  await expect(tray(page)).toContainText(/Eliminazione · \d+%/);
  await tray(page).getByRole('button', { expanded: false }).click();
  await expect(tray(page)).toContainText(new RegExp(`\\d+ di ${N + 1} elementi · .* liberati`));
  await expect(page.getByRole('status').filter({ hasText: `Eliminati ${N + 1} elementi.` })).toBeVisible({ timeout: 15_000 });
  await expect(item(page, 'Tanti')).toHaveCount(0);
  await expect(tray(page)).toHaveCount(0);
});

test('the listing follows the committed batches; Annulla stops it and keeps what is left', async ({ page }) => {
  await bigFolder(page, 'slow_delete=600', true);
  await page.keyboard.press('ControlOrMeta+a');
  await confirmDelete(page, `Eliminare ${N} elementi?`);
  // after the first batch, fewer items but not none, while the job still runs
  await expect(page.getByText(new RegExp(`^${N - 128} elementi$`)).first()).toBeVisible({ timeout: 10_000 });
  await expect(tray(page)).toContainText(/Eliminazione/);
  await tray(page).getByRole('button', { expanded: false }).click();
  await tray(page).getByRole('button', { name: `Annulla ${N} elementi` }).click();
  await expect(page.getByRole('status').filter({ hasText: /Eliminazione annullata: eliminati \d+ di 600\./ })).toBeVisible();
  await expect(tray(page)).toHaveCount(0);
  const left = Number((await page.getByText(/^\d+ elementi$/).first().textContent())!.split(' ')[0]);
  expect(left).toBeGreaterThan(0);
  expect(left).toBeLessThan(N);
  // nothing more goes after the cancel
  await page.waitForTimeout(1300);
  await expect(page.getByText(`${left} elementi`).first()).toBeVisible();
});

test('a quick delete is just its toast (no tray)', async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
  await item(page, 'Leggimi.txt').click();
  await confirmDelete(page, 'Eliminare “Leggimi.txt”?');
  await expect(page.getByRole('status').filter({ hasText: 'Eliminato.' })).toBeVisible();
  await expect(item(page, 'Leggimi.txt')).toHaveCount(0);
  await expect(tray(page)).toHaveCount(0);
});

test('a full volume stops the delete with a clear message (Estendi)', async ({ page }) => {
  await bigFolder(page, 'slow_delete=60&error=delete');
  await item(page, 'Tanti').click();
  await confirmDelete(page, 'Eliminare “Tanti”?');
  await expect(page.getByRole('alert').filter({ hasText: /Non c’è spazio per finire l’eliminazione.*Estendi/ })).toBeVisible({ timeout: 10_000 });
  // the tray keeps the failed job, the part already deleted is gone from the folder
  await expect(tray(page)).toContainText(/non riuscit/i);
  await item(page, 'Tanti').dblclick();
  const left = Number((await page.getByText(/^\d+ elementi$/).first().textContent())!.split(' ')[0]);
  expect(left).toBeLessThan(N);
});

test.describe('phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

  test('a long delete: the slip while it runs, a toast at the end and no slip left', async ({ page }) => {
    await bigFolder(page, 'platform=mobile&slow_delete=250');
    await item(page, 'Tanti').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Elimina' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Elimina' }).click();
    await expect(tray(page)).toBeVisible();
    await expect(page.getByRole('status').filter({ hasText: `Eliminati ${N + 1} elementi.` })).toBeVisible({ timeout: 15_000 });
    await expect(tray(page)).toHaveCount(0);
    await expect(item(page, 'Tanti')).toHaveCount(0);
  });
});
