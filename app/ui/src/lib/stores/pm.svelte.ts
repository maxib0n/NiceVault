// Password manager state for ONE open volume (the Password screen): the list (no secrets),
// search-as-you-type, filters (all / favourites / 2FA / a folder), the selection (one entry
// shown in the detail, any number picked for "Sposta in cartella"), folder operations, site
// icons, and the clipboard countdown after a copy.
// Secrets are never kept here: values are revealed (pm_reveal) or copied (pm_copy) on demand.
//
// Performance (2,000+ entries): pm_list is loaded once (it carries everything a row shows,
// icon ref included) and searched here, in memory, against a lowercase index built once per
// load; the typed query is applied on the next animation frame (typing stays instant, the list
// follows within a frame); folder counts come from one pass. The screen renders only the rows
// in view (Passwords.svelte).
import * as api from '../api';
import type { EntryId, PmEntry, PmEntryInput, PmEntrySummary, PmSecretField } from '../api';
import { errorText } from '../errors';
import { click as selClick, empty as selEmpty, ensureSelected, selectAll, type Mods, type Selection } from '../selection';
import type { VaultId } from '../types';
import { play } from '../sound';
import { toasts } from './toasts.svelte';
import { m } from '../../paraglide/messages.js';

export type PmFilter = { kind: 'all' } | { kind: 'favorites' } | { kind: 'totp' } | { kind: 'folder'; folder: string };

export interface PmCopiedState {
  /** what was copied, as shown ("Password", "Utente"...) */
  label: string;
  field: PmSecretField;
  /** Date.now() when the backend wipes the clipboard (null = never) */
  until: number | null;
  /** the whole countdown, seconds */
  seconds: number;
}

const sameFilter = (a: PmFilter, b: PmFilter) => a.kind === b.kind && (a.kind !== 'folder' || a.folder === (b as { folder: string }).folder);

/** What a search looks at (same as the backend's pm_search): title, user, websites, tags, folder. */
export const haystack = (e: PmEntrySummary) => `${e.title}\n${e.username}\n${e.urls.join('\n')}\n${e.tags.join('\n')}\n${e.folder}`.toLowerCase();

const inFolder = (folder: string, f: string) => folder === f || folder.startsWith(`${f}/`);

/** Folder paths and all their parents, sorted ("Personale", "Personale/Banche"). */
export function folderTree(entries: readonly PmEntrySummary[]): string[] {
  const set = new Set<string>();
  for (const e of entries) {
    if (!e.folder) continue;
    const parts = e.folder.split('/');
    for (let i = 1; i <= parts.length; i++) set.add(parts.slice(0, i).join('/'));
  }
  return [...set].sort((a, b) => a.localeCompare(b));
}

/** Entries matching every whitespace-separated term of `q` (index = haystack per entry). */
export function searchIn(entries: readonly PmEntrySummary[], index: readonly string[], q: string): PmEntrySummary[] {
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return entries as PmEntrySummary[];
  const out: PmEntrySummary[] = [];
  for (let i = 0; i < entries.length; i++) {
    const h = index[i];
    if (terms.every((t) => h.includes(t))) out.push(entries[i]);
  }
  return out;
}

const raf: (f: () => void) => unknown = typeof requestAnimationFrame === 'function' ? requestAnimationFrame : (f) => setTimeout(f, 0);
const caf: (h: unknown) => void = typeof cancelAnimationFrame === 'function' ? (h) => cancelAnimationFrame(h as number) : (h) => clearTimeout(h as ReturnType<typeof setTimeout>);

export class PmStore {
  readonly vault: VaultId;
  all = $state.raw<PmEntrySummary[]>([]);
  /** what is typed in the search box (instant) */
  query = $state('');
  /** the query the list shows (follows `query` on the next frame) */
  applied = $state('');
  filter = $state<PmFilter>({ kind: 'all' });
  loaded = $state(false);
  /** the entry in the detail (also the keyboard cursor) */
  selectedId = $state<EntryId | null>(null);
  /** entries picked for a bulk action (always includes selectedId when there is one) */
  sel = $state.raw<Selection<EntryId>>(selEmpty());
  detail = $state<PmEntry | null>(null);
  copied = $state<PmCopiedState | null>(null);
  now = $state(Date.now());

  private frame: unknown = null;
  private ticker: ReturnType<typeof setInterval> | undefined;
  private reloadTimer: ReturnType<typeof setTimeout> | undefined;
  private unlisten: (() => void) | null = null;
  private disposed = false;

  constructor(vault: VaultId) {
    this.vault = vault;
  }

  /** lowercase search text per entry, parallel to `all` (rebuilt once per load) */
  index = $derived(this.all.map(haystack));

  /** every folder path and its parents, sorted */
  folders = $derived(folderTree(this.all));

