// Shared helpers for the mock-UI suite. Everything here locates elements by
// role / accessible name / text / data-testid / data-* state hooks — never by
// CSS class — so the suite survives the visual redesign.
import { test as base, expect, type Locator, type Page } from '@playwright/test';

export { expect };

/** Fails a test on any uncaught page error or console.error. */
export const test = base.extend<{ consoleErrors: string[] }>({
  consoleErrors: [
    async ({ page }, use) => {
      const errors: string[] = [];
      page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
      page.on('console', (m) => {
        if (m.type() === 'error') errors.push(`console: ${m.text()}`);
      });
      await use(errors);
      expect(errors, 'page reported errors').toEqual([]);
    },
    { auto: true },
  ],
});

/** Load the mock UI with URL switches (see src/lib/switches.ts) and wait until it is ready. */
export async function boot(page: Page, query = ''): Promise<void> {
  await page.goto(`/?${query}`);
  await expect(page.locator('[data-ready="true"]')).toHaveCount(1);
}

export const screen = (page: Page) => page.locator('[data-screen]').first();

export async function expectScreen(page: Page, s: string) {
  await expect(page.locator('[data-ready="true"]')).toHaveAttribute('data-screen', s);
}

/** The file listbox of the open folder. */
export const fileList = (page: Page) => page.getByRole('listbox', { name: /^File in / });

/** One entry (role=option) by its exact name. */
export function item(page: Page, name: string): Locator {
  return fileList(page)
    .getByRole('option')
    .filter({ has: page.getByTestId('item-name').and(page.getByText(name, { exact: true })) });
}

export async function names(page: Page): Promise<string[]> {
  return (await fileList(page).getByTestId('item-name').allTextContents()).map((s) => s.trim());
}

export async function selectedNames(page: Page): Promise<string[]> {
  return (await fileList(page).getByRole('option', { selected: true }).getByTestId('item-name').allTextContents()).map((s) =>
    s.trim(),
  );
}

/** Wait for the listing to be loaded (non-busy) */
export async function listed(page: Page) {
  await expect(fileList(page)).toHaveAttribute('aria-busy', 'false');
}

export const breadcrumb = (page: Page) => page.getByRole('navigation', { name: 'Percorso' });
export async function expectFolder(page: Page, name: string) {
  await expect(breadcrumb(page).locator('[aria-current="page"]')).toHaveText(name);
  await listed(page);
}

/** "Control" or "Meta", matching the UI's own isMac test (navigator.platform). */
export async function modKey(page: Page): Promise<'Control' | 'Meta'> {
  return (await page.evaluate(() => /Mac/i.test(navigator.platform))) ? 'Meta' : 'Control';
}

export const dropOverlay = (page: Page) => page.getByTestId('drop-overlay');
export const jobTray = (page: Page) => page.getByRole('region', { name: 'Trasferimenti' });
export const toasts = (page: Page) => page.getByRole('region', { name: 'Notifiche' });

/** Node id of an entry (from its DOM id "item-<id>", used by aria-activedescendant). */
export async function nodeId(page: Page, name: string): Promise<number> {
  const id = await item(page, name).getAttribute('id');
  return Number(id?.replace('item-', ''));
}

// ---------------------------------------------------------------------------
// Synthesized drag & drop. Events are dispatched at the element the browser
// would hit-test at the target's centre (document.elementFromPoint), so modal
// dialogs and overlays block them exactly like a real pointer would.

export interface FakeFile {
  name: string;
  content?: string;
  type?: string;
}

export interface DragPayload {
  files?: FakeFile[];
  /** extra string data, e.g. { 'text/plain': 'hi' } or the internal node MIME */
  data?: Record<string, string>;
  /** pretend `types` contains "Files" while `files` is empty (some OS drags of folders) */
  emptyFiles?: boolean;
}

export type DragStep = 'dragenter' | 'dragover' | 'dragleave' | 'drop';

/** Dispatch a sequence of drag events at the hit-tested element under `target`'s centre. Returns, per event, whether it was default-prevented (= "accepted"). */
export async function dragSequence(target: Locator, payload: DragPayload, steps: DragStep[]): Promise<{ type: string; accepted: boolean; hit: string; inDialog: boolean }[]> {
  return target.evaluate(
    (el, { payload, steps }) => {
      const r = el.getBoundingClientRect();
      const x = r.left + r.width / 2;
      const y = r.top + r.height / 2;
      const hit = document.elementFromPoint(x, y) ?? el;
      const dt = new DataTransfer();
      for (const f of payload.files ?? []) dt.items.add(new File([f.content ?? 'x'], f.name, { type: f.type ?? '' }));
      for (const [k, v] of Object.entries(payload.data ?? {})) dt.setData(k, v);
      const fake = payload.emptyFiles
        ? { types: ['Files'], files: [] as unknown as FileList, getData: () => '', setData() {}, dropEffect: 'none', effectAllowed: 'all', items: [] }
        : null;
      const out: { type: string; accepted: boolean; hit: string; inDialog: boolean }[] = [];
      for (const type of steps) {
        const ev = new DragEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, dataTransfer: fake ? null : dt });
        if (fake) Object.defineProperty(ev, 'dataTransfer', { value: fake });
        if (type === 'dragleave') Object.defineProperty(ev, 'relatedTarget', { value: null });
        const ok = hit.dispatchEvent(ev);
        out.push({ type, accepted: !ok, hit: hit.tagName.toLowerCase() + (hit.id ? '#' + hit.id : ''), inDialog: !!hit.closest('dialog[open]') });
      }
      return out;
    },
    { payload, steps },
  );
}

/** Full OS-style file drop on `target`: dragenter, dragover ×2, drop. */
export function dropFiles(target: Locator, files: FakeFile[]) {
  return dragSequence(target, { files }, ['dragenter', 'dragover', 'dragover', 'drop']);
}
