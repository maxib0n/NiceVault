// The sidebar folds to a rail (toggle + Ctrl/Cmd+B, remembered); phones have no
// interface size; list rows show what a folder holds; an empty folder is a closed, sealed
// envelope at every size; a phone's finished transfers are a toast, not a slip that stays.
import { boot, dropFiles, expect, expectScreen, fileList, item, jobTray, listed, modKey, test, toasts } from './helpers';

const BUSTE = 'screen=vault&dir=Personale/Buste';

test.describe('sidebar rail', () => {
  test('the toggle and Ctrl/Cmd+B fold it to a rail and back; remembered across a lock', async ({ page }) => {
    await boot(page, 'screen=vault');
    await listed(page);
    const toggle = page.getByTestId('sidebar-toggle');
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    // the rail: the sections as icons with their names, the clasp, Impostazioni, all in a slim strip
    await expect(async () => {
      for (const id of ['nav-files', 'nav-passwords', 'nav-details', 'open-settings']) {
        const b = (await page.getByTestId(id).boundingBox())!;
        expect(b.x + b.width, id).toBeLessThan(80);
      }
      const list = (await fileList(page).boundingBox())!;
      expect(list.x).toBeLessThan(120);
    }).toPass();
    await expect(page.getByTestId('nav-passwords')).toHaveAttribute('title', 'Password');
    await page.getByTestId('nav-details').click();
    await expect(page.getByTestId('nav-details')).toHaveAttribute('aria-current', 'page');
    await page.getByTestId('nav-files').click();
    await listed(page);

    // remembered: lock, unlock, still a rail
    await page.getByRole('button', { name: 'Blocca' }).click();
    await expectScreen(page, 'unlock');
    await page.getByRole('textbox', { name: 'Password' }).fill('demo');
    await page.keyboard.press('Enter');
    await expectScreen(page, 'vault');
    await expect(page.getByTestId('sidebar-toggle')).toHaveAttribute('aria-expanded', 'false');

    // the keyboard: Ctrl/Cmd+B unfolds it, and folds it again
    await fileList(page).focus();
    const mod = await modKey(page);
    await page.keyboard.press(`${mod}+b`);
    await expect(page.getByTestId('sidebar-toggle')).toHaveAttribute('aria-expanded', 'true');
    await expect(page.getByText('Volumi aperti')).toBeVisible();
    await page.keyboard.press(`${mod}+b`);
    await expect(page.getByTestId('sidebar-toggle')).toHaveAttribute('aria-expanded', 'false');
  });

  test('at 900×600 and 150% the rail fits and the page widens', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 600 });
    await boot(page, 'screen=vault&scale=150');
    await listed(page);
    await page.getByTestId('sidebar-toggle').click();
    await expect(async () => {
      const s = (await page.getByTestId('open-settings').boundingBox())!;
      // zoomed 150%: boxes are in CSS px of the zoomed root, still inside the window
      expect(s.y).toBeGreaterThan(0);
      const files = (await page.getByTestId('nav-files').boundingBox())!;
      expect(files.x).toBeGreaterThanOrEqual(0);
    }).toPass();
    await expect(page.getByRole('button', { name: 'Blocca' })).toBeVisible();
  });
});

test.describe('phones', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

  test('no interface size control, and no rail toggle', async ({ page }) => {
    await boot(page, 'screen=settings&platform=mobile&scale=150');
    await expect(page.getByTestId('app-scale')).toHaveCount(0);
    // the saved size is not worn on a phone
    expect(await page.evaluate(() => document.documentElement.style.zoom || '1')).toBe('1');
    await boot(page, 'screen=vault&platform=mobile');
    await listed(page);
    await expect(page.getByTestId('sidebar-toggle')).toHaveCount(0);
  });

  test('the "Vista" key: grid or list and the size, in a sheet', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile&view=grid');
    await listed(page);
    await page.getByTestId('phone-view').click();
    const sheet = page.getByRole('dialog', { name: 'Vista' });
    await expect(sheet).toBeVisible();
    await sheet.getByRole('radio', { name: 'Grandi', exact: true }).click();
    await expect(page.locator('[data-grid-size="l"]')).toHaveCount(1);
    await sheet.getByRole('radio', { name: 'Elenco' }).click();
    await expect(sheet.getByRole('radio', { name: 'Ampia' })).toBeVisible();
    await sheet.getByRole('radio', { name: 'Ampia' }).click();
    await expect(sheet.getByRole('radio', { name: 'Ampia' })).toHaveAttribute('aria-checked', 'true');
    await sheet.getByRole('button', { name: 'Chiudi' }).click();
    await expect(sheet).toBeHidden();
    await expect(fileList(page).getByRole('option').first()).toHaveAttribute('data-layout', 'list');
  });

  test('finished transfers: a toast, nothing left over the page', async ({ page }) => {
    await boot(page, 'screen=vault&platform=mobile');
    await listed(page);
    // onto a file row: the list's own centre may sit on a folder tile, which would take the file into that folder
    const file = item(page, 'Leggimi.txt');
    await file.scrollIntoViewIfNeeded();
    await dropFiles(file, [{ name: 'Nota del telefono.txt', content: 'ciao' }]);
    await expect(toasts(page)).toContainText('Operazioni completate');
    await expect(jobTray(page)).toHaveCount(0);
    await expect(page.locator('dialog[open]')).toHaveCount(0);
    // the phone's list is virtual: walk it (from the top) until the new row is drawn
    await fileList(page).evaluate((el) => el.scrollTo(0, 0));
    for (let i = 0; i < 20 && !(await item(page, 'Nota del telefono.txt').count()); i++) {
      await fileList(page).evaluate((el) => el.scrollBy(0, el.clientHeight * 0.8));
      await page.waitForTimeout(120);
    }
    await expect(item(page, 'Nota del telefono.txt')).toBeVisible();
  });
});

