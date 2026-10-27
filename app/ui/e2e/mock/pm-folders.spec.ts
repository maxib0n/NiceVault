import type { Page } from '@playwright/test';
import { boot, expect, test } from './helpers';

const entry = (page: Page, title: string) => page.locator(`[data-testid="pm-entry"][data-title="${title}"]`);
const folder = (page: Page, f: string) => page.locator(`[data-filter="folder"][data-folder="${f}"]`);
const count = async (page: Page, f: string) => Number(await folder(page, f).locator('.f-n').textContent());
const mod = process.platform === 'darwin' ? 'Meta' : 'Control';

test('context menu: move to a new folder through the picker', async ({ page }) => {
  await boot(page, 'screen=passwords&entry=SPID');
  await entry(page, 'Film in streaming').click({ button: 'right' });
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: 'Sposta in cartella…' }).click();
  const dlg = page.getByTestId('pm-move');
  const input = dlg.getByRole('combobox');
  await expect(input).toBeFocused();
  await expect(dlg.getByRole('option')).toHaveCount(5);
  await input.fill('Svago/Film');
  await expect(dlg.locator('[data-new="Svago/Film"]')).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('Enter');
  await expect(dlg).toHaveCount(0);
  await expect(folder(page, 'Svago/Film')).toBeVisible();
  await expect.poll(() => count(page, 'Svago')).toBe(1);
  await folder(page, 'Svago/Film').click();
  await expect(page.getByTestId('pm-entry')).toHaveCount(1);
  await expect(page.getByText('1 voce spostata in Svago › Film.')).toBeVisible();
});

test('keyboard: pick several, Ctrl+Shift+M, search the picker, Enter', async ({ page }) => {
  await boot(page, 'screen=passwords&entry=Film in streaming');
  await page.getByTestId('pm-list').focus();
  await page.keyboard.press('Shift+ArrowDown');
  await expect(page.getByTestId('pm-multi')).toHaveAttribute('data-count', '2');
  await page.keyboard.press(`${mod}+Shift+m`);
  const dlg = page.getByTestId('pm-move');
  await page.keyboard.type('banc');
  await expect(dlg.getByRole('option')).toHaveCount(2);
  await expect(dlg.locator('[data-folder="Personale/Banche"]')).toHaveAttribute('aria-selected', 'true');
  await expect(dlg.locator('[data-new="banc"]')).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(dlg).toHaveCount(0);
  await expect.poll(() => count(page, 'Personale/Banche')).toBe(3);
  await page.getByTestId('pm-list').focus();
  await page.keyboard.press(`${mod}+a`);
  await expect(page.getByTestId('pm-multi')).toHaveAttribute('data-count', '6');
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('pm-multi')).toHaveCount(0);
  await expect(page.getByTestId('pm-detail')).toBeVisible();
});

test('drag picked entries onto a folder; onto "Tutte" takes them out', async ({ page }) => {
  await boot(page, 'screen=passwords&entry=Film in streaming');
  await entry(page, 'SPID').click({ modifiers: [mod] });
  await expect(page.getByTestId('pm-multi')).toHaveAttribute('data-count', '2');
  await entry(page, 'SPID').dragTo(folder(page, 'Casa'));
  await expect.poll(() => count(page, 'Casa')).toBe(3);
  await folder(page, 'Casa').click();
  await expect(page.getByTestId('pm-entry')).toHaveCount(3);
  await entry(page, 'Wi-Fi di casa').click();
  await entry(page, 'Wi-Fi di casa').dragTo(page.locator('[data-filter="all"]'));
  await expect.poll(() => count(page, 'Casa')).toBe(2);
});

test('folders: rename (F2) and delete (context menu) move entries, never delete them', async ({ page }) => {
  await boot(page, 'screen=passwords');
  await folder(page, 'Personale').focus();
  await page.keyboard.press('F2');
  const form = page.getByTestId('pm-folder-rename');
  await form.getByRole('textbox').fill('Privato');
  await page.getByTestId('pm-folder-rename-save').click();
  await expect(folder(page, 'Privato/Banche')).toBeVisible();
  await expect(folder(page, 'Personale')).toHaveCount(0);
  await expect.poll(() => count(page, 'Privato')).toBe(3);

  await folder(page, 'Privato').click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Elimina cartella…' }).click();
  await expect(page.getByRole('dialog')).toContainText('passano, con le sottocartelle, nella cartella superiore');
  await page.getByTestId('pm-folder-delete-confirm').click();
  await expect(folder(page, 'Privato')).toHaveCount(0);
  await expect(folder(page, 'Banche')).toBeVisible();
  await expect(page.getByTestId('pm-entry')).toHaveCount(6);
});

