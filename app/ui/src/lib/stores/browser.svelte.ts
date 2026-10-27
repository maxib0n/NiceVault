import * as api from '../api';
import type { ImportSource } from '../api';
import { canPreview } from '../filetype';
import { isMobile } from '../platform';
import { itemCount } from '../format';
import { m } from '../../paraglide/messages.js';
import * as S from '../selection';
import { terms } from '../search';
import { sortNodes, type SortSpec } from '../sort';
import type { FsNode, NodeId, SearchSort, TagId, VaultInfo } from '../types';
import { ROOT } from '../types';
import { photos } from './photos.svelte';
import { space } from './space.svelte';
import { gallery } from './gallery.svelte';
import { isGalleryMedia } from '../gallery';
import { jobs, type Job } from './jobs.svelte';
import { prefs } from './prefs.svelte';
import { toasts } from './toasts.svelte';
import { errorText } from '../errors';
import { apiFilters, hasFilters, remember, rememberedFor, VolumeTags, type SearchScope, type UiFilters } from './search.svelte';

export interface Crumb {
  id: NodeId;
  name: string;
}

export interface HistoryPlace {
  trail: Crumb[];
  selected: NodeId[];
  focus: NodeId | null;
  scroll: number;
}

export interface MenuState {
  x: number;
  y: number;
  target: NodeId | null;
}

export interface HitMeta {
  path: string;
  highlights: [number, number][];
}

export interface Anchor {
  x: number;
  y: number;
}

const SEARCH_LIMIT = 1000;
const SEARCH_DEBOUNCE = 120;

export class FileBrowser {
  readonly vault: VaultInfo;

  trail = $state<Crumb[]>([]);
  entries = $state.raw<FsNode[]>([]);
  loading = $state(false);
  loaded = $state(false);
  sel = $state.raw<S.Selection<NodeId>>(S.empty());
  renaming = $state<NodeId | null>(null);
  selecting = $state(false);
  quickLook = $state<NodeId | null>(null);
  menu = $state<MenuState | null>(null);
  confirmDelete = $state.raw<NodeId[] | null>(null);
  props = $state.raw<NodeId[] | null>(null);
  dragOver = $state(false);
  back = $state.raw<HistoryPlace[]>([]);
  forward = $state.raw<HistoryPlace[]>([]);
  scrollPos = 0;
  scrollRestore = $state<number | null>(null);

  readonly tags: VolumeTags;
  searchOpen = $state(false);
  searchFocus = $state(0);
  query = $state('');
  appliedQuery = $state('');
  scope = $state<SearchScope>('volume');
  filters = $state.raw<UiFilters>({ kinds: [], size: null, date: null, tags: [], tagsAny: false });
  resultSort = $state.raw<SortSpec | null>(null);
  hitMeta = $state.raw<Map<NodeId, HitMeta>>(new Map());
  hitTotal = $state(0);
  searching = $state(false);
  searchError = $state<string | null>(null);
  indexing = $state.raw<{ done: number; total: number } | null>(null);
  tagger = $state.raw<(Anchor & { nodes: NodeId[] }) | null>(null);
  tagManager = $state.raw<Anchor | null>(null);

  resultsMode = $derived(terms(this.appliedQuery).length > 0);
  filtering = $derived(hasFilters(this.filters));

  cwd = $derived(this.trail[this.trail.length - 1]?.id ?? ROOT);
  sorted = $derived(this.resultsMode ? this.entries : sortNodes(this.entries, prefs.sort));
  order = $derived(this.sorted.map((n) => n.id));
  byId = $derived(new Map(this.entries.map((n) => [n.id, n])));
  selectedIds = $derived(S.ordered(this.sel, this.order));
  selectedNodes = $derived(this.selectedIds.map((id) => this.byId.get(id)!).filter(Boolean));
  focused = $derived(this.sel.focus !== null ? (this.byId.get(this.sel.focus) ?? null) : null);
  previewNode = $derived(this.quickLook !== null ? (this.byId.get(this.quickLook) ?? null) : null);
  previewable = $derived(this.sorted.filter((n) => n.kind === 'file' && canPreview(n)));

  private disposeJobs: () => void;
  changes = $state(0);
  private disposeFs: (() => void) | null = null;
  private disposed = false;
  private liveDirs = new Set<NodeId>();
  private liveRun: Promise<unknown> = Promise.resolve();
  private disposeSearch: (() => void)[] = [];
  private searchSeq = 0;
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  private warmed = false;