test.describe('envelopes', () => {
  test('list rows show what a folder holds; the empty one is closed and sealed', async ({ page }) => {
    await boot(page, `${BUSTE}&view=list`);
    await listed(page);
    await expect(item(page, 'Due file').locator('[data-peek]')).toHaveCount(2);
    await expect(item(page, 'Foto e cartelle').locator('[data-peek]')).toHaveCount(3);
    const empty = item(page, 'Vuota');
    await expect(empty.locator('[data-envelope][data-closed]')).toHaveCount(1);
    await expect(empty.locator('[data-seal]')).toHaveCount(1);
    // open envelopes carry no wax
    await expect(fileList(page).locator('[data-seal]')).toHaveCount(1);
  });

  test('an empty folder is closed and sealed at every grid size', async ({ page }) => {
    await boot(page, `${BUSTE}&view=grid`);
    await listed(page);
    const sizes = page.getByRole('radiogroup', { name: 'Dimensione delle icone' });
    for (const s of ['Piccole', 'Medie', 'Grandi', 'Molto grandi']) {
      await sizes.getByRole('radio', { name: s, exact: true }).click();
      await expect(item(page, 'Vuota').locator('[data-closed] [data-seal]'), s).toHaveCount(1);
      await expect(item(page, 'Due file').locator('[data-closed]'), s).toHaveCount(0);
      await expect(fileList(page).locator('[data-seal]'), s).toHaveCount(1);
    }
  });
});

test.describe('passports on Benvenuto', () => {
  // one grid for every passport: the header's top, the seal's centre (on the cover's visual
  // centre: between the stitches, or the impression's own centre when the volume is missing)
  const VARIANTS = ['', 'frame=laurel', 'frame=rope', 'frame=meander', 'frame=starburst', 'frame=hairline', 'emblem=image', 'emblem=initial', 'emblem=icon', 'emblem=hidden'];
  test('header, seal and name share one grid across every kind of passport', async ({ page }) => {
    test.slow();
    const all: { v: string; name: string; issuerTop: number; issuerDx: number; sealDx: number; sealCy: number; nameDx: number }[] = [];
    for (const v of VARIANTS) {
      await boot(page, `screen=welcome&${v}`);
      await expect(page.getByTestId('passport').first()).toBeVisible();
      const rows = await page.evaluate(() =>
        [...document.querySelectorAll<HTMLElement>('[data-testid=passport]')].map((p) => {
          const r = (s: string) => p.querySelector(s)!.getBoundingClientRect();
          const c = r('.cover');
          const stitch = p.querySelector('.cover .stitch');
          const st = stitch && stitch.getClientRects().length ? stitch.getBoundingClientRect() : null;
          // stitched: the panel inside the thread (inset 20 at the spine, 6 at the fore-edge)
          const mid = st && st.width ? c.left + (20 + (c.width - 6)) / 2 : c.left + c.width / 2;
          const is = r('.issuer');
          const se = r('.mark .rosette');
          const nm = r('.name');
          return {
            name: p.dataset.name ?? '',
            issuerTop: is.top - c.top,
            issuerDx: is.left + is.width / 2 - mid,
            sealDx: se.left + se.width / 2 - mid,
            sealCy: se.top + se.height / 2 - c.top,
            nameDx: nm.left + nm.width / 2 - mid,
          };
        }),
      );
      for (const row of rows) all.push({ v, ...row });
    }
    const first = all[0];
    for (const a of all) {
      const at = `${a.v || 'default'} / ${a.name}`;
      expect(Math.abs(a.issuerTop - first.issuerTop), `${at}: header top`).toBeLessThan(0.75);
      expect(Math.abs(a.sealCy - first.sealCy), `${at}: seal centre y`).toBeLessThan(0.75);
      expect(Math.abs(a.sealDx), `${at}: seal centre x`).toBeLessThan(0.75);
      expect(Math.abs(a.issuerDx), `${at}: header centre x`).toBeLessThan(0.75);
      expect(Math.abs(a.nameDx), `${at}: name centre x`).toBeLessThan(1);
    }
  });
});

test.describe('emblem inside its seal', () => {
  // the mark (initial, icon, picture, the hidden rosette) sits on the seal's centre, in every
  // frame and at every size: Benvenuto, the cover in the sidebar, Sblocca, Dettagli
  for (const q of ['screen=welcome', 'screen=welcome&frame=laurel', 'screen=welcome&emblem=icon', 'screen=welcome&emblem=image', 'screen=vault', 'screen=unlock', 'screen=details&frame=starburst']) {
    test(`centred (${q})`, async ({ page }) => {
      await boot(page, q);
      await expect(page.locator('.rosette .emblem').first()).toBeVisible();
      const off = await page.evaluate(() =>
        [...document.querySelectorAll('.rosette')]
          .filter((r) => r.querySelector('.emblem') && r.getClientRects().length)
          .map((r) => {
            const a = r.getBoundingClientRect();
            const b = r.querySelector('.emblem')!.getBoundingClientRect();
            return [b.left + b.width / 2 - (a.left + a.width / 2), b.top + b.height / 2 - (a.top + a.height / 2)];
          }),
      );
      expect(off.length).toBeGreaterThan(0);
      for (const [dx, dy] of off) {
        expect(Math.abs(dx)).toBeLessThan(0.3);
        expect(Math.abs(dy)).toBeLessThan(0.3);
      }
    });
  }
});