test('editor: the folder field is the picker; relative dates in the detail', async ({ page }) => {
  await boot(page, 'screen=passwords&entry=SPID');
  await expect(page.getByTestId('pm-dates')).toContainText('Modificata ieri');
  await page.getByTestId('pm-edit').click();
  const field = page.locator('#pm-folder');
  await field.click();
  await expect(page.getByTestId('pm-editor').getByRole('option')).toHaveCount(5);
  await page.getByTestId('pm-editor').locator('[data-folder="Lavoro"]').click();
  await expect(field).toHaveValue('Lavoro');
  await page.getByTestId('pm-save').click();
  await expect(page.getByTestId('pm-detail')).toContainText('Lavoro');
  await expect.poll(() => count(page, 'Lavoro')).toBe(2);
});

test('settings: site icons on by default, with the disclosure; off is kept', async ({ page }) => {
  await boot(page, 'screen=passwords');
  await expect(page.getByTestId('pm-icon').first()).toBeVisible();
  await boot(page, 'screen=settings');
  const sw = page.getByTestId('site-icons');
  await expect(sw).toHaveAttribute('aria-checked', 'true');
  await expect(page.getByTestId('settings-passwords')).toContainText('il sito vede una richiesta dal tuo indirizzo IP');
  await sw.click();
  await expect(sw).toHaveAttribute('aria-checked', 'false');
});

test('2,000 entries: only the rows in view exist, scroll reaches the end, search within budget', async ({ page, browserName }) => {
  await boot(page, 'screen=passwords&pm=2000');
  const list = page.getByTestId('pm-list');
  await expect(list).toHaveAttribute('data-count', '2006');
  expect(await page.getByTestId('pm-entry').count()).toBeLessThan(60);
  await list.evaluate((el) => (el.scrollTop = el.scrollHeight));
  await expect(page.getByTestId('pm-entry').last()).toHaveAttribute('aria-posinset', '2006');
  await list.evaluate((el) => (el.scrollTop = 0));

  const lat = await page.evaluate(async () => {
    const input = document.querySelector<HTMLInputElement>('[data-testid=pm-search]')!;
    const listEl = document.querySelector<HTMLElement>('[data-testid=pm-list]')!;
    const out: number[] = [];
    for (const ch of 'cloud 0041') {
      const t = performance.now();
      input.value += ch;
      input.dispatchEvent(new InputEvent('input', { bubbles: true, data: ch, inputType: 'insertText' }));
      await new Promise<void>((r) => {
        const check = () => (listEl.dataset.q === input.value ? requestAnimationFrame(() => r()) : requestAnimationFrame(check));
        check();
      });
      out.push(performance.now() - t);
    }
    return out;
  });
  await expect(list).toHaveAttribute('data-count', '1');
  await expect(page.getByTestId('pm-entry')).toHaveAttribute('data-title', 'Cloud 0041');
  if (browserName === 'chromium') {
    expect(Math.max(...lat), `keystroke -> list ms: ${lat.join(', ')}`).toBeLessThan(150);

    await page.getByTestId('pm-search').fill('');
    await expect(list).toHaveAttribute('data-count', '2006');
    const fps = await list.evaluate(async (sc) => {
      const frames: number[] = [];
      let last = performance.now();
      const t0 = last;
      await new Promise<void>((done) => {
        const tick = (t: number) => {
          frames.push(t - last);
          last = t;
          sc.scrollTop = ((t - t0) / 1000) * 2400;
          if (t - t0 < 2000) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      const f = frames.slice(1);
      return f.length / (f.reduce((a, b) => a + b, 0) / 1000);
    });
    expect(fps, 'scroll fps').toBeGreaterThan(40);
  }
});