  constructor(vault: VaultInfo) {
    this.vault = vault;
    this.trail = [{ id: ROOT, name: vault.name }];
    this.tags = new VolumeTags(vault.id);
    const kept = rememberedFor(vault.path);
    this.filters = kept.filters;
    this.scope = kept.scope;
    void this.tags.load();
    const keep = (p: Promise<() => void>) => void p.then((un) => (this.disposed ? un() : this.disposeSearch.push(un)));
    keep(
      api.onTagsChanged((e) => {
        if (e.vault === this.vault.id) void this.tagsChanged(e.nodes);
      }),
    );
    keep(
      api.onSearchIndex((e) => {
        if (e.vault === this.vault.id) this.indexing = e.done >= e.total ? null : { done: e.done, total: e.total };
      }),
    );
    this.disposeJobs = jobs.onFinished((j) => {
      if (j.vault === this.vault.id && j.kind === 'import') {
        void this.load();
        if (j.status === 'done') toasts.push(m.toast_imported({ label: j.label }), 'success');
      }
      if (j.vault === this.vault.id && j.kind === 'delete') this.deleteFinished(j);
      else if (j.vault === this.vault.id && j.status === 'error') {
        toasts.push((j.kind === 'import' ? m.toast_import_failed : m.toast_export_failed)({ error: j.error ?? m.err_generic() }), 'error');
      }
      if (j.vault === this.vault.id && j.kind === 'export' && j.status === 'done') {
        toasts.push(m.toast_exported({ place: j.target ?? '' }), 'success');
      }
    });
    void api
      .onFsChanged((e) => {
        if (e.vault === this.vault.id) void this.externalChange(e.dirs);
      })
      .then((un) => (this.disposed ? un() : (this.disposeFs = un)));
  }

  dispose() {
    this.disposed = true;
    this.disposeJobs();
    this.disposeFs?.();
    for (const un of this.disposeSearch) un();
    clearTimeout(this.searchTimer);
  }

  async load() {
    if (this.resultsMode) return this.runSearch();
    this.loading = true;
    try {
      const dir = this.cwd;
      const list = await api.fsList(this.vault.id, dir, this.folderFilters());
      if (dir !== this.cwd) return;
      this.entries = list;
      this.sel = S.prune(this.sel, this.order);
      this.loaded = true;
    } catch (e) {
      toasts.error(e);
    } finally {
      this.loading = false;
    }
  }

  private here(): HistoryPlace {
    return { trail: this.trail, selected: this.selectedIds, focus: this.sel.focus, scroll: this.scrollPos };
  }

  private remember() {
    this.back = [...this.back, this.here()].slice(-50);
    this.forward = [];
  }

  async goBack() {
    const to = this.back[this.back.length - 1];
    if (!to) return;
    this.back = this.back.slice(0, -1);
    this.forward = [...this.forward, this.here()];
    await this.restore(to);
  }

  async goForward() {
    const to = this.forward[this.forward.length - 1];
    if (!to) return;
    this.forward = this.forward.slice(0, -1);
    this.back = [...this.back, this.here()];
    await this.restore(to);
  }

  private async restore(p: HistoryPlace) {
    this.trail = p.trail;
    this.resetTransient();
    await this.load();
    const keep = p.selected.filter((id) => this.byId.has(id));
    const focus = p.focus !== null && this.byId.has(p.focus) ? p.focus : (keep[keep.length - 1] ?? null);
    if (keep.length || focus !== null) this.sel = { selected: new Set(keep), anchor: focus, focus };
    this.scrollRestore = p.scroll;
  }

  async openDir(node: Pick<FsNode, 'id' | 'name'>) {
    this.remember();
    this.trail = [...this.trail, { id: node.id, name: node.name }];
    this.resetTransient();
    await this.load();
  }

  async up() {
    if (this.resultsMode) return this.closeSearch();
    if (this.trail.length <= 1) return;
    const from = this.trail[this.trail.length - 1];
    this.remember();
    this.trail = this.trail.slice(0, -1);
    this.resetTransient();
    await this.load();
    if (this.byId.has(from.id)) this.sel = { selected: new Set([from.id]), anchor: from.id, focus: from.id };
  }

