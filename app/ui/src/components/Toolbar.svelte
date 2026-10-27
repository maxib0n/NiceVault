<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { ArrowDownUp, ArrowUp, ChevronDown, ListFilter, Search, Download, FileUp, FolderPlus, Images, LayoutGrid, Library, List, Rows2, Rows3, Rows4, Tag as TagIcon, Upload } from '@lucide/svelte';
  import { isMobile } from '../lib/platform';
  import { flushSync } from 'svelte';
  import { app } from '../lib/stores/app.svelte';
  import { sortLabel, type SortKey } from '../lib/sort';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import { prefs } from '../lib/stores/prefs.svelte';
  import Breadcrumb from './Breadcrumb.svelte';
  import FilterChips from './FilterChips.svelte';
  import SearchBar from './SearchBar.svelte';
  import Menu, { type MenuItem } from './Menu.svelte';
  import Band from '../ornament/Band.svelte';
  import PhoneViewSheet from './PhoneViewSheet.svelte';
  import { formatBytes, formatNumber, itemCount } from '../lib/format';
  import { radioKeys } from '../lib/radio';
  import { DENSITIES, GRID_SIZES, viewSize } from '../lib/stores/viewsize.svelte';
  import type { GridSize, ListDensity } from '../lib/types';

  let { browser }: { browser: FileBrowser } = $props();

  let sortBtn: HTMLButtonElement | undefined = $state();
  let sortMenu = $state<{ x: number; y: number } | null>(null);
  let importKey: HTMLButtonElement | undefined = $state();
  let importMenu = $state<{ x: number; y: number } | null>(null);
  const importItems: MenuItem[] = [
    { id: 'photos', label: m.import_photos(), icon: Images, action: () => browser.importPhotos() },
    { id: 'files', label: m.import_files(), icon: FileUp, action: () => browser.importViaDialog() },
  ];
  function importNow() {
    if (!isMobile) return void browser.importViaDialog();
    const r = importKey?.getBoundingClientRect();
    importMenu = { x: r?.left ?? 0, y: r?.top ?? 0 };
  }

  const resultItems = $derived.by<MenuItem[]>(() => {
    const rs = browser.resultSort;
    const dir = rs?.dir ?? 'asc';
    return [
      { id: 'relevance', label: m.sr_relevance(), checked: rs === null, action: () => browser.sortResults(null) },
      ...(['name', 'size', 'mtime', 'type'] as SortKey[]).map((key) => ({
        id: key,
        label: sortLabel(key),
        checked: rs?.key === key,
        action: () => browser.sortResults({ key, dir }),
      })),
      { id: 'asc', label: m.tb_asc(), checked: rs !== null && dir === 'asc', disabled: rs === null, separatorBefore: true, action: () => rs && browser.sortResults({ ...rs, dir: 'asc' }) },
      { id: 'desc', label: m.tb_desc(), checked: rs !== null && dir === 'desc', disabled: rs === null, action: () => rs && browser.sortResults({ ...rs, dir: 'desc' }) },
    ];
  });

  const sortItems = $derived<MenuItem[]>([
    ...(['name', 'size', 'mtime', 'type'] as SortKey[]).map((key) => ({
      id: key,
      label: sortLabel(key),
      checked: prefs.sort.key === key,
      action: () => prefs.setSort({ ...prefs.sort, key }),
    })),
    { id: 'asc', label: m.tb_asc(), checked: prefs.sort.dir === 'asc', separatorBefore: true, action: () => prefs.setSort({ ...prefs.sort, dir: 'asc' }) },
    { id: 'desc', label: m.tb_desc(), checked: prefs.sort.dir === 'desc', action: () => prefs.setSort({ ...prefs.sort, dir: 'desc' }) },
  ]);

  const folderName = $derived(browser.trail[browser.trail.length - 1]?.name ?? '');
  const results = $derived(browser.resultsMode);
  const layout = $derived(results ? 'list' : prefs.view);
  const resultsTitle = $derived(m.sr_results_title({ count: browser.hitTotal, n: formatNumber(browser.hitTotal), q: browser.appliedQuery.trim() }));
  let tagKey: HTMLButtonElement | undefined = $state();
  const filesBytes = $derived(browser.entries.reduce((n, e) => n + (e.kind === 'file' ? e.size : 0), 0));

  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
  const MOD = isMac ? '⌘' : 'Ctrl';
  const SIZE_LABEL: Record<GridSize, () => string> = { s: m.size_s, m: m.size_m, l: m.size_l, xl: m.size_xl };
  const DENSITY_LABEL: Record<ListDensity, () => string> = { compact: m.density_compact, normal: m.density_normal, comfortable: m.density_comfortable };
  const DENSITY_ICON = { compact: Rows4, normal: Rows3, comfortable: Rows2 } as const;
  const grid = $derived(viewSize.grid(browser.vault.id));
  const density = $derived(viewSize.density(browser.vault.id));

  const sizeItems = $derived<MenuItem[]>(
    !isMobile
      ? []
      : layout === 'grid'
        ? GRID_SIZES.map((g, i) => ({
            id: `size-${g}`,
            label: m.tb_size_icons_item({ label: SIZE_LABEL[g]() }),
            checked: grid === g,
            separatorBefore: i === 0,
            action: () => viewSize.setGrid(browser.vault.id, g),
          }))
        : DENSITIES.map((d, i) => ({
            id: `density-${d}`,
            label: m.tb_size_rows_item({ label: DENSITY_LABEL[d]() }),
            checked: density === d,
            separatorBefore: i === 0,
            action: () => viewSize.setDensity(browser.vault.id, d),
          })),
  );

  let phoneFilters = $state(false);
  let viewSheet = $state(false);
  const showSearch = $derived(!isMobile || browser.searchOpen);
  const bare = $derived(browser.loaded && browser.entries.length === 0 && !browser.filtering && !browser.searchOpen);
  const showFilters = $derived(!bare && (!isMobile || phoneFilters || browser.filtering || browser.searchOpen));

  const sortName = $derived(results ? (browser.resultSort ? sortLabel(browser.resultSort.key) : m.sr_relevance()) : sortLabel(prefs.sort.key));

  function openSort() {
    const r = sortBtn?.getBoundingClientRect();
    if (r) sortMenu = { x: r.right - 200, y: r.bottom + 6 };
  }
