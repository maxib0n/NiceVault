// Drag & drop: OS files onto the window / onto a folder, internal moves onto
// folders and breadcrumb crumbs, refusals, non-file data, and drags while a
// modal is open. OS drags are synthesized with a real DataTransfer holding real
// File objects and dispatched at the hit-tested element (see helpers.ts).
import {
  boot,
  breadcrumb,
  dragSequence,
  dropFiles,
  dropOverlay,
  expect,
  expectFolder,
  fileList,
  item,
  jobTray,
  listed,
  names,
  nodeId,
  test,
  toasts,
} from './helpers';

const DRAG_MIME = 'application/x-nv-nodes';

test.beforeEach(async ({ page }) => {
  await boot(page, 'screen=vault&view=list');
  await listed(page);
});

test.describe('OS files → window', () => {
  test('overlay appears on dragenter/dragover and disappears on dragleave', async ({ page }) => {
    await expect(dropOverlay(page)).toHaveCount(0);
    const r = await dragSequence(fileList(page), { files: [{ name: 'Nota.txt' }] }, ['dragenter', 'dragover']);
    expect(r[1].accepted, 'dragover with Files must be accepted (preventDefault)').toBe(true);
    await expect(dropOverlay(page)).toBeVisible();
    await expect(dropOverlay(page)).toContainText('Rilascia per importare in Documenti');
    // more dragover events keep it
    await dragSequence(item(page, 'Leggimi.txt'), { files: [{ name: 'Nota.txt' }] }, ['dragover']);
    await expect(dropOverlay(page)).toBeVisible();
    // leaving the window (relatedTarget = null)
    await dragSequence(fileList(page), { files: [{ name: 'Nota.txt' }] }, ['dragleave']);
    await expect(dropOverlay(page)).toHaveCount(0);
  });

  test('drop imports through a job: overlay goes away, progress runs, files appear, toast', async ({ page }) => {
    const files = [
      { name: 'Nota.txt', content: 'ciao', type: 'text/plain' },
      { name: 'Scontrino.pdf', content: '%PDF-1.4', type: 'application/pdf' },
    ];
    await dragSequence(fileList(page), { files }, ['dragenter', 'dragover']);
    await expect(dropOverlay(page)).toBeVisible();
    const r = await dragSequence(fileList(page), { files }, ['drop']);
    expect(r[0].accepted).toBe(true);
    await expect(dropOverlay(page)).toHaveCount(0);
    await expect(jobTray(page)).toBeVisible();
    // the tray reports live progress while the job runs
    await expect(jobTray(page).getByRole('status')).toHaveText(/Importazione · \d+%|Operazioni completate/);
    await expect(item(page, 'Nota.txt')).toBeVisible({ timeout: 8000 });
    await expect(item(page, 'Scontrino.pdf')).toBeVisible();
    await expect(toasts(page).getByText('Importato: 2 elementi.')).toBeVisible();
    await expect(jobTray(page).getByRole('status')).toHaveText('Operazioni completate');
    // the imported text file is previewable from the dropped bytes
    await item(page, 'Nota.txt').dblclick();
    await expect(page.getByRole('dialog', { name: 'Anteprima di Nota.txt' })).toContainText('ciao');
  });

  test('dropping a name that exists gets a free name', async ({ page }) => {
    await dropFiles(fileList(page), [{ name: 'Leggimi.txt', content: 'nuovo' }]);
    await expect(item(page, 'Leggimi (2).txt')).toBeVisible({ timeout: 8000 });
    await expect(item(page, 'Leggimi.txt')).toBeVisible();
  });

  test('drop onto a folder tile imports into that folder (not the current one)', async ({ page }) => {
    const r = await dropFiles(item(page, 'Lavoro'), [{ name: 'Busta paga.pdf', content: '%PDF' }]);
    // (dragenter needs no preventDefault in browsers; dragover and drop must accept)
    expect(r.slice(1).every((e) => e.accepted)).toBe(true);
    await expect(dropOverlay(page)).toHaveCount(0);
    await expect(toasts(page).getByText('Importato: Busta paga.pdf.')).toBeVisible({ timeout: 8000 });
    expect(await names(page)).not.toContain('Busta paga.pdf');
    // the job names the folder it went into
    await jobTray(page).getByRole('button', { expanded: false }).click();
    await expect(jobTray(page).getByRole('listitem').first()).toContainText('→ Lavoro');
    await item(page, 'Lavoro').dblclick();
    await expectFolder(page, 'Lavoro');
    await expect(item(page, 'Busta paga.pdf')).toBeVisible();
  });

  test('a file tile is not a drop target: drop on it imports into the current folder', async ({ page }) => {
    await dropFiles(item(page, 'Copertina.jpg'), [{ name: 'Altro.txt' }]);
    await expect(item(page, 'Altro.txt')).toBeVisible({ timeout: 8000 });
  });
});