  /** entries per filter, from one pass over the list */
  counts = $derived.by(() => {
    let favorites = 0;
    let totp = 0;
    const folder = new Map<string, number>();
    for (const e of this.all) {
      if (e.favorite) favorites++;
      if (e.has_totp) totp++;
      if (e.folder) {
        const parts = e.folder.split('/');
        for (let i = 1; i <= parts.length; i++) {
          const k = parts.slice(0, i).join('/');
          folder.set(k, (folder.get(k) ?? 0) + 1);
        }
      }
    }
    return { all: this.all.length, favorites, totp, folder };
  });

  /** what the list shows: the search (or everything), then the filter */
  visible = $derived.by(() => {
    const base = searchIn(this.all, this.index, this.applied);
    const f = this.filter;
    if (f.kind === 'favorites') return base.filter((e) => e.favorite);
    if (f.kind === 'totp') return base.filter((e) => e.has_totp);
    if (f.kind === 'folder') return base.filter((e) => inFolder(e.folder, f.folder));
    return base;
  });

  /** id -> position in `visible` (aria-posinset, keyboard, scroll) */
  positions = $derived.by(() => {
    const map = new Map<EntryId, number>();
    this.visible.forEach((e, i) => map.set(e.id, i));
    return map;
  });

  selected = $derived(this.selectedId ? (this.all.find((e) => e.id === this.selectedId) ?? null) : null);

  /** ids picked for a bulk action, in list order */
  picked = $derived(this.visible.filter((e) => this.sel.selected.has(e.id)).map((e) => e.id));

  countIn(f: PmFilter): number {
    const c = this.counts;
    if (f.kind === 'all') return c.all;
    if (f.kind === 'favorites') return c.favorites;
    if (f.kind === 'totp') return c.totp;
    return c.folder.get(f.folder) ?? 0;
  }

  isFilter(f: PmFilter) {
    return sameFilter(this.filter, f);
  }

  async load() {
    try {
      this.all = await api.pmList(this.vault);
      this.loaded = true;
      if (this.selectedId && !this.all.some((e) => e.id === this.selectedId)) void this.select(null);
      const ids = new Set(this.all.map((e) => e.id));
      if ([...this.sel.selected].some((id) => !ids.has(id))) this.sel = { ...this.sel, selected: new Set([...this.sel.selected].filter((id) => ids.has(id))) };
    } catch (e) {
      toasts.error(e);
    }
  }

  /** Starts listening for stored site icons and asks the backend for the ones that are due. */
  async startIcons() {
    if (!this.unlisten) {
      const un = await api.onPmIcons((p) => {
        if (p.vault !== this.vault || this.disposed) return;
        clearTimeout(this.reloadTimer);
        this.reloadTimer = setTimeout(() => void this.load(), 250);
      });
      if (this.disposed) un();
      else this.unlisten = un;
    }
    this.refreshIcons();
  }

  refreshIcons() {
    void api.pmIconsRefresh(this.vault).catch(() => undefined);
  }

  /** Search as you type: the box shows every key at once, the list follows on the next frame. */
  setQuery(q: string) {
    this.query = q;
    if (this.frame !== null) caf(this.frame);
    if (!q.trim()) {
      this.frame = null;
      this.applied = '';
      return;
    }
    this.frame = raf(() => {
      this.frame = null;
      this.applied = this.query;
    });
  }

  async select(id: EntryId | null, mods: Mods = {}) {
    if (id && (mods.shift || mods.toggle)) {
      this.sel = selClick(this.sel, this.visible.map((e) => e.id), id, mods);
      if (!this.sel.selected.has(id)) {
        // ctrl-click took it out: the detail keeps the last one still picked
        const rest = this.picked.at(-1) ?? null;
        if (rest !== this.selectedId) await this.show(rest);
        return;
      }
    } else {
      this.sel = id ? { selected: new Set([id]), anchor: id, focus: id } : selEmpty();
    }
    await this.show(id);
  }

  /** Right-click / drag start: keeps a multi-selection that contains `id`. */
  async touch(id: EntryId) {
    if (this.sel.selected.has(id)) return;
    this.sel = ensureSelected(this.sel, id);
    await this.show(id);
  }

  selectAllVisible() {
    this.sel = selectAll(this.sel, this.visible.map((e) => e.id));
  }

  private async show(id: EntryId | null) {
    this.selectedId = id;
    if (!id) {
      this.detail = null;
      return;
    }
    if (this.detail?.id === id) return;
    try {
      const d = await api.pmGet(this.vault, id);
      if (this.selectedId === id) this.detail = d;
    } catch (e) {
      toasts.error(e);
    }
  }

  /** Move the selection through the visible list (arrow keys; shift extends). */
  step(delta: number, mods: Mods = {}) {
    const list = this.visible;
    if (!list.length) return;
    const i = this.selectedId ? (this.positions.get(this.selectedId) ?? -1) : -1;
    const j = i < 0 ? (delta > 0 ? 0 : list.length - 1) : Math.max(0, Math.min(list.length - 1, i + delta));
    void this.select(list[j].id, mods.shift ? { shift: true } : {});
  }

