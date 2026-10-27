<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Check } from '@lucide/svelte';
  import { DRAG_MIME } from '../lib/dnd';
  import { setDragPreview } from '../lib/dragPreview';
  import { fileType, typeLabel } from '../lib/filetype';
  import { formatBytes, formatDate, itemCount, nsToDate } from '../lib/format';
  import { highlightParts } from '../lib/search';
  import type { FileBrowser, HitMeta } from '../lib/stores/browser.svelte';
  import type { FsNode, Tag } from '../lib/types';
  import FolderPeek from './FolderPeek.svelte';
  import RenameInput from './RenameInput.svelte';
  import TagChip from './TagChip.svelte';
  import Thumbnail from './Thumbnail.svelte';
  import { ROW, thumbPx, viewSize } from '../lib/stores/viewsize.svelte';
  import { isMobile } from '../lib/platform';

  let { node, browser, layout, hit }: { node: FsNode; browser: FileBrowser; layout: 'grid' | 'list'; hit?: HitMeta } = $props();

  const selected = $derived(browser.sel.selected.has(node.id));
  const focused = $derived(browser.sel.focus === node.id);
  const renaming = $derived(browser.renaming === node.id && !isMobile);
  const type = $derived(fileType(node));
  const px = $derived(layout === 'grid' ? thumbPx(viewSize.grid(browser.vault.id)) : Math.ceil(ROW[viewSize.density(browser.vault.id)].w * 2 * (window.devicePixelRatio || 1)));
  let dropTarget = $state(false);
  let count = $state<number | null>(null);
  const dirMeta = $derived(count === null ? m.fi_folder() : count === 0 ? m.fi_empty() : itemCount(count));

  const tags = $derived((node.tags ?? []).map((id) => browser.tags.byId.get(id)).filter((t): t is Tag => !!t));
  const dots = $derived(layout === 'grid' && viewSize.grid(browser.vault.id) === 's');
  const PILLS = 2;
  const pills = $derived(dots ? tags.slice(0, 4) : tags.slice(0, PILLS));
  const more = $derived(tags.length - pills.length);
  const tagsLabel = $derived(tags.length ? m.tag_chips_label({ names: tags.map((t) => t.name).join(', ') }) : undefined);
  const parts = $derived(hit?.highlights.length ? highlightParts(node.name, hit.highlights) : null);
  const place = $derived(hit ? hit.path.split('/').filter(Boolean) : []);
  const placeText = $derived(place.length ? place.join(' › ') : browser.vault.name);

  function click(e: MouseEvent) {
    browser.click(node.id, { shift: e.shiftKey, toggle: e.ctrlKey || e.metaKey }, true);
    if (browser.renaming === null) (e.currentTarget as HTMLElement).closest<HTMLElement>('[role=listbox]')?.focus({ preventScroll: true });
  }

  function contextmenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    browser.openMenu(e.clientX, e.clientY, node.id);
  }

  function dragstart(e: DragEvent) {
    if (!e.dataTransfer) return;
    if (!browser.sel.selected.has(node.id)) browser.click(node.id, {});
    e.dataTransfer.setData(DRAG_MIME, JSON.stringify(browser.selectedIds));
    e.dataTransfer.effectAllowed = 'move';
    setDragPreview(e, browser.selectedIds, node.id, node.name);
  }

  function accepts(e: DragEvent): boolean {
    if (node.kind !== 'dir' || !e.dataTransfer) return false;
    const types = [...e.dataTransfer.types];
    if (types.includes(DRAG_MIME)) return !browser.sel.selected.has(node.id);
    return types.includes('Files');
  }

  function dragover(e: DragEvent) {
    if (!accepts(e)) return;
    e.preventDefault();
    e.stopPropagation();
    dropTarget = true;
  }

  function drop(e: DragEvent) {
    dropTarget = false;
    if (!accepts(e) || !e.dataTransfer) return;
    e.preventDefault();
    e.stopPropagation();
    browser.dragOver = false;
    const raw = e.dataTransfer.getData(DRAG_MIME);
    if (raw) void browser.moveInto(JSON.parse(raw), node.id);
    else if (e.dataTransfer.files.length) void browser.importSources([...e.dataTransfer.files], node.id);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  id="item-{node.id}"
  class="file-item"
  data-layout={layout}
  data-kind={node.kind}
  data-type={type}
  data-testid={hit ? 'search-hit' : undefined}
  class:selected
  class:focused
  class:drop-target={dropTarget}
  role="option"
  aria-selected={selected}
  tabindex="-1"
  draggable={!renaming && !isMobile}
  onclick={click}
  ondblclick={() => browser.activate(node.id)}
  oncontextmenu={contextmenu}
  ondragstart={dragstart}
  ondragover={dragover}
  ondragleave={() => (dropTarget = false)}
  ondrop={drop}
>
  {#if isMobile && browser.selecting}
    <span class="pick" aria-hidden="true">{#if selected}<Check size={14} strokeWidth={2.6} />{/if}</span>
  {/if}
  <span class="visual" data-ascii="media">
    {#if node.kind === 'dir'}
      <FolderPeek vault={browser.vault.id} {node} {layout} refresh={browser.entries} bind:count />
    {:else}
      <Thumbnail vault={browser.vault.id} {node} size={layout === 'grid' ? 38 : 17} {px} />
    {/if}
  </span>
  <span class="name" title={node.name} data-testid="item-name">
    {#if renaming}
      <RenameInput name={node.name} oncommit={(v) => browser.commitRename(node.id, v)} oncancel={() => browser.cancelRename()} />
    {:else if parts}
      {#each parts as p, i (i)}{#if p.hit}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}
    {:else}
      {node.name}
    {/if}
  </span>
  {#snippet chips()}
    {#each pills as t (t.id)}<TagChip tag={t} dot={dots} />{/each}{#if more > 0}<span class="more" title={tagsLabel}>+{more}</span>{/if}
  {/snippet}
  {#if layout === 'list'}
    <span class="chips" role={tags.length ? 'img' : undefined} aria-label={tagsLabel}>{@render chips()}</span>
    {#if hit}
      <button
        type="button"
        class="place"
        title={m.sr_open_place({ path: placeText })}
        data-testid="hit-path"
        onclick={(e) => {
          e.stopPropagation();
          void browser.openHitPlace(node.id);
        }}
        ondblclick={(e) => e.stopPropagation()}
      >
        {#if place.length > 1}<span class="place-up">{place.slice(0, -1).join(' › ')} ›</span>{/if}<span class="place-dir">{place.length ? place[place.length - 1] : browser.vault.name}</span>
      </button>
    {/if}
    <span class="size">{node.kind === 'dir' ? (count === null ? '—' : dirMeta) : formatBytes(node.size)}</span>
    <span class="mtime">{formatDate(nsToDate(node.mtime))}</span>
    {#if !hit}<span class="type">{typeLabel(type)}</span>{/if}
  {:else}
    <span class="meta">
      {#if tags.length}<span class="chips" class:dots role="img" aria-label={tagsLabel}>{@render chips()}</span>{/if}
      <span class="meta-text">{#if node.kind === 'dir'}{dirMeta}{:else}<span class="num">{formatBytes(node.size)}</span>{/if}</span>
    </span>
  {/if}
</div>

<style>
  .file-item {
    position: relative;
    color: var(--ink);
    cursor: default;
    outline: none;
  }

  .file-item[data-layout='grid'] {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 8px 8px 10px;
    border-radius: var(--radius-md);
    text-align: center;
  }
  .file-item[data-layout='grid'] .visual {
    position: relative;
    display: block;
    width: 100%;
    aspect-ratio: 4 / 3;
    margin-bottom: 4px;
  }
  .file-item[data-layout='grid'] .visual > :global(.thumb) {
    width: 100%;
    height: 100%;
  }
  .file-item[data-layout='grid'] .visual :global(.paper-file) {
    height: 86%;
  }
  .file-item[data-layout='grid']:hover .visual :global(.print) {
    box-shadow: var(--lift-print-hover), var(--print-glow);
  }
  .file-item[data-layout='grid'] .name {
    width: 100%;
    font-size: var(--text-sm);
    font-weight: 500;
    line-height: 1.32;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .meta {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    width: 100%;
    min-width: 0;
    height: 16px;
    margin-top: -2px;
    font-size: var(--text-xs);
    line-height: 16px;
    color: var(--ink-3);
  }
  .meta-text {
    flex: none;
    white-space: nowrap;
  }
  .chips {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
    overflow: hidden;
  }
  .meta .chips {
    flex: 0 1 auto;
  }
  .chips.dots {
    gap: 2px;
    padding: 0 1px;
  }
  .more {
    flex: none;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--ink-3);
  }
  .file-item[data-layout='list'] .chips {
    max-width: 230px;
  }
  .file-item[data-layout='list'] .chips :global(.tag-chip) {
    max-width: 104px;
  }
  .name mark {
    background: none;
    color: var(--accent);
    font-weight: 650;
    text-decoration: underline;
    text-decoration-color: var(--accent-line);
    text-decoration-thickness: 1px;
    text-underline-offset: 2.5px;
  }
  .file-item.selected .name mark {
    color: var(--accent);
  }
  .place {
    display: flex;
    gap: 4px;
    justify-self: start;
    min-width: 0;
    max-width: 100%;
    height: 24px;
    align-items: center;
    padding: 0 6px;
    margin-left: -6px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    font-size: var(--text-sm);
    color: var(--ink-3);
    white-space: nowrap;
  }
  .place-up {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ink-4);
  }
  .place-dir {
    flex: none;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
    color: var(--ink-2);
  }
  .place:hover {
    background: color-mix(in oklab, var(--ink) 6%, transparent);
  }
  .place:hover .place-dir {
    color: var(--accent);
    text-decoration: underline;
    text-decoration-color: var(--accent-line);
    text-underline-offset: 2.5px;
  }
  .place:active {
    background: color-mix(in oklab, var(--ink) 10%, transparent);
  }
  .place:focus-visible {
    outline-offset: -2px;
  }

  .file-item[data-layout='grid'].selected .visual::after {
    content: '';
    position: absolute;
    inset: -5px;
    border-radius: var(--radius-md);
    box-shadow:
      0 0 0 2px var(--select-ring),
      0 0 0 5px var(--select-wash);
    pointer-events: none;
  }
  .file-item[data-layout='grid'].selected .name {
    color: var(--accent);
    font-weight: 600;
  }
  .file-item[data-layout='grid'].selected .meta {
    color: var(--ink-2);
  }
  :global(.file-view:focus-visible) .file-item.focused:not(.selected) {
    outline: 2px solid var(--focus);
    outline-offset: 1px;
  }
  :global(.file-view:focus-visible) .file-item[data-layout='list'].focused:not(.selected) {
    outline-offset: -2px;
  }
  .file-item.drop-target {
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }
  .file-item.drop-target :global(.envelope .contents) {
    --nv-dy: -6%;
  }

  .file-item[data-layout='list'] {
    display: grid;
    grid-template-columns: var(--list-cols);
    align-items: center;
    gap: var(--space-3);
    min-height: var(--row-h, 38px);
    padding: 4px var(--space-3);
    border-radius: var(--radius-sm);
  }
  .file-item[data-layout='list'] + :global(.file-item[data-layout='list']) {
    box-shadow: 0 -1px 0 var(--paper-rule);
  }
  .file-item[data-layout='list']:hover {
    background: color-mix(in oklab, var(--ink) 4%, transparent);
  }
  .file-item[data-layout='list'].selected {
    background: var(--select-wash);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--select-ring) 22%, transparent);
  }
  .file-item[data-layout='list'] .visual {
    display: block;
    width: var(--row-vis-w, 30px);
    height: var(--row-vis-h, 26px);
  }
  .file-item[data-layout='list'] .visual > :global(.thumb) {
    width: 100%;
    height: 100%;
  }
  .file-item[data-layout='list'] .visual :global(.print) {
    padding: 1.5px;
    box-shadow: 0 0 0 0.5px rgb(var(--cast) / 0.2), 0 1px 2px rgb(var(--cast) / 0.2), var(--print-glow);
  }
  .file-item[data-layout='list'] .visual :global(.paper-file) {
    height: var(--row-vis-h, 26px);
  }
  :global([data-density='compact']) .file-item[data-layout='list'] {
    padding-block: 1px;
  }
  :global([data-density='compact']) .file-item[data-layout='list'] .name {
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }
  .file-item[data-layout='list'] .name {
    font-size: var(--text-md);
    font-weight: 500;
    line-height: 1.3;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .file-item[data-layout='list'].selected .name {
    color: var(--ink);
    font-weight: 600;
  }
  .size {
    text-align: right;
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  .size,
  .mtime,
  .type {
    font-size: var(--text-sm);
    color: var(--ink-3);
    white-space: nowrap;
  }
  .mtime {
    padding-left: 18px;
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  .file-item[data-layout='list'] .size {
    font-size: 11.5px;
  }
  .selected .size,
  .selected .mtime,
  .selected .type {
    color: var(--ink-2);
  }
</style>