test.describe('non-file data is ignored', () => {
  test('text/plain drag: not accepted, no overlay, drop does nothing', async ({ page }) => {
    const before = await names(page);
    const r = await dragSequence(fileList(page), { data: { 'text/plain': 'ciao' } }, ['dragenter', 'dragover', 'drop']);
    expect(r[1].accepted).toBe(false);
    expect(r[2].accepted).toBe(false);
    await expect(dropOverlay(page)).toHaveCount(0);
    await expect(jobTray(page)).toHaveCount(0);
    expect(await names(page)).toEqual(before);
  });

  test('"Files" type with zero files: overlay clears on drop and no job starts', async ({ page }) => {
    const before = await names(page);
    await dragSequence(fileList(page), { emptyFiles: true }, ['dragenter', 'dragover']);
    await expect(dropOverlay(page)).toBeVisible();
    await dragSequence(fileList(page), { emptyFiles: true }, ['drop']);
    await expect(dropOverlay(page)).toHaveCount(0);
    await page.waitForTimeout(300);
    await expect(jobTray(page)).toHaveCount(0);
    expect(await names(page)).toEqual(before);
  });

  test('internal node drag over the background never shows the import overlay', async ({ page }) => {
    const id = await nodeId(page, 'Leggimi.txt');
    const r = await dragSequence(fileList(page), { data: { [DRAG_MIME]: JSON.stringify([id]) } }, ['dragenter', 'dragover', 'drop']);
    expect(r[1].accepted).toBe(false);
    await expect(dropOverlay(page)).toHaveCount(0);
    expect(await names(page)).toContain('Leggimi.txt');
  });
});