</script>

{#if isMobile}<PhoneViewSheet open={viewSheet} vault={browser.vault.id} {results} onclose={() => (viewSheet = false)} />{/if}

<header class="folder-head">
  <div class="toolbar" role="toolbar" aria-label={m.fb_menu_folder()}>
    <div class="line nav-line" data-phone-search={isMobile && browser.searchOpen ? '' : undefined}>
      {#if isMobile && browser.trail.length <= 1}
        <button type="button" class="btn ghost icon shelf" aria-label={m.mob_shelf()} title={m.mob_shelf()} data-testid="mobile-shelf" onclick={() => app.goto('welcome')}>
          <Library size={18} />
        </button>
      {/if}
      <button
        type="button"
        class="btn ghost icon up"
        aria-label={m.tb_up()}
        title={m.tb_up_title()}
        disabled={browser.trail.length <= 1}
        onclick={() => browser.up()}
      >
        <ArrowUp size={16} />
      </button>

      <Breadcrumb {browser} />

      <span class="spacer"></span>

      {#if showSearch}
        <SearchBar {browser} />
      {:else}
        <button type="button" class="btn ghost icon phone-key" aria-label={m.tb_search_key()} data-testid="phone-search" style="margin-left: auto" onclick={() => flushSync(() => browser.openSearch(true))}>
          <Search size={17} />
        </button>
        <button
          type="button"
          class="btn ghost icon phone-key"
          aria-label={m.flt_label()}
          aria-expanded={showFilters}
          data-testid="phone-filters"
          data-active={browser.filtering ? '' : undefined}
          onclick={() => (phoneFilters = !phoneFilters)}
        >
          <ListFilter size={17} />
        </button>
      {/if}

      {#if layout === 'grid'}
        <div
          class="view-toggle sizes"
          role="radiogroup"
          aria-label={m.tb_size_grid()}
          tabindex="-1"
          data-testid="view-size"
          onkeydown={(e) => radioKeys(e, GRID_SIZES, grid, (g) => viewSize.setGrid(browser.vault.id, g))}
        >
          {#each GRID_SIZES as g (g)}
            <button
              type="button"
              role="radio"
              class="seg size"
              aria-checked={grid === g}
              aria-label={SIZE_LABEL[g]()}
              title={m.tb_size_keys({ label: SIZE_LABEL[g](), mod: MOD })}
              tabindex={grid === g ? 0 : -1}
              onclick={() => viewSize.setGrid(browser.vault.id, g)}
            >
              <span class="size-glyph" data-size={g} aria-hidden="true"></span>
            </button>
          {/each}
        </div>
      {:else}
        <div
          class="view-toggle sizes"
          role="radiogroup"
          aria-label={m.tb_density()}
          tabindex="-1"
          data-testid="view-size"
          onkeydown={(e) => radioKeys(e, DENSITIES, density, (d) => viewSize.setDensity(browser.vault.id, d))}
        >
          {#each DENSITIES as d (d)}
            {@const Icon = DENSITY_ICON[d]}
            <button
              type="button"
              role="radio"
              class="seg"
              aria-checked={density === d}
              aria-label={DENSITY_LABEL[d]()}
              title={m.tb_size_keys({ label: DENSITY_LABEL[d](), mod: MOD })}
              tabindex={density === d ? 0 : -1}
              onclick={() => viewSize.setDensity(browser.vault.id, d)}
            >
              <Icon size={15} />
            </button>
          {/each}
        </div>
      {/if}

      {#if isMobile}
        <button type="button" class="btn ghost icon phone-key" aria-label={m.tb_view()} title={m.tb_view()} aria-haspopup="dialog" aria-expanded={viewSheet} data-testid="phone-view" onclick={() => (viewSheet = true)}>
          {#if layout === 'grid'}<LayoutGrid size={17} />{:else}<List size={17} />{/if}
        </button>
      {:else}
        <div class="view-toggle" role="group" aria-label={m.tb_view()}>
          <button
            type="button"
            class="seg"
            title={m.tb_grid()}
            aria-label={m.tb_grid()}
            aria-pressed={layout === 'grid'}
            disabled={results}
            onclick={() => prefs.setView('grid')}
          >
            <LayoutGrid size={15} />
          </button>
          <button
            type="button"
            class="seg"
            title={m.tb_list()}
            aria-label={m.tb_list()}
            aria-pressed={layout === 'list'}
            disabled={results}
            onclick={() => prefs.setView('list')}
          >
            <List size={15} />
          </button>
        </div>
      {/if}

      <button
        bind:this={sortBtn}
        type="button"
        class="btn ghost sm sort"
        aria-label={m.tb_sort_named({ key: sortName })}
        aria-haspopup="menu"
        aria-expanded={sortMenu !== null}
        onclick={openSort}
      >
        <ArrowDownUp size={15} aria-hidden="true" />
        <span class="label-text" aria-hidden="true">{sortName}</span>
        <ChevronDown size={13} aria-hidden="true" class="caret" />
      </button>
    </div>

    {#if showFilters}<FilterChips {browser} />{/if}

    <div class="line title-line">
      <div class="title-block">
        {#if results}
          <h2 class="folder-title display results-title" title={resultsTitle} data-testid="search-title">{resultsTitle}</h2>
          <p class="folder-meta" aria-live="polite">
            <span>{browser.scope === 'folder' ? m.sr_in_folder({ name: folderName }) : m.sr_in_volume()}</span>
            {#if browser.hitTotal > browser.entries.length}<span class="sep" aria-hidden="true">·</span><span>{m.sr_first_n({ n: formatNumber(browser.entries.length) })}</span>{/if}
            {#if browser.indexing}<span class="sep" aria-hidden="true">·</span><span class="quiet num" data-testid="search-indexing">{m.sr_indexing({ done: formatNumber(browser.indexing.done), total: formatNumber(browser.indexing.total) })}</span>{/if}
          </p>
        {:else}
        <h2 class="folder-title display" title={folderName}>{folderName}</h2>
        {/if}
        {#if browser.loaded && !results}
          <p class="folder-meta">
            <span>{itemCount(browser.entries.length)}</span>{#if filesBytes > 0}<span class="sep" aria-hidden="true">·</span><span class="num">{formatBytes(filesBytes)}</span>{/if}
          </p>
        {/if}
      </div>
      <div class="actions">
        {#if !results}
          <button type="button" class="btn" onclick={() => browser.newFolder()} title={m.tb_new_folder_title()}>
            <FolderPlus size={16} aria-hidden="true" /> <span class="label-text">{m.fb_new_folder()}</span>
          </button>
        {/if}
        <button
          type="button"
          class="btn"
          disabled={browser.selectedIds.length === 0}
          onclick={() => browser.exportNodes()}
          title={m.tb_export_title()}
        >
          <Download size={16} aria-hidden="true" /> <span class="label-text">{m.tb_export()}</span>
        </button>
        <button
          bind:this={tagKey}
          type="button"
          class="btn"
          disabled={browser.selectedIds.length === 0}
          aria-haspopup="dialog"
          aria-expanded={browser.tagger !== null}
          data-testid="tag-selection"
          onclick={() => {
            const r = tagKey?.getBoundingClientRect();
            if (r) browser.openTagger({ x: r.left, y: r.bottom + 6 });
          }}
          title={m.tag_button_title()}
        >
          <TagIcon size={16} aria-hidden="true" /> <span class="label-text">{m.tag_button()}</span>
        </button>
        {#if !results}
          <button bind:this={importKey} type="button" class="btn primary" onclick={importNow} title={m.tb_import_title()} aria-haspopup={isMobile ? 'menu' : undefined} aria-expanded={isMobile ? importMenu !== null : undefined}>
            <Upload size={16} aria-hidden="true" /> <span class="label-text">{m.tb_import()}</span>
          </button>
        {/if}
      </div>
    </div>
  </div>
  <Band />
</header>

{#if importMenu}
  <Menu
    items={importItems}
    x={importMenu.x}
    y={importMenu.y}
    label={m.import_from()}
    title={m.import_from()}
    onclose={(restore) => {
      importMenu = null;
      if (restore) importKey?.focus();
    }}
  />
{/if}

{#if sortMenu}
  <Menu
    items={[...(results ? resultItems : sortItems), ...sizeItems]}
    x={sortMenu.x}
    y={sortMenu.y}
    label={m.tb_sort_by()}
    onclose={(restore) => {
      sortMenu = null;
      if (restore) sortBtn?.focus();
    }}
  />
{/if}

<style>
  .folder-head {
    container-type: inline-size;
    --bleed-start: var(--space-4);
    --bleed-end: var(--space-5);
    flex: none;
    padding: 0 var(--space-5) 0 var(--space-4);
  }
  .toolbar {
    display: flex;
    flex-direction: column;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .phone-key {
    position: relative;
    flex: none;
    width: var(--tap, 44px);
    min-height: var(--tap, 44px);
  }
  .phone-key[data-active]::before {
    content: '';
    position: absolute;
    top: 9px;
    right: 9px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .nav-line[data-phone-search] > :not(:global(.search-box)) {
    display: none;
  }
  .nav-line {
    height: var(--toolbar-h);
  }
  .spacer {
    flex: 1;
    min-width: var(--space-2);
  }
  .up {
    flex: none;
  }

  .view-toggle {
    display: flex;
    flex: none;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .seg {
    display: grid;
    place-items: center;
    width: 30px;
    height: 26px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--ink-3);
  }
  .seg:hover {
    color: var(--ink);
  }
  .seg:active {
    background: color-mix(in oklab, var(--ink) 7%, transparent);
  }
  .seg[aria-pressed='true'],
  .seg[aria-checked='true'] {
    color: var(--ink);
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--key-edge),
      var(--lift-key);
  }
  .seg:focus-visible {
    outline-offset: 0;
  }
  .sizes {
    margin-right: var(--space-1);
  }
  .size-glyph {
    display: block;
    align-self: end;
    margin-bottom: 6px;
    border: 1.5px solid currentColor;
    border-radius: 1.5px;
    background: linear-gradient(currentColor, currentColor) top / 100% 55% no-repeat;
    background-clip: content-box;
    padding: 1px;
    opacity: 0.95;
  }
  .seg.size {
    place-items: end center;
  }
  .size-glyph[data-size='s'] {
    width: 7px;
    height: 7px;
    padding: 0.5px;
    border-radius: 1px;
  }
  .size-glyph[data-size='m'] {
    width: 9px;
    height: 9px;
  }
  .size-glyph[data-size='l'] {
    width: 11px;
    height: 11px;
    border-radius: 2px;
  }
  .size-glyph[data-size='xl'] {
    width: 14px;
    height: 14px;
    border-radius: 2.5px;
    padding: 1.5px;
  }
  .seg[aria-checked='false'] .size-glyph {
    opacity: 0.8;
  }
  .sort {
    flex: none;
    margin-left: var(--space-1);
  }
  .sort :global(.caret) {
    margin-left: -2px;
    color: var(--ink-3);
  }

  .title-line {
    align-items: flex-end;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
    padding: 2px 0 14px var(--space-2);
  }
  .title-block {
    flex: 1 1 300px;
    min-width: 0;
  }
  .folder-title {
    font-size: clamp(26px, 2.6vw, 36px);
    font-weight: 500;
    line-height: 1.04;
    color: var(--title-ink);
    overflow-wrap: break-word;
    text-wrap: balance;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    padding-block: 0.12em 0.16em;
    margin-block: -0.12em -0.16em;
  }
  .folder-meta {
    margin-top: 6px;
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
  .results-title {
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
  .folder-meta .quiet {
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .folder-meta .sep {
    margin: 0 6px;
    color: var(--ink-4);
  }
  .actions {
    display: flex;
    flex: none;
    margin-left: auto;
    gap: var(--space-2);
    padding-bottom: 2px;
  }

  @container (max-width: 560px) {
    .sizes {
      display: none;
    }
  }
  @container (max-width: 620px) {
    .sort .label-text {
      position: absolute;
      width: 1px;
      height: 1px;
      margin: -1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }
  }

  @media (max-width: 1080px) {
    .actions .label-text {
      position: absolute;
      width: 1px;
      height: 1px;
      margin: -1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }
    .actions .btn {
      width: var(--key-h);
      padding: 0;
    }
  }
</style>