  async gotoCrumb(index: number) {
    if (index >= this.trail.length - 1) return;
    this.remember();
    this.trail = this.trail.slice(0, index + 1);
    this.resetTransient();
    await this.load();
  }

  async gotoPath(path: string) {
    this.trail = this.trail.slice(0, 1);
    await this.load();
    for (const part of path.split('/').filter(Boolean)) {
      const d = this.entries.find((n) => n.kind === 'dir' && n.name === part);
      if (!d) break;
      await this.openDir(d);
    }
    this.back = [];
    this.forward = [];
  }

  private resetTransient() {
    this.endSearch();
    this.sel = S.empty();
    this.selecting = false;
    this.renaming = null;
    this.quickLook = null;
    this.menu = null;
    this.props = null;
    this.entries = [];
    this.loaded = false;
  }

  async activate(id: NodeId) {
    const n = this.byId.get(id);
    if (!n) return;
    if (n.kind === 'dir' && this.resultsMode) await this.openHitPlace(id, true);
    else if (n.kind === 'dir') await this.openDir(n);
    else if (canPreview(n)) this.quickLook = id;
    else await this.openExternal(id);
  }

  click(id: NodeId, mods: S.Mods, tap = false) {
    if (tap && isMobile && !mods.shift && !mods.toggle) {
      if (this.selecting || this.selectedIds.length > 1) {
        this.sel = S.click(this.sel, this.order, id, { toggle: true });
        if (this.selectedIds.length === 0) this.selecting = false;
        return;
      }
      this.sel = S.click(this.sel, this.order, id, {});
      void this.activate(id);
      return;
    }
    this.sel = S.click(this.sel, this.order, id, mods);
  }
  move(delta: number, mods: S.Mods = {}) {
    this.sel = S.move(this.sel, this.order, delta, mods);
  }
  moveTo(index: number, mods: S.Mods = {}) {
    this.sel = S.moveTo(this.sel, this.order, index, mods);
  }
  selectAll() {
    this.sel = S.selectAll(this.sel, this.order);
  }
  startSelecting(id: NodeId | null = this.sel.focus) {
    this.menu = null;
    this.selecting = true;
    if (id !== null) this.sel = S.ensureSelected(this.sel, id);
  }
  endSelecting() {
    this.selecting = false;
    this.sel = S.clear(this.sel);
  }
  clearSelection() {
    this.sel = S.clear(this.sel);
  }
  toggleFocus() {
    this.sel = S.toggleFocus(this.sel);
  }

  openMenu(x: number, y: number, target: NodeId | null) {
    if (target !== null) this.sel = S.ensureSelected(this.sel, target);
    this.menu = { x, y, target };
  }
  closeMenu() {
    this.menu = null;
  }

  showProps(ids: NodeId[] = this.selectedIds) {
    this.menu = null;
    this.props = ids.length ? [...ids] : [this.cwd];
  }

  toggleQuickLook() {
    if (this.quickLook !== null) {
      this.quickLook = null;
      return;
    }
    const n = this.focused ?? this.selectedNodes[0];
    if (n && n.kind === 'file') this.quickLook = n.id;
  }
  stepQuickLook(delta: number) {
    const list = this.previewable;
    if (list.length === 0 || this.quickLook === null) return;
    const i = list.findIndex((n) => n.id === this.quickLook);
    const next = list[(i + delta + list.length) % list.length];
    this.quickLook = next.id;
    this.sel = { selected: new Set([next.id]), anchor: next.id, focus: next.id };
  }

  private uniqueName(base: string): string {
    const taken = new Set(this.entries.map((n) => n.name));
    if (!taken.has(base)) return base;
    for (let k = 2; ; k++) if (!taken.has(`${base} ${k}`)) return `${base} ${k}`;
  }

  async newFolder() {
    if (this.resultsMode) return;
    try {
      const node = await api.fsMkdir(this.vault.id, this.cwd, this.uniqueName(m.new_folder_name()));
      await this.load();
      this.sel = { selected: new Set([node.id]), anchor: node.id, focus: node.id };
      this.renaming = node.id;
    } catch (e) {
      toasts.error(e);
    }
  }

  startRename(id: NodeId | null = this.sel.focus) {
    if (id !== null && this.byId.has(id)) this.renaming = id;
  }