test.describe('internal moves', () => {
  test('drag a file onto a folder moves it (real pointer drag)', async ({ page }) => {
    await item(page, 'Leggimi.txt').dragTo(item(page, 'Lavoro'));
    await expect(item(page, 'Leggimi.txt')).toHaveCount(0);
    await item(page, 'Lavoro').dblclick();
    await expectFolder(page, 'Lavoro');
    await expect(item(page, 'Leggimi.txt')).toBeVisible();
  });

  test('drag a multi-selection onto a folder moves all of it', async ({ page }) => {
    await item(page, 'Copertina.jpg').click();
    await item(page, 'Leggimi.txt').click({ modifiers: ['Shift'] });
    await item(page, 'Foto profilo.jpg').dragTo(item(page, 'Personale'));
    await expect(item(page, 'Copertina.jpg')).toHaveCount(0);
    await expect(item(page, 'Foto profilo.jpg')).toHaveCount(0);
    await expect(item(page, 'Leggimi.txt')).toHaveCount(0);
    await item(page, 'Personale').dblclick();
    await expectFolder(page, 'Personale');
    for (const n of ['Copertina.jpg', 'Foto profilo.jpg', 'Leggimi.txt']) await expect(item(page, n)).toBeVisible();
  });

  test('dragging an unselected item moves only that item', async ({ page }) => {
    await item(page, 'Copertina.jpg').click();
    await item(page, 'Leggimi.txt').dragTo(item(page, 'Chiavi'));
    await expect(item(page, 'Leggimi.txt')).toHaveCount(0);
    await expect(item(page, 'Copertina.jpg')).toBeVisible();
  });

  test('drag onto a breadcrumb segment moves into that ancestor', async ({ page }) => {
    await item(page, 'Foto').dblclick();
    await item(page, 'Vacanze 2024').dblclick();
    await expectFolder(page, 'Vacanze 2024');
    await item(page, 'Rifugio.jpg').dragTo(breadcrumb(page).getByRole('button', { name: 'Documenti' }));
    await expect(item(page, 'Rifugio.jpg')).toHaveCount(0);
    await item(page, 'Lago di Braies.jpg').dragTo(breadcrumb(page).getByRole('button', { name: 'Foto' }));
    await expect(item(page, 'Lago di Braies.jpg')).toHaveCount(0);
    await breadcrumb(page).getByRole('button', { name: 'Foto' }).click();
    await expectFolder(page, 'Foto');
    await expect(item(page, 'Lago di Braies.jpg')).toBeVisible();
    await breadcrumb(page).getByRole('button', { name: 'Documenti' }).click();
    await expectFolder(page, 'Documenti');
    await expect(item(page, 'Rifugio.jpg')).toBeVisible();
  });

  test('a move that collides reports the error and keeps both items', async ({ page }) => {
    await item(page, 'Foto').dblclick();
    await expectFolder(page, 'Foto');
    // root already has "Foto profilo.jpg"; make a same-named file here first
    await item(page, 'Micio.jpg').click();
    await page.keyboard.press('F2');
    await page.getByRole('textbox', { name: 'Nuovo nome' }).fill('Foto profilo.jpg');
    await page.keyboard.press('Enter');
    await item(page, 'Foto profilo.jpg').dragTo(breadcrumb(page).getByRole('button', { name: 'Documenti' }));
    await expect(toasts(page).getByText('Esiste già un elemento con questo nome.')).toBeVisible();
    await expect(item(page, 'Foto profilo.jpg')).toBeVisible();
  });

  test('a folder dropped onto itself is refused (not even a drop target)', async ({ page }) => {
    await item(page, 'Foto').dragTo(item(page, 'Foto'));
    await expect(item(page, 'Foto')).toBeVisible();
    await expect(toasts(page).getByText(/Esiste|non valido|stessa/)).toHaveCount(0);
    // synthesized: the tile does not accept a drag of itself
    const id = await nodeId(page, 'Foto');
    await item(page, 'Foto').click();
    const r = await dragSequence(item(page, 'Foto'), { data: { [DRAG_MIME]: JSON.stringify([id]) } }, ['dragover', 'drop']);
    expect(r[0].accepted).toBe(false);
    await item(page, 'Foto').dblclick();
    await expectFolder(page, 'Foto');
    expect(await names(page)).not.toContain('Foto');
  });

  test('a folder dropped into its own descendant is refused', async ({ page }) => {
    const foto = await nodeId(page, 'Foto');
    await item(page, 'Foto').dblclick();
    await expectFolder(page, 'Foto');
    const before = await names(page);
    // a stale/foreign drag payload carrying the ancestor "Foto" onto its child "Vacanze 2024"
    await dragSequence(item(page, 'Vacanze 2024'), { data: { [DRAG_MIME]: JSON.stringify([foto]) } }, ['dragenter', 'dragover', 'drop']);
    await page.waitForTimeout(300);
    expect(await names(page)).toEqual(before);
    await breadcrumb(page).getByRole('button', { name: 'Documenti' }).click();
    await expectFolder(page, 'Documenti');
    await expect(item(page, 'Foto')).toBeVisible();
    // and the backend refuses it too, whatever the UI sends
    const err = await page.evaluate(async (foto) => {
      const nv = (window as unknown as { __nv: { mock: { invoke: (c: string, a: unknown) => Promise<unknown> } } }).__nv.mock;
      const vault = 'v1';
      const kids = (await nv.invoke('fs_list', { vault, dir: foto })) as { id: number; name: string }[];
      const vac = kids.find((k) => k.name === 'Vacanze 2024')!;
      try {
        await nv.invoke('fs_rename', { vault, node: foto, new_parent: vac.id, new_name: 'Foto' });
        return null;
      } catch (e) {
        return (e as { code: string }).code;
      }
    }, foto);
    expect(err).toBe('invalid_name');
  });
});

test.describe('drags while a modal is open', () => {
  test('delete confirmation open: no overlay, drop does not import', async ({ page }) => {
    await item(page, 'Leggimi.txt').click();
    await page.keyboard.press('Delete');
    const dlg = page.getByRole('dialog', { name: /Eliminare/ });
    await expect(dlg).toBeVisible();
    const r = await dropFiles(fileList(page), [{ name: 'Intruso.txt' }]);
    expect(r[0].inDialog, 'the modal must be what the pointer hits').toBe(true);
    await expect(dropOverlay(page)).toHaveCount(0);
    await dlg.getByRole('button', { name: 'Annulla' }).click();
    await page.waitForTimeout(1500);
    expect(await names(page)).not.toContain('Intruso.txt');
    await expect(jobTray(page)).toHaveCount(0);
  });

  test('quick look open: no overlay, drop does not import; after closing, drops work again', async ({ page }) => {
    await item(page, 'Copertina.jpg').dblclick();
    await expect(page.getByRole('dialog', { name: 'Anteprima di Copertina.jpg' })).toBeVisible();
    const r = await dropFiles(fileList(page), [{ name: 'Intruso.txt' }]);
    expect(r[0].inDialog).toBe(true);
    await expect(dropOverlay(page)).toHaveCount(0);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(1500);
    expect(await names(page)).not.toContain('Intruso.txt');
    await dropFiles(fileList(page), [{ name: 'Benvenuto.txt' }]);
    await expect(item(page, 'Benvenuto.txt')).toBeVisible({ timeout: 8000 });
  });

  test('context menu open: a drop still lands (menu is not modal) and the overlay clears', async ({ page }) => {
    await item(page, 'Leggimi.txt').click({ button: 'right' });
    await expect(page.getByRole('menu')).toBeVisible();
    await dropFiles(fileList(page), [{ name: 'Durante menu.txt' }]);
    await expect(dropOverlay(page)).toHaveCount(0);
    await expect(item(page, 'Durante menu.txt')).toBeVisible({ timeout: 8000 });
  });
});

