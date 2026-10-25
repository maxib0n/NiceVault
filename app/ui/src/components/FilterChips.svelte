<!--
  The filter line under the search field: where to search (the whole volume / this folder, with
  a query), then Tipo · Dimensione · Data · Tag. Each chip is a pill that opens a small slip of
  choices; a chip in use shows its value and an x that clears it; "Azzera filtri" clears all.
  Without a query the chips narrow the folder on screen (fs_list filters).
  Hooks: [data-testid=filter-chip-<kind|size|date|tags>], [data-testid=filter-clear-<…>],
  [data-testid=filter-clear-all], [data-testid=filter-menu], [data-testid=search-scope].
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Archive, Calendar, ChevronDown, Code, File, FileText, Film, Folder, Image, Music, Ruler, Shapes, Tag as TagIcon, Tags, X } from '@lucide/svelte';
  import type { Component } from 'svelte';
  import { KIND_GROUPS } from '../lib/search';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import { DATE_PRESETS, SIZE_PRESETS, type DatePreset, type SizePreset, type UiFilters } from '../lib/stores/search.svelte';
  import { radioKeys } from '../lib/radio';
  import type { KindGroup } from '../lib/types';
  import Popover from './Popover.svelte';
  import TagChip from './TagChip.svelte';

  let { browser }: { browser: FileBrowser } = $props();

  type ChipId = 'kind' | 'size' | 'date' | 'tags';
  let open = $state<{ id: ChipId; x: number; y: number } | null>(null);
  const keys: Partial<Record<ChipId, HTMLButtonElement>> = $state({});

  const KIND_LABEL: Record<KindGroup, () => string> = {
    photos: m.flt_kind_photos,
    videos: m.flt_kind_videos,
    audio: m.flt_kind_audio,
    documents: m.flt_kind_documents,
    archives: m.flt_kind_archives,
    code: m.flt_kind_code,
    other: m.flt_kind_other,
    folders: m.flt_kind_folders,
  };
  const KIND_ICON: Record<KindGroup, Component<{ size?: number }>> = {
    photos: Image,
    videos: Film,
    audio: Music,
    documents: FileText,
    archives: Archive,
    code: Code,
    other: File,
    folders: Folder,
  };
  const SIZE_LABEL: Record<SizePreset, () => string> = { small: m.flt_size_small, medium: m.flt_size_medium, large: m.flt_size_large };
  const DATE_LABEL: Record<DatePreset, () => string> = {
    today: m.flt_date_today,
    week: m.flt_date_week,
    month: m.flt_date_month,
    year: m.flt_date_year,
  };

  const f = $derived(browser.filters);
  const tagList = $derived(browser.tags.list);

  const many = (names: string[]) => (names.length <= 1 ? (names[0] ?? '') : `${names[0]} ${m.flt_more({ n: String(names.length - 1) })}`);
  const value: Record<ChipId, () => string | null> = {
    kind: () => (f.kinds.length ? many(f.kinds.map((k) => KIND_LABEL[k]())) : null),
    size: () => (f.size ? SIZE_LABEL[f.size]() : null),
    date: () => (f.date ? DATE_LABEL[f.date]() : null),
    tags: () => (f.tags.length ? many(f.tags.map((t) => browser.tags.byId.get(t)?.name ?? '?')) : null),
  };
  const CHIPS: { id: ChipId; label: () => string; icon: Component<{ size?: number }> }[] = [
    { id: 'kind', label: m.flt_kind, icon: Shapes },
    { id: 'size', label: m.flt_size, icon: Ruler },
    { id: 'date', label: m.flt_date, icon: Calendar },
    { id: 'tags', label: m.flt_tags, icon: TagIcon },
  ];

  function set(patch: Partial<UiFilters>) {
    browser.setFilters({ ...browser.filters, ...patch });
  }
  function clear(id: ChipId) {
    if (id === 'kind') set({ kinds: [] });
    else if (id === 'size') set({ size: null });
    else if (id === 'date') set({ date: null });
    else set({ tags: [], tagsAny: false });
  }
  function toggle(id: ChipId) {
    if (open?.id === id) {
      open = null;
      return;
    }
    const r = keys[id]?.getBoundingClientRect();
    if (r) open = { id, x: r.left, y: r.bottom + 6 };
  }
  function close(restore: boolean) {
    const id = open?.id;
    open = null;
    if (restore && id) keys[id]?.focus();
  }
  function flip<T>(list: readonly T[], v: T): T[] {
    return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
  }

  /** ↑ ↓ Home End between the choices of a slip */
  function listKeys(e: KeyboardEvent) {
    const box = e.currentTarget as HTMLElement;
    const opts = [...box.querySelectorAll<HTMLElement>('.opt:not(:disabled)')];
    const i = opts.indexOf(document.activeElement as HTMLElement);
    const to = e.key === 'ArrowDown' ? i + 1 : e.key === 'ArrowUp' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? opts.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    opts[(to + opts.length) % opts.length]?.focus();
  }

  const MATCH = [false, true] as const;