  async commitRename(id: NodeId, name: string): Promise<boolean> {
    const n = this.byId.get(id);
    const clean = name.trim();
    if (!n || clean === n.name || clean === '') {
      this.renaming = null;
      return true;
    }
    try {
      await api.fsRename(this.vault.id, id, n.parent, clean);
      this.renaming = null;
      await this.load();
      return true;
    } catch (e) {
      toasts.error(e);
      return false;
    }
  }

  cancelRename() {
    this.renaming = null;
  }

  requestDelete(ids: NodeId[] = this.selectedIds) {
    if (ids.length) this.confirmDelete = ids;
  }

  async confirmDeleteNow() {
    const ids = this.confirmDelete;
    this.confirmDelete = null;
    if (!ids?.length) return;
    try {
      const nodes = ids.map((id) => this.byId.get(id)!).filter(Boolean);
      const job = await api.fsDelete(this.vault.id, ids);
      if (this.quickLook !== null && ids.includes(this.quickLook)) this.quickLook = null;
      jobs.track(job, 'delete', this.vault.id, nodes.length ? this.labelFor(nodes) : itemCount(ids.length));
    } catch (e) {
      toasts.error(e);
    }
  }

  private deleteFinished(j: Job) {
    void this.load();
    const n = j.outcome?.deleted ?? j.done_items;
    if (j.status === 'done') {
      toasts.push(n === 1 ? m.toast_deleted() : m.toast_deleted_n({ count: n, n: String(n) }), 'success');
    } else if (j.status === 'cancelled') {
      toasts.push(m.toast_delete_cancelled({ n: String(n), total: String(j.total_items || j.outcome?.total || n) }), 'info');
    } else {
      toasts.push(m.toast_delete_failed({ error: j.error ?? m.err_generic() }), 'error');
    }
  }

  async moveInto(ids: NodeId[], dir: NodeId) {
    const movable = ids.filter((id) => id !== dir);
    try {
      for (const id of movable) {
        const n = this.byId.get(id);
        if (n) await api.fsRename(this.vault.id, id, dir, n.name);
      }
      await this.load();
    } catch (e) {
      toasts.error(e);
      await this.load();
    }
  }

  isMedia(id: NodeId): boolean {
    const n = this.byId.get(id);
    return !!n && isGalleryMedia(n);
  }

  async saveToGallery(ids: NodeId[] = this.selectedIds) {
    const media = ids.filter((id) => this.isMedia(id));
    if (media.length) await gallery.save(this.vault.id, media);
  }

  labelFor(nodes: Pick<FsNode, 'name'>[]): string {
    return nodes.length === 1 ? nodes[0].name : itemCount(nodes.length);
  }

  async exportNodes(ids: NodeId[] = this.selectedIds) {
    if (!ids.length) return;
    if (isMobile && ids.some((id) => this.isMedia(id))) {
      const on = await gallery.enabled();
      if (on === null) return;
      if (on) {
        const media = ids.filter((id) => this.isMedia(id));
        const left = await gallery.save(this.vault.id, media);
        if (left === null) return;
        ids = ids.filter((id) => !media.includes(id)).concat(left);
        if (!ids.length) return;
      }
    }
    const dest = await api.pickFolder(m.dialog_export_to());
    if (!dest) return;
    try {
      const job = await api.fsExport(this.vault.id, ids, dest);
      const nodes = ids.map((id) => this.byId.get(id)!).filter(Boolean);
      jobs.track(job, 'export', this.vault.id, this.labelFor(nodes), dest);
    } catch (e) {
      toasts.error(e);
    }
  }

  async importSources(sources: ImportSource[], parent: NodeId = this.cwd) {
    if (!sources.length) return;
    if (!(await space.check(this.vault.id, sources))) return;
    try {
      const job = await api.fsImport(this.vault.id, parent, sources);
      const names = sources.map((s) => ({ name: typeof s === 'string' ? (s.split(/[\\/]/).pop() ?? s) : s.name }));
      const folder = this.trail.find((c) => c.id === parent)?.name ?? this.byId.get(parent)?.name ?? this.vault.name;
      jobs.track(job, 'import', this.vault.id, this.labelFor(names), folder);
    } catch (e) {
      toasts.error(e);
    }
  }

  async importViaDialog() {
    const paths = await api.pickImportFiles();
    await this.importSources(paths);
  }

  async importPhotos() {
    const folder = this.trail.find((c) => c.id === this.cwd)?.name ?? this.vault.name;
    await photos.start(this.vault.id, this.cwd, folder);
  }