// The Tauri webview delivers OS file drops as Tauri events (host paths + a
// position), not as DOM drop events. The mock replays that event stream into
// the very same handler (api.onOsDragDrop), so this logic is covered here too.
test.describe('Tauri-style OS drag events (replayed by the mock)', () => {
  type Ev = { type: 'enter' | 'over' | 'drop' | 'leave'; paths?: string[]; position?: { x: number; y: number } };
  const os = (page: import('@playwright/test').Page, e: Ev) =>
    page.evaluate((e) => (window as unknown as { __nv: { mock: { simulateOsDrag: (e: unknown) => void } } }).__nv.mock.simulateOsDrag(e), e);
  const centre = async (l: import('@playwright/test').Locator) => {
    const b = (await l.boundingBox())!;
    return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
  };

  test('enter/over show the overlay, leave hides it, drop on the background imports into the current folder', async ({ page }) => {
    const at = await centre(fileList(page));
    await os(page, { type: 'enter', paths: ['/home/utente/Scaricati/Bolletta gas.pdf'], position: at });
    await expect(dropOverlay(page)).toBeVisible();
    await os(page, { type: 'over', position: at });
    await os(page, { type: 'leave' });
    await expect(dropOverlay(page)).toHaveCount(0);
    await os(page, { type: 'enter', paths: ['/home/utente/Scaricati/Bolletta gas.pdf'], position: at });
    await os(page, { type: 'drop', paths: ['/home/utente/Scaricati/Bolletta gas.pdf'], position: at });
    await expect(dropOverlay(page)).toHaveCount(0);
    await expect(item(page, 'Bolletta gas.pdf')).toBeVisible({ timeout: 8000 });
  });

  test('drop over a folder tile imports into that folder', async ({ page }) => {
    const at = await centre(item(page, 'Personale'));
    await os(page, { type: 'enter', paths: ['/tmp/Ricetta.txt'], position: at });
    await os(page, { type: 'drop', paths: ['/tmp/Ricetta.txt'], position: at });
    await expect(toasts(page).getByText('Importato: Ricetta.txt.')).toBeVisible({ timeout: 8000 });
    expect(await names(page)).not.toContain('Ricetta.txt');
    await item(page, 'Personale').dblclick();
    await expectFolder(page, 'Personale');
    await expect(item(page, 'Ricetta.txt')).toBeVisible();
  });

  test('drop over a file tile imports into the current folder', async ({ page }) => {
    const at = await centre(item(page, 'Leggimi.txt'));
    await os(page, { type: 'drop', paths: ['/tmp/Accanto.txt'], position: at });
    await expect(item(page, 'Accanto.txt')).toBeVisible({ timeout: 8000 });
  });

  test('ignored while a modal is open (delete confirmation, quick look)', async ({ page }) => {
    const at = await centre(fileList(page));
    await item(page, 'Leggimi.txt').click();
    await page.keyboard.press('Delete');
    await expect(page.getByRole('dialog', { name: /Eliminare/ })).toBeVisible();
    await os(page, { type: 'enter', paths: ['/tmp/Intruso.txt'], position: at });
    await expect(dropOverlay(page)).toHaveCount(0);
    await os(page, { type: 'drop', paths: ['/tmp/Intruso.txt'], position: at });
    await page.getByRole('button', { name: 'Annulla' }).click();

    await item(page, 'Copertina.jpg').dblclick();
    await expect(page.getByRole('dialog', { name: 'Anteprima di Copertina.jpg' })).toBeVisible();
    await os(page, { type: 'drop', paths: ['/tmp/Intruso2.txt'], position: at });
    await page.keyboard.press('Escape');
    await page.waitForTimeout(1500);
    expect(await names(page)).not.toContain('Intruso.txt');
    expect(await names(page)).not.toContain('Intruso2.txt');
    await expect(jobTray(page)).toHaveCount(0);
  });
});