  async reveal(field: PmSecretField): Promise<string | null> {
    if (!this.selectedId) return null;
    try {
      return await api.pmReveal(this.vault, this.selectedId, field);
    } catch (e) {
      toasts.error(e);
      return null;
    }
  }

  /** Copy a secret of the selected entry: the backend wipes the clipboard after its countdown. */
  async copy(field: PmSecretField, label: string, id: EntryId | null = this.selectedId) {
    if (!id) return;
    try {
      const r = await api.pmCopy(this.vault, id, field);
      play(field === 'totp_code' ? 'totp' : 'copy');
      this.copied = { label, field, until: r.clear_after > 0 ? Date.now() + r.clear_after * 1000 : null, seconds: r.clear_after };
      this.now = Date.now();
      clearInterval(this.ticker);
      this.ticker = setInterval(() => {
        this.now = Date.now();
        const c = this.copied;
        if (c?.until && this.now >= c.until) {
          clearInterval(this.ticker);
          this.copied = null;
          toasts.push(m.pm_cleared(), 'info');
        }
      }, 250);
    } catch (e) {
      toasts.error(e);
    }
  }

  /** seconds until the clipboard is wiped (null: no countdown) */
  copyLeft = $derived(this.copied?.until ? Math.max(0, Math.ceil((this.copied.until - this.now) / 1000)) : null);

  async save(input: PmEntryInput, id: EntryId | null): Promise<string | null> {
    try {
      const e = id ? await api.pmUpdate(this.vault, id, input) : await api.pmCreate(this.vault, input);
      await this.load();
      this.sel = { selected: new Set([e.id]), anchor: e.id, focus: e.id };
      this.selectedId = e.id;
      this.detail = e;
      toasts.push(m.pm_saved(), 'success');
      this.refreshIcons();
      return null;
    } catch (e) {
      return errorText(e);
    }
  }

  async toggleFavorite(id: EntryId | null = this.selectedId) {
    const e = this.all.find((x) => x.id === id);
    if (!e) return;
    try {
      const d = await api.pmUpdate(this.vault, e.id, { favorite: !e.favorite });
      await this.load();
      if (this.selectedId === e.id) this.detail = d;
    } catch (err) {
      toasts.error(err);
    }
  }

  async remove(id: EntryId) {
    try {
      await api.pmDelete(this.vault, id);
      if (this.selectedId === id) await this.select(null);
      await this.load();
      toasts.push(m.pm_deleted(), 'success');
    } catch (e) {
      toasts.error(e);
    }
  }

  private async refreshDetail() {
    if (!this.selectedId) return;
    try {
      const d = await api.pmGet(this.vault, this.selectedId);
      if (this.selectedId === d.id) this.detail = d;
    } catch {
      /* gone: load() clears it */
    }
  }

  /** "Sposta in cartella": `ids` into `folder` ("" = no folder). */
  async move(ids: EntryId[], folder: string): Promise<boolean> {
    if (!ids.length) return false;
    try {
      const n = await api.pmMove(this.vault, ids, folder);
      await this.load();
      await this.refreshDetail();
      const name = folder.split('/').filter(Boolean).join(' › ');
      toasts.push(folder ? m.pm_moved({ count: n, n: String(n), folder: name }) : m.pm_moved_out({ count: n, n: String(n) }), 'success');
      return true;
    } catch (e) {
      toasts.error(e);
      return false;
    }
  }

  async renameFolder(from: string, to: string): Promise<string | null> {
    const clean = to.split(/[/\\]/).map((x) => x.trim()).filter(Boolean).join('/');
    try {
      await api.pmFolderRename(this.vault, from, clean);
      if (this.filter.kind === 'folder' && inFolder(this.filter.folder, from)) this.filter = { kind: 'folder', folder: clean + this.filter.folder.slice(from.length) };
      await this.load();
      await this.refreshDetail();
      return null;
    } catch (e) {
      return errorText(e);
    }
  }

  async deleteFolder(folder: string) {
    try {
      await api.pmFolderDelete(this.vault, folder);
      if (this.filter.kind === 'folder' && inFolder(this.filter.folder, folder)) {
        const parent = folder.split('/').slice(0, -1).join('/');
        this.filter = parent ? { kind: 'folder', folder: parent } : { kind: 'all' };
      }
      await this.load();
      await this.refreshDetail();
      toasts.push(m.pm_folder_deleted(), 'success');
    } catch (e) {
      toasts.error(e);
    }
  }

  /** "Ripara voci importate" */
  async repair() {
    try {
      const n = await api.pmRepair(this.vault);
      await this.load();
      await this.refreshDetail();
      toasts.push(n ? m.pm_repaired({ count: n, n: String(n) }) : m.pm_repaired_none(), n ? 'success' : 'info');
      if (n) this.refreshIcons();
    } catch (e) {
      toasts.error(e);
    }
  }

  dispose() {
    this.disposed = true;
    if (this.frame !== null) caf(this.frame);
    clearInterval(this.ticker);
    clearTimeout(this.reloadTimer);
    this.unlisten?.();
    this.unlisten = null;
  }
}