  async openExternal(id: NodeId | null = this.sel.focus) {
    if (id === null) return;
    try {
      const r = await api.fsOpenExternal(this.vault.id, id);
      if (r?.host_disk_notice) toasts.push(m.toast_host_copy(), 'info', 10000);
    } catch (e) {
      toasts.error(e);
    }
  }

  externalChange(dirs: NodeId[]): Promise<boolean> {
    this.changes++;
    for (const d of dirs) this.liveDirs.add(d);
    const run = this.liveRun.then(() => this.applyExternal());
    this.liveRun = run.catch(() => {});
    return run;
  }

  private async applyExternal(): Promise<boolean> {
    const dirs = this.liveDirs;
    this.liveDirs = new Set();
    if (this.resultsMode) return this.refresh();
    let trail = this.trail;
    const shown = trail.some((c) => dirs.has(c.id)) || this.entries.some((n) => n.kind === 'dir' && dirs.has(n.id));
    if (!shown || !this.loaded) return false;
    for (let i = 0; i < trail.length - 1; i++) {
      if (!dirs.has(trail[i].id)) continue;
      let kids: FsNode[];
      try {
        kids = await api.fsList(this.vault.id, trail[i].id);
      } catch {
        return false;
      }
      if (this.trail !== trail) return false;
      const next = kids.find((k) => k.id === trail[i + 1].id && k.kind === 'dir');
      if (!next) {
        this.trail = trail.slice(0, i + 1);
        this.resetTransient();
        await this.load();
        return true;
      }
      if (next.name !== trail[i + 1].name) {
        this.trail = trail.map((c, j) => (j === i + 1 ? { ...c, name: next.name } : c));
        trail = this.trail;
      }
    }
    return this.refresh();
  }

  async refresh(): Promise<boolean> {
    if (this.resultsMode) {
      await this.runSearch();
      return true;
    }
    const dir = this.cwd;
    let list: FsNode[];
    try {
      list = await api.fsList(this.vault.id, dir, this.folderFilters());
    } catch {
      return false;
    }
    if (dir !== this.cwd) return false;
    this.entries = list;
    this.sel = S.prune(this.sel, this.order);
    const gone = (id: NodeId | null) => id !== null && !this.byId.has(id);
    if (gone(this.renaming)) this.renaming = null;
    if (gone(this.quickLook)) this.quickLook = null;
    if (this.menu && gone(this.menu.target)) this.menu = null;
    if (this.confirmDelete) {
      const left = this.confirmDelete.filter((id) => this.byId.has(id));
      this.confirmDelete = left.length ? left : null;
    }
    return true;
  }

  private folderFilters() {
    return this.filtering ? apiFilters(this.filters) : null;
  }

  openSearch(focus = true) {
    this.searchOpen = true;
    if (focus) this.searchFocus++;
    if (this.warmed) return;
    this.warmed = true;
    void api
      .fsSearch(this.vault.id, '', { limit: 1 })
      .then((r) => {
        if (r.pending > 0 && !this.indexing) this.indexing = { done: 0, total: r.pending };
      })
      .catch(() => {});
  }

  closeSearch() {
    clearTimeout(this.searchTimer);
    this.searchOpen = false;
    this.query = '';
    void this.applyQuery();
  }

  setQuery(q: string, now = false) {
    this.query = q;
    clearTimeout(this.searchTimer);
    if (now || terms(q).length === 0) void this.applyQuery();
    else this.searchTimer = setTimeout(() => void this.applyQuery(), SEARCH_DEBOUNCE);
  }

  private async applyQuery() {
    const want = terms(this.query).length ? this.query : '';
    if (want) return this.runSearch();
    if (!this.appliedQuery) return;
    this.searchSeq++;
    this.searching = false;
    this.searchError = null;
    this.appliedQuery = '';
    this.hitMeta = new Map();
    this.sel = S.empty();
    this.menu = null;
    this.quickLook = null;
    await this.load();
  }

  setScope(s: SearchScope) {
    if (s === this.scope) return;
    this.scope = s;
    remember(this.vault.path, this.filters, s);
    if (this.resultsMode || terms(this.query).length) void this.runSearch();
  }

  setFilters(f: UiFilters) {
    this.filters = f;
    remember(this.vault.path, f, this.scope);
    if (this.resultsMode || terms(this.query).length) void this.runSearch();
    else void this.reloadFiltered();
  }