</script>

<div class="filter-line" role="group" aria-label={m.flt_label()} data-testid="filter-line">
  {#if browser.searchOpen}
    <div
      class="scope"
      role="radiogroup"
      aria-label={m.sr_scope()}
      tabindex="-1"
      data-testid="search-scope"
      onkeydown={(e) => radioKeys(e, ['volume', 'folder'] as const, browser.scope, (s) => browser.setScope(s))}
    >
      {#each [['volume', m.sr_scope_volume()], ['folder', m.sr_scope_folder()]] as const as [id, text] (id)}
        <button
          type="button"
          role="radio"
          class="seg"
          aria-checked={browser.scope === id}
          tabindex={browser.scope === id ? 0 : -1}
          data-scope={id}
          onclick={() => browser.setScope(id)}>{text}</button
        >
      {/each}
    </div>
    <span class="rule" aria-hidden="true"></span>
  {/if}

  {#each CHIPS as chip (chip.id)}
    {@const v = value[chip.id]()}
    <span class="chip" data-active={v !== null ? '' : undefined}>
      <button
        bind:this={keys[chip.id]}
        type="button"
        class="chip-key"
        aria-haspopup="dialog"
        aria-expanded={open?.id === chip.id}
        data-testid="filter-chip-{chip.id}"
        onclick={() => toggle(chip.id)}
      >
        <chip.icon size={13} />
        <span class="chip-name">{chip.label()}</span>
        {#if v !== null}<span class="chip-value">{v}</span>{/if}
        {#if v === null}<ChevronDown size={12} class="caret" aria-hidden="true" />{/if}
      </button>
      {#if v !== null}
        <button type="button" class="chip-x" aria-label={m.flt_clear_one({ name: chip.label() })} data-testid="filter-clear-{chip.id}" onclick={() => clear(chip.id)}>
          <X size={12} />
        </button>
      {/if}
    </span>
  {/each}

  {#if browser.filtering}
    <button type="button" class="btn ghost sm clear-all" data-testid="filter-clear-all" onclick={() => browser.setFilters({ kinds: [], size: null, date: null, tags: [], tagsAny: false })}>
      {m.flt_clear()}
    </button>
  {/if}

  <span class="spacer"></span>
  <button
    type="button"
    class="btn ghost sm manage"
    data-testid="tags-manage"
    aria-haspopup="dialog"
    onclick={(e) => {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      browser.tagManager = { x: r.right - 340, y: r.bottom + 6 };
    }}
  >
    <Tags size={14} aria-hidden="true" /> <span class="label-text">{m.tag_manage()}</span>
  </button>
</div>

{#if open}
  {@const id = open?.id ?? 'kind'}
  <Popover x={open?.x ?? 0} y={open?.y ?? 0} label={CHIPS.find((c) => c.id === id)?.label() ?? ''} onclose={close} ignore={keys[id]} width={id === 'tags' ? 250 : 220} testid="filter-menu">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="opts" onkeydown={listKeys}>
      {#if id === 'kind'}
        {#each KIND_GROUPS as k (k)}
          {@const Icon = KIND_ICON[k]}
          <button type="button" class="opt" role="checkbox" aria-checked={f.kinds.includes(k)} data-kind={k} onclick={() => set({ kinds: flip(f.kinds, k) })}>
            <span class="box" aria-hidden="true"></span>
            <span class="glyph" aria-hidden="true"><Icon size={14} /></span>
            <span class="lbl">{KIND_LABEL[k]()}</span>
          </button>
        {/each}
      {:else if id === 'size' || id === 'date'}
        {@const list = id === 'size' ? SIZE_PRESETS : DATE_PRESETS}
        {@const cur = id === 'size' ? f.size : f.date}
        <button type="button" class="opt" role="radio" aria-checked={cur === null} onclick={() => (set(id === 'size' ? { size: null } : { date: null }), close(true))}>
          <span class="box" aria-hidden="true"></span>
          <span class="lbl">{m.flt_any_value()}</span>
        </button>
        {#each list as p (p)}
          <button
            type="button"
            class="opt"
            role="radio"
            aria-checked={cur === p}
            data-preset={p}
            onclick={() => (set(id === 'size' ? { size: p as SizePreset } : { date: p as DatePreset }), close(true))}
          >
            <span class="box" aria-hidden="true"></span>
            <span class="lbl" class:num={id === 'size'}>{id === 'size' ? SIZE_LABEL[p as SizePreset]() : DATE_LABEL[p as DatePreset]()}</span>
          </button>
        {/each}
      {:else}
        {#each tagList as t (t.id)}
          <button type="button" class="opt" role="checkbox" aria-checked={f.tags.includes(t.id)} data-tag={t.id} onclick={() => set({ tags: flip(f.tags, t.id) })}>
            <span class="box" aria-hidden="true"></span>
            <span class="lbl"><TagChip tag={t} /></span>
            <span class="count">{t.count}</span>
          </button>
        {:else}
          <p class="pop-note">{m.flt_no_tags()}</p>
        {/each}
      {/if}
    </div>
    {#if id === 'tags' && tagList.length > 1}
      <div class="pop-sep" role="separator"></div>
      <div class="match">
        <div
          class="scope small"
          role="radiogroup"
          aria-label={m.flt_match()}
          tabindex="-1"
          onkeydown={(e) => radioKeys(e, MATCH, f.tagsAny, (v) => set({ tagsAny: v }))}
        >
          {#each MATCH as any (any)}
            <button type="button" role="radio" class="seg" aria-checked={f.tagsAny === any} tabindex={f.tagsAny === any ? 0 : -1} data-any={any} onclick={() => set({ tagsAny: any })}
              >{any ? m.flt_any() : m.flt_all()}</button
            >
          {/each}
        </div>
      </div>
    {/if}
  </Popover>
{/if}

<style>
  .filter-line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    min-height: 38px;
    padding: 2px 0 8px var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .rule {
    width: 1px;
    height: 18px;
    margin: 0 4px;
    background: var(--paper-rule-2);
  }

  /* where to search: two latching keys in one pressed-in bed (the toolbar's view switch) */
  .scope {
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
    height: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    font-size: var(--text-sm);
    font-weight: 500;
    white-space: nowrap;
    color: var(--ink-3);
  }
  .seg:hover {
    color: var(--ink);
  }
  .seg:active {
    background: color-mix(in oklab, var(--ink) 7%, transparent);
  }
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
  .scope.small .seg {
    height: 22px;
    padding: 0 8px;
    font-size: var(--text-xs);
  }

  /* a chip: a pill engraved round its edge; in use, it is inked in the volume's colour */
  .chip {
    display: inline-flex;
    align-items: stretch;
    flex: none;
    max-width: 260px;
    height: 26px;
    border-radius: var(--radius-pill);
    box-shadow: inset 0 0 0 1px var(--paper-rule-2);
    color: var(--ink-2);
  }
  .chip[data-active] {
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
    color: var(--ink);
  }
  .chip-key {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    padding: 0 10px 0 10px;
    border: 0;
    border-radius: var(--radius-pill);
    background: none;
    color: inherit;
    font-size: var(--text-sm);
    font-weight: 500;
    white-space: nowrap;
  }
  .chip[data-active] .chip-key {
    padding-right: 4px;
  }
  .chip-key > :global(svg:first-child) {
    flex: none;
    color: var(--ink-3);
  }
  .chip[data-active] .chip-key > :global(svg:first-child) {
    color: var(--accent);
  }
  .chip-key :global(.caret) {
    margin-left: -1px;
    color: var(--ink-4);
  }
  .chip-key:hover {
    background: color-mix(in oklab, var(--ink) 5%, transparent);
    color: var(--ink);
  }
  .chip-key:active {
    background: color-mix(in oklab, var(--ink) 9%, transparent);
  }
  .chip-key[aria-expanded='true'] {
    background: color-mix(in oklab, var(--ink) 7%, transparent);
    color: var(--ink);
  }
  .chip-key:focus-visible,
  .chip-x:focus-visible {
    outline-offset: -2px;
  }
  .chip-value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
    color: var(--accent);
  }
  .chip-name {
    flex: none;
  }
  .chip[data-active] .chip-name::after {
    content: ':';
  }
  .chip-x {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    margin-left: 1px;
    border: 0;
    border-radius: var(--radius-pill);
    background: none;
    color: var(--ink-3);
  }
  .chip-x:hover {
    background: color-mix(in oklab, var(--accent) 16%, transparent);
    color: var(--ink);
  }
  .chip-x:active {
    background: color-mix(in oklab, var(--accent) 24%, transparent);
  }
  .clear-all {
    margin-left: 2px;
  }
  .manage {
    flex: none;
  }

  .opts {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .num {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }
  .match {
    padding: 2px 4px 4px;
  }
  .match .scope {
    width: 100%;
  }
  .match .seg {
    flex: 1;
  }

  @media (max-width: 1080px) {
    .manage .label-text {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }
  }
</style>
