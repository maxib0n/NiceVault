<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { isMobile } from '../lib/platform';
  import { ArrowDown, ArrowUp, FolderOpen, SearchX, ListFilter, TriangleAlert } from '@lucide/svelte';
  import { untrack } from 'svelte';
  import { pointerLast } from '../lib/modality';
  import { formatBytes, itemCount } from '../lib/format';
  import { sortLabel, type SortKey } from '../lib/sort';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import { prefs } from '../lib/stores/prefs.svelte';
  import { decode, firstTime, skeleton } from '../lib/reveal';
  import { COMPOSE_AT, ceremony } from './ceremony';
  import FileItem from './FileItem.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import { ROW, TILE_MIN, viewSize } from '../lib/stores/viewsize.svelte';

  let { browser, view = $bindable() }: { browser: FileBrowser; view?: HTMLDivElement } = $props();

  const results = $derived(browser.resultsMode);
  const layout = $derived(results ? 'list' : prefs.view);
  // svelte-ignore state_referenced_locally
  const SEARCH_KEY = `search:${browser.vault.id}`;
  firstTime(SEARCH_KEY);
  const gridSize = $derived(viewSize.grid(browser.vault.id));
  const density = $derived(viewSize.density(browser.vault.id));
  const row = $derived(ROW[density]);
  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

  const reveal = $derived.by(() => {
    const folder = `${browser.vault.id}:${browser.cwd}`;
    const common = { ready: browser.loaded, selector: '.items', items: '.file-item', fades: '[data-ascii="fade"]' };
    if (results) return { ...common, key: SEARCH_KEY, pointerOnly: true };
    const c = browser.loaded ? ceremony() : null;
    const sinceUnlock = performance.now() - vaults.unlockedAt;
    const slow = c?.slow ?? 1;
    const age = c && c.age < sinceUnlock ? c.age : sinceUnlock;
    return browser.loaded && age < 900 * slow && browser.trail.length === 1
      ? { ...common, key: `ceremony:${Math.round(vaults.unlockedAt)}`, also: folder, speed: 1.1, delay: Math.max(0, COMPOSE_AT * slow - age) }
      : { ...common, key: folder, speed: 1.3, pointerOnly: true };
  });

  let typeahead = '';
  let typeaheadTimer: ReturnType<typeof setTimeout> | undefined;

  const VIRTUAL_MIN = 150;
  const FIRST = 72;
  const OVERSCAN = 3;
  const STEP = 3;
  let itemsEl: HTMLDivElement | undefined = $state();
  let viewH = $state(0);
  let viewW = $state(0);
  let cols = $state(1);
  let pitch = $state(0);
  let tileH = $state(0);
  let firstRow = $state(0);
  let measure: CanvasRenderingContext2D | null = null;
  let fontsTick = $state(0);
  if (typeof document !== 'undefined' && document.fonts) void document.fonts.ready.then(() => fontsTick++);
  const total = $derived(browser.sorted.length);
  const big = $derived(total >= VIRTUAL_MIN);
  const virtual = $derived(big && pitch > 0);
  const rowsTotal = $derived(Math.ceil(total / cols));
  const spanRows = $derived(Math.ceil(viewH / Math.max(1, pitch)) + 1);
  const fromRow = $derived(Math.max(0, Math.min(firstRow, rowsTotal - spanRows) - OVERSCAN));
  const toRow = $derived(Math.min(rowsTotal, firstRow + STEP + spanRows + OVERSCAN));
  const shown = $derived(!big ? browser.sorted : virtual ? browser.sorted.slice(fromRow * cols, toRow * cols) : browser.sorted.slice(0, FIRST));

  const itemsTop = () =>
    itemsEl && view ? itemsEl.getBoundingClientRect().top - view.getBoundingClientRect().top + view.scrollTop : 0;

  function onScroll() {
    if (!view) return;
    browser.scrollPos = view.scrollTop;
    if (!virtual) return;
    const raw = Math.max(0, Math.floor((view.scrollTop - itemsTop()) / pitch));
    const f = raw - (raw % STEP);
    if (f !== firstRow) firstRow = f;
  }

  $effect(() => {
    void viewW;
    void fontsTick;
    void layout;
    void gridSize;
    void density;
    const el = itemsEl;
    if (!big || !el || !browser.loaded) {
      pitch = 0;
      return;
    }
    const first = el.querySelector<HTMLElement>('.file-item');
    if (!first) return;
    const cs = getComputedStyle(el);
    const name = first.querySelector<HTMLElement>('.name');
    const ncs = name ? getComputedStyle(name) : null;
    const lh = ncs ? parseFloat(ncs.lineHeight) || 0 : 0;
    let lines = 1;
    const clamp = ncs ? parseInt(ncs.getPropertyValue('-webkit-line-clamp'), 10) || 2 : 1;
    const ctx = ncs && name && clamp > 1 ? (measure ??= document.createElement('canvas').getContext('2d')) : null;
    if (ctx && ncs && name) {
      ctx.font = `${ncs.fontStyle} ${ncs.fontWeight} ${ncs.fontSize} ${ncs.fontFamily}`;
      const room = name.clientWidth - 1;
      if (browser.sorted.some((n) => ctx.measureText(n.name).width > room)) lines = clamp;
    }
    if (layout === 'grid') {
      const n = cs.gridTemplateColumns.split(' ').filter(Boolean).length;
      const h = Math.ceil(first.offsetHeight - (name?.offsetHeight ?? 0) + lines * lh);
      const gap = parseFloat(cs.rowGap) || 0;
      cols = Math.max(1, n);
      tileH = h;
      pitch = h + gap;
    } else {
      const fcs = getComputedStyle(first);
      const px = (v: string) => parseFloat(v) || 0;
      const padY = px(fcs.paddingTop) + px(fcs.paddingBottom) + px(fcs.borderTopWidth) + px(fcs.borderBottomWidth);
      const vis = first.querySelector<HTMLElement>('.visual')?.offsetHeight ?? 0;
      const h = Math.ceil(Math.max(px(fcs.minHeight), padY + Math.max(vis, lines * lh)));
      cols = 1;
      tileH = h;
      pitch = h;
    }
    untrack(onScroll);
  });

  $effect(() => {
    void browser.sorted;
    void pitch;
    void viewH;
    if (!big) {
      firstRow = 0;
      return;
    }
    const id = requestAnimationFrame(() => untrack(onScroll));
    return () => cancelAnimationFrame(id);
  });

  function scrollToIndex(index: number) {
    if (!view || !virtual) return;
    const y = itemsTop() + Math.floor(index / cols) * pitch;
    if (y < view.scrollTop) view.scrollTop = y;
    else if (y + pitch > view.scrollTop + view.clientHeight) view.scrollTop = y + pitch - view.clientHeight;
    onScroll();
  }

  function columns(): number {
    if (virtual) return cols;
    if (layout === 'list' || !view) return 1;
    const items = view.querySelectorAll<HTMLElement>('.file-item');
    if (items.length < 2) return 1;
    const top = items[0].offsetTop;
    let n = 0;
    for (const it of items) {
      if (it.offsetTop !== top) break;
      n++;
    }
    return Math.max(1, n);
  }

  function pageSize(): number {
    if (!view) return 10;
    const first = view.querySelector<HTMLElement>('.file-item');
    const h = first?.offsetHeight || 30;
    return Math.max(1, Math.floor(view.clientHeight / h)) * columns();
  }

  $effect(() => {
    const to = browser.scrollRestore;
    if (to === null || !browser.loaded || !view) return;
    const v = view;
    requestAnimationFrame(() => {
      v.scrollTop = to;
      browser.scrollRestore = null;
    });
  });

  $effect(() => {
    const f = browser.sel.focus;
    if (f === null || !view) return;
    const el = view.querySelector(`#item-${f}`);
    if (el && pointerLast()) {
      const v = view;
      requestAnimationFrame(() => {
        if (!el.isConnected) return;
        const r = el.getBoundingClientRect();
        const b = v.getBoundingClientRect();
        if (r.top < b.top || r.bottom > b.bottom) el.scrollIntoView({ block: 'nearest' });
      });
    } else if (el) el.scrollIntoView({ block: 'nearest' });
    else scrollToIndex(browser.sorted.findIndex((n) => n.id === f));
  });

  function keydown(e: KeyboardEvent) {
    if (browser.renaming !== null) return;
    const mod = isMac ? e.metaKey : e.ctrlKey;
    const mods = { shift: e.shiftKey, toggle: mod };
    const cols = columns();
    let handled = true;
    switch (e.key) {
      case 'ArrowRight':
        if (e.altKey) handled = false;
        else if (layout === 'grid') browser.move(1, mods);
        else handled = false;
        break;
      case 'ArrowLeft':
        if (e.altKey) handled = false;
        else if (layout === 'grid') browser.move(-1, mods);
        else handled = false;
        break;
      case 'ArrowDown':
        browser.move(cols, mods);
        break;
      case 'ArrowUp':
        if (e.altKey) void browser.up();
        else browser.move(-cols, mods);
        break;
      case 'Home':
        browser.moveTo(0, mods);
        break;
      case 'End':
        browser.moveTo(browser.order.length - 1, mods);
        break;
      case 'PageDown':
        browser.move(pageSize(), mods);
        break;
      case 'PageUp':
        browser.move(-pageSize(), mods);
        break;
      case 'Enter':
        if (e.altKey) browser.showProps(browser.selectedIds.length ? browser.selectedIds : browser.focused ? [browser.focused.id] : []);
        else if (browser.sel.focus !== null) void browser.activate(browser.sel.focus);
        break;
      case 'Backspace':
        if (isMac && e.metaKey) browser.requestDelete();
        else void browser.up();
        break;
      case 'Delete':
        browser.requestDelete();
        break;
      case 'F2':
        browser.startRename();
        break;
      case ' ':
        if (mod) browser.toggleFocus();
        else browser.toggleQuickLook();
        break;
      case 'Escape':
        if (results && browser.sel.selected.size === 0) browser.closeSearch();
        else browser.clearSelection();
        break;
      case 'ContextMenu':
        openMenuAtFocus();
        break;
      case 'F10':
        if (e.shiftKey) openMenuAtFocus();
        else handled = false;
        break;
      default:
        handled = false;
        if (mod && e.key.toLowerCase() === 'a') {
          browser.selectAll();
          handled = true;
        } else if (mod && e.shiftKey && e.key.toLowerCase() === 'n') {
          void browser.newFolder();
          handled = true;
        } else if (e.key.length === 1 && !mod && !e.altKey) {
          jump(e.key);
          handled = true;
        }
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  }

  function ctrlWheel(el: HTMLElement) {
    let acc = 0;
    let last = 0;
    const wheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const now = performance.now();
      if (now - last > 400) acc = 0;
      last = now;
      acc += e.deltaMode === 1 ? e.deltaY * 40 : e.deltaY;
      if (Math.abs(acc) >= 80 || (e.deltaMode === 1 && acc !== 0)) {
        viewSize.zoom(browser.vault.id, layout, acc < 0 ? 1 : -1);
        acc = 0;
      }
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return { destroy: () => el.removeEventListener('wheel', wheel) };
  }

  function jump(ch: string) {
    clearTimeout(typeaheadTimer);
    typeahead += ch.toLowerCase();
    typeaheadTimer = setTimeout(() => (typeahead = ''), 700);
    const i = browser.sorted.findIndex((n) => n.name.toLowerCase().startsWith(typeahead));
    if (i >= 0) browser.moveTo(i);
  }

  function openMenuAtFocus() {
    const id = browser.sel.focus;
    const el = id !== null ? view?.querySelector<HTMLElement>(`#item-${id}`) : null;
    const r = (el ?? view)?.getBoundingClientRect();
    if (r) browser.openMenu(r.left + 16, r.top + Math.min(r.height, 24), id);
  }

  function backgroundClick(e: MouseEvent) {
    if (e.target === e.currentTarget && !e.shiftKey && !e.ctrlKey && !e.metaKey) browser.clearSelection();
  }

  function backgroundMenu(e: MouseEvent) {
    e.preventDefault();
    if (!(e.target as HTMLElement).closest('[role="option"], button, a, input')) {
      browser.clearSelection();
      browser.openMenu(e.clientX, e.clientY, null);
    }
  }

  function sortBy(key: SortKey) {
    const cur = prefs.sort;
    prefs.setSort({ key, dir: cur.key === key && cur.dir === 'asc' ? 'desc' : 'asc' });
  }

  const COLS: SortKey[] = ['name', 'size', 'mtime', 'type'];
  const RESULT_COLS: (SortKey | 'place')[] = ['name', 'place', 'size', 'mtime'];
  function sortResultsBy(key: SortKey) {
    const cur = browser.resultSort;
    browser.sortResults(cur?.key === key ? (cur.dir === 'asc' ? { key, dir: 'desc' } : null) : { key, dir: 'asc' });
  }
  const listLabel = $derived(
    results ? m.sr_results_label({ q: browser.appliedQuery.trim() }) : m.fv_files_in({ name: browser.trail[browser.trail.length - 1]?.name ?? '' }),
  );
</script>

<div
  class="file-view-wrap"
  data-layout={layout}
  data-grid-size={gridSize}
  data-density={density}
  data-results={results ? '' : undefined}
  style:--tile-min="{TILE_MIN[gridSize]}px"
  style:--row-h="{row.h}px"
  style:--row-vis-w="{row.w}px"
  style:--row-vis-h="{row.vis}px"
>
  {#if browser.loaded && browser.entries.length === 0}
    <!-- the empty state speaks alone -->
  {:else if results}
    <div class="list-header" role="presentation">
      <span></span>
      {#each RESULT_COLS as key (key)}
        {#if key === 'place'}
          <span class="col static" data-col="place">{m.sr_col_place()}</span>
        {:else}
          {@const on = browser.resultSort?.key === key}
          <button
            type="button"
            class="col"
            data-col={key}
            class:active={on}
            aria-label={m.fv_sort_by({ key: sortLabel(key).toLowerCase() })}
            aria-pressed={on}
            onclick={() => sortResultsBy(key)}
          >
            {sortLabel(key)}
            {#if on}
              {#if browser.resultSort?.dir === 'asc'}<ArrowUp size={12} aria-hidden="true" />{:else}<ArrowDown size={12} aria-hidden="true" />{/if}
            {/if}
          </button>
        {/if}
        {#if key === 'name'}<span></span>{/if}
      {/each}
    </div>
  {:else if layout === 'list'}
    <div class="list-header" role="presentation">
      <span></span>
      {#each COLS as key (key)}
        <button
          type="button"
          class="col"
          data-col={key}
          class:active={prefs.sort.key === key}
          aria-label={m.fv_sort_by({ key: sortLabel(key).toLowerCase() })}
          aria-pressed={prefs.sort.key === key}
          onclick={() => sortBy(key)}
        >
          {sortLabel(key)}
          {#if prefs.sort.key === key}
            {#if prefs.sort.dir === 'asc'}<ArrowUp size={12} aria-hidden="true" />{:else}<ArrowDown size={12} aria-hidden="true" />{/if}
          {/if}
        </button>
        {#if key === 'name'}<span></span>{/if}
      {/each}
    </div>
  {/if}

  <div
    bind:this={view}
    class="file-view"
    data-layout={layout}
    role="listbox"
    tabindex="0"
    aria-label={listLabel}
    data-testid={results ? 'search-results' : undefined}
    aria-multiselectable="true"
    aria-activedescendant={browser.sel.focus !== null ? `item-${browser.sel.focus}` : undefined}
    aria-busy={browser.loading || browser.searching}
    onkeydown={keydown}
    onscroll={onScroll}
    bind:clientHeight={viewH}
    bind:clientWidth={viewW}
    data-ascii-host
    onclick={backgroundClick}
    oncontextmenu={backgroundMenu}
    use:decode={reveal}
    use:ctrlWheel
  >
    {#if !browser.loaded && browser.entries.length === 0}
      <div class="loading nv-enter" data-state="loading" data-layout={layout}>
        <span class="visually-hidden">{m.fv_reading()}</span>
        {#if layout === 'grid'}
          <span class="items tile-skeletons" data-layout="grid" aria-hidden="true">
            {#each { length: 10 } as _, i (i)}
              <span class="tile-skeleton">
                <span class="skeleton piece" use:skeleton={{ rows: 5, seed: 3 + i }}></span>
                <span class="skeleton label" use:skeleton={{ lines: 1, seed: 11 + i, widths: [`${58 + ((i * 17) % 34)}%`] }}></span>
              </span>
            {/each}
          </span>
        {:else}
          <span class="skeleton" aria-hidden="true" use:skeleton={{ lines: 7, seed: 3, widths: ['92%', '100%', '78%', '96%', '84%', '100%', '70%'] }}></span>
        {/if}
      </div>
    {:else if browser.loaded && browser.entries.length === 0 && results}
      <div class="empty" data-state={browser.searchError ? 'search-error' : 'no-results'} data-testid="search-empty">
        <span class="empty-envelope" aria-hidden="true"
          ><span class="ee-body">{#if browser.searchError}<TriangleAlert size={22} strokeWidth={1.4} />{:else}<SearchX size={22} strokeWidth={1.4} />{/if}</span></span
        >
        <p class="empty-title display">{browser.searchError ? m.sr_error() : m.sr_none()}</p>
        <p class="hint">
          {browser.searchError ?? (browser.scope === 'folder' ? m.sr_none_hint_scope() : m.sr_none_hint())}
        </p>
        {#if !browser.searchError && browser.filtering}
          <button type="button" class="btn sm" onclick={() => browser.setFilters({ kinds: [], size: null, date: null, tags: [], tagsAny: false })}>{m.flt_clear()}</button>
        {:else if !browser.searchError && browser.scope === 'folder'}
          <button type="button" class="btn sm" onclick={() => browser.setScope('volume')}>{m.sr_scope_volume()}</button>
        {/if}
      </div>
    {:else if browser.loaded && browser.entries.length === 0 && browser.filtering}
      <div class="empty" data-state="filter-empty" data-testid="filter-empty">
        <span class="empty-envelope" aria-hidden="true"><span class="ee-body"><ListFilter size={22} strokeWidth={1.4} /></span></span>
        <p class="empty-title display">{m.flt_empty()}</p>
        <p class="hint">{m.flt_empty_hint()}</p>
        <button type="button" class="btn sm" onclick={() => browser.setFilters({ kinds: [], size: null, date: null, tags: [], tagsAny: false })}>{m.flt_clear()}</button>
      </div>
    {:else if browser.loaded && browser.entries.length === 0}
      <div class="empty" data-state="empty-folder">
        <span class="empty-envelope" aria-hidden="true"><span class="ee-body"><FolderOpen size={22} strokeWidth={1.4} /></span></span>
        <p class="empty-title display">{browser.trail.length <= 1 ? m.fv_empty_volume() : m.fv_empty()}</p>
        <p class="hint">{isMobile ? m.fv_empty_hint_touch() : m.fv_empty_hint()}</p>
      </div>
    {:else}
      <div
        bind:this={itemsEl}
        class="items"
        data-layout={layout}
        data-virtual={virtual ? '' : undefined}
        style:padding-top={virtual ? `${fromRow * pitch}px` : undefined}
        style:height={virtual ? `${rowsTotal * pitch - (pitch - tileH)}px` : undefined}
        style:grid-auto-rows={virtual && layout === 'grid' ? `${tileH}px` : undefined}
        style:--vrow={virtual && layout === 'list' ? `${tileH}px` : undefined}
      >
        {#each shown as node (node.id)}
          <FileItem {node} {browser} {layout} hit={results ? browser.hitMeta.get(node.id) : undefined} />
        {/each}
      </div>
    {/if}
  </div>

  <footer class="status-bar" aria-live="polite">
    <span>{itemCount(browser.entries.length)}</span>
    {#if browser.selectedIds.length > 0}
      <span class="sel">
        <span class="dot" aria-hidden="true">·</span> {m.fv_selected({ n: String(browser.selectedIds.length) })}
        {#if browser.selectedNodes.some((n) => n.kind === 'file')}
          <span class="dot" aria-hidden="true">·</span> <span class="num">{formatBytes(browser.selectedNodes.reduce((s, n) => s + n.size, 0))}</span>
        {/if}
      </span>
    {/if}
  </footer>
</div>

<style>
  .file-view-wrap {
    --list-cols: var(--row-vis-w, 30px) minmax(0, 1fr) auto 96px 136px 120px;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .file-view-wrap[data-results] {
    --list-cols: var(--row-vis-w, 30px) minmax(0, 1fr) auto minmax(0, 30%) 96px 136px;
  }
  .col.static {
    display: inline-flex;
    cursor: default;
  }
  .col.static:hover,
  .col.static:active {
    background: none;
    color: var(--ink-3);
  }
  .empty .btn {
    margin-top: var(--space-3);
  }
  .file-view {
    flex: 1;
    min-height: 0;
    overflow-y: scroll;
    overflow-x: hidden;
    padding: var(--space-4) var(--space-3) var(--space-6) var(--space-5);
  }
  .file-view[data-layout='list'] {
    padding-top: var(--space-1);
  }
  .file-view:focus-visible {
    outline: none;
  }
  .items[data-layout='grid'] {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--tile-min, 164px), 1fr));
    gap: var(--space-3) var(--space-3);
    align-items: start;
  }
  .items[data-layout='list'] {
    display: flex;
    flex-direction: column;
  }
  .items[data-virtual][data-layout='list'] > :global(.file-item) {
    height: var(--vrow);
    flex: none;
  }
  .list-header {
    flex: none;
    display: grid;
    grid-template-columns: var(--list-cols);
    gap: var(--space-3);
    height: 32px;
    padding: 0 calc(var(--space-3) + var(--scrollbar) + var(--space-3)) 0 calc(var(--space-5) + var(--space-3));
    align-items: center;
    border-bottom: 1px solid var(--paper-rule-2);
  }
  .col {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 24px;
    margin: 0 -6px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    font-weight: 500;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-3);
  }
  .col:hover {
    background: color-mix(in oklab, var(--ink) 5%, transparent);
    color: var(--ink);
  }
  .col:active {
    background: color-mix(in oklab, var(--ink) 9%, transparent);
  }
  .col[data-col='mtime'] {
    margin-left: 12px;
  }
  .col[data-col='size'] {
    justify-content: flex-end;
  }
  .col.active {
    color: var(--accent);
  }

  .loading {
    padding: var(--space-4) var(--space-2) 0 0;
    animation: nv-enter 1ms linear 320ms backwards;
  }
  .skeleton {
    display: block;
    max-width: 760px;
    font-size: 13px;
    --ascii-skeleton: var(--ink-4);
  }
  .loading[data-layout='grid'] {
    padding: 0;
  }
  .tile-skeleton {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 8px 8px 10px;
  }
  .tile-skeleton .piece {
    width: 100%;
    aspect-ratio: 4 / 3;
    overflow: hidden;
    font-size: 11px;
    line-height: 1.1;
  }
  .tile-skeleton .label {
    width: 100%;
    font-size: 11px;
    text-align: center;
  }
  .empty {
    height: 100%;
    min-height: 240px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding-bottom: var(--space-7);
    text-align: center;
  }
  .empty-envelope {
    position: relative;
    width: 132px;
    height: 84px;
    margin-bottom: var(--space-4);
  }
  .ee-body {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--ink-4);
    background: linear-gradient(172deg, var(--env-paper-hi), var(--env-paper) 55%, var(--env-paper-lo));
    border-radius: 2px;
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.55),
      0 0 0 0.5px rgb(var(--cast) / 0.2),
      0 3px 6px -3px rgb(var(--cast) / 0.35),
      var(--env-glow);
  }
  .ee-body::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: 100%;
    height: 44px;
    clip-path: polygon(0 100%, 50% 0, 100% 100%);
    background:
      repeating-linear-gradient(60deg, color-mix(in srgb, var(--engrave-2) 50%, transparent) 0 0.6px, transparent 0.6px 3px),
      repeating-linear-gradient(-60deg, color-mix(in srgb, var(--engrave-2) 50%, transparent) 0 0.6px, transparent 0.6px 3px),
      var(--env-paper-lo);
  }
  .empty-title {
    font-size: var(--display-s);
    color: var(--ink);
  }

  .status-bar {
    flex: none;
    height: var(--statusbar-h);
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0 var(--space-5);
    border-top: 1px solid var(--paper-rule);
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
  .sel {
    color: var(--ink);
  }
  .dot {
    color: var(--ink-4);
  }
</style>