  private async reloadFiltered() {
    const keep = this.sel;
    await this.load();
    this.sel = S.prune(keep, this.order);
  }

  sortResults(spec: SortSpec | null) {
    this.resultSort = spec;
    if (this.resultsMode) void this.runSearch();
  }

  async runSearch(): Promise<void> {
    const q = this.query;
    if (!terms(q).length) return this.applyQuery();
    const my = ++this.searchSeq;
    this.searching = true;
    const sort: SearchSort = this.resultSort?.key ?? 'relevance';
    try {
      const r = await api.fsSearch(this.vault.id, q, {
        filters: apiFilters(this.filters, this.scope === 'folder' ? this.cwd : null),
        sort,
        desc: this.resultSort?.dir === 'desc',
        limit: SEARCH_LIMIT,
      });
      if (my !== this.searchSeq) return;
      const fresh = !this.appliedQuery;
      this.appliedQuery = q;
      this.entries = r.hits.map((h) => (h.tags.length ? { ...h.node, tags: h.tags } : h.node));
      this.hitMeta = new Map(r.hits.map((h) => [h.node.id, { path: h.path, highlights: h.highlights }]));
      this.hitTotal = r.total;
      this.searchError = null;
      this.loaded = true;
      if (fresh) {
        this.sel = S.empty();
        this.menu = null;
        this.renaming = null;
      } else this.sel = S.prune(this.sel, this.order);
      if (this.quickLook !== null && !this.byId.has(this.quickLook)) this.quickLook = null;
      if (r.pending > 0 && !this.indexing) this.indexing = { done: 0, total: r.pending };
    } catch (e) {
      if (my !== this.searchSeq) return;
      this.appliedQuery = q;
      this.entries = [];
      this.hitMeta = new Map();
      this.hitTotal = 0;
      this.searchError = errorText(e);
      this.loaded = true;
    } finally {
      if (my === this.searchSeq) this.searching = false;
    }
  }

  async openHitPlace(id: NodeId, into = false) {
    const meta = this.hitMeta.get(id);
    const node = this.byId.get(id);
    if (!meta || !node) return;
    const trail: Crumb[] = [{ id: ROOT, name: this.vault.name }];
    try {
      for (const part of meta.path.split('/').filter(Boolean)) {
        const kids = await api.fsList(this.vault.id, trail[trail.length - 1].id);
        const d = kids.find((k) => k.kind === 'dir' && k.name === part);
        if (!d) return;
        trail.push({ id: d.id, name: d.name });
      }
    } catch (e) {
      toasts.error(e);
      return;
    }
    this.remember();
    this.searchOpen = false;
    this.trail = into ? [...trail, { id: node.id, name: node.name }] : trail;
    this.resetTransient();
    await this.load();
    if (!into && this.byId.has(id)) this.sel = { selected: new Set([id]), anchor: id, focus: id };
  }

  private endSearch() {
    if (!this.appliedQuery && !this.query) return;
    clearTimeout(this.searchTimer);
    this.searchSeq++;
    this.searching = false;
    this.searchOpen = false;
    this.query = '';
    this.appliedQuery = '';
    this.hitMeta = new Map();
    this.searchError = null;
  }

  openTagger(anchor: Anchor, nodes: NodeId[] = this.selectedIds) {
    this.menu = null;
    if (nodes.length) this.tagger = { ...anchor, nodes: [...nodes] };
  }

  private async tagsChanged(nodes: NodeId[]) {
    await this.tags.load();
    const known = this.tags.byId;
    const f = this.filters;
    if (f.tags.some((t) => !known.has(t))) {
      this.filters = { ...f, tags: f.tags.filter((t) => known.has(t)) };
      remember(this.vault.path, this.filters, this.scope);
      void this.refresh();
      return;
    }
    if (f.tags.length) {
      void this.refresh();
      return;
    }
    const on = nodes.filter((id) => this.byId.has(id));
    if (!on.length) return;
    let of: Record<string, TagId[]>;
    try {
      of = await api.tagsOf(this.vault.id, on);
    } catch {
      return;
    }
    const changed = new Set(on);
    this.entries = this.entries.map((n) => {
      if (!changed.has(n.id)) return n;
      const tags = of[String(n.id)] ?? [];
      const { tags: _old, ...rest } = n;
      return tags.length ? { ...rest, tags } : rest;
    });
  }
}
