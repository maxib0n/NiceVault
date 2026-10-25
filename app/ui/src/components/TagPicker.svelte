<!--
  Tags of the selection (context menu "Tag…", the toolbar's Tag key): every tag of the volume
  with a box that says whether all the selected items carry it, some of them (mixed) or none;
  a press gives it to all of them, or takes it from all when they all have it. The field finds
  a tag, or creates it ("Crea «…»", the next ink) and gives it at once. Enter in the field:
  the first tag found, else the new one.
  Hooks: [data-testid=tag-picker], [data-testid=tag-picker-input], options [data-tag]
  (role=checkbox, aria-checked true | mixed | false), [data-testid=tag-create].
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Plus, Search, Tags } from '@lucide/svelte';
  import { fold, MAX_TAG_NAME, TAG_COLORS } from '../lib/search';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import { tagErrorText } from '../lib/stores/search.svelte';
  import type { Tag } from '../lib/types';
  import Popover from './Popover.svelte';
  import TagChip from './TagChip.svelte';

  let { browser, onclosed }: { browser: FileBrowser; onclosed?: (restore: boolean) => void } = $props();

  const t = $derived(browser.tagger ?? { x: 0, y: 0, nodes: [] });
  let text = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);

  const nodes = $derived(t.nodes.map((id) => browser.byId.get(id)).filter((n) => !!n));
  const label = $derived(m.tag_picker_label({ name: browser.labelFor(nodes) }));
  const norm = (s: string) => fold(s).split(/\s+/).filter(Boolean).join(' ');
  const shown = $derived(text.trim() ? browser.tags.list.filter((x) => norm(x.name).includes(norm(text))) : browser.tags.list);
  const exact = $derived(text.trim() ? browser.tags.list.find((x) => norm(x.name) === norm(text)) : undefined);

  function checkedOf(tag: Tag): 'true' | 'mixed' | 'false' {
    let n = 0;
    for (const node of nodes) if (node.tags?.includes(tag.id)) n++;
    return n === 0 ? 'false' : n === nodes.length ? 'true' : 'mixed';
  }

  async function flip(tag: Tag) {
    if (busy || !nodes.length) return;
    busy = true;
    error = null;
    try {
      await browser.tags.assign([tag.id], nodes.map((n) => n.id), checkedOf(tag) !== 'true');
    } catch (e) {
      error = tagErrorText(e);
    } finally {
      busy = false;
    }
  }

  async function create() {
    const name = text.trim();
    if (!name || busy) return;
    busy = true;
    error = null;
    try {
      const color = TAG_COLORS[browser.tags.list.length % (TAG_COLORS.length - 1)];
      const tag = await browser.tags.create(name, color);
      await browser.tags.assign([tag.id], nodes.map((n) => n.id), true);
      text = '';
    } catch (e) {
      error = tagErrorText(e);
    } finally {
      busy = false;
    }
  }

  function fieldKeys(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (exact) void flip(exact);
      else if (text.trim() && !shown.length) void create();
      else if (shown[0]) void flip(shown[0]);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      (e.currentTarget as HTMLElement).closest('.popover')?.querySelector<HTMLElement>('.opt')?.focus();
    }
  }

  function listKeys(e: KeyboardEvent) {
    const box = e.currentTarget as HTMLElement;
    const opts = [...box.querySelectorAll<HTMLElement>('.opt:not(:disabled)')];
    const i = opts.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowUp' && i === 0) {
      e.preventDefault();
      box.closest('.popover')?.querySelector<HTMLInputElement>('input')?.focus();
      return;
    }
    const to = e.key === 'ArrowDown' ? i + 1 : e.key === 'ArrowUp' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? opts.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    opts[Math.max(0, Math.min(opts.length - 1, to))]?.focus();
  }

  function close(restore: boolean) {
    browser.tagger = null;
    onclosed?.(restore);
  }
</script>

<Popover x={t.x} y={t.y} {label} onclose={close} width={272} testid="tag-picker">
  <div class="find">
    <Search size={14} aria-hidden="true" />
    <input
      class="input"
      type="text"
      bind:value={text}
      oninput={() => (error = null)}
      onkeydown={fieldKeys}
      placeholder={m.tag_search_ph()}
      aria-label={m.tag_search_ph()}
      aria-invalid={error ? 'true' : undefined}
      aria-describedby={error ? 'tag-picker-error' : undefined}
      maxlength={MAX_TAG_NAME}
      spellcheck="false"
      autocomplete="off"
      data-autofocus
      data-testid="tag-picker-input"
    />
  </div>
  {#if error}<p class="error-text pick-error" id="tag-picker-error" role="alert">{error}</p>{/if}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="opts" onkeydown={listKeys} aria-busy={busy}>
    {#each shown as tag (tag.id)}
      <button type="button" class="opt" role="checkbox" aria-checked={checkedOf(tag)} data-tag={tag.id} onclick={() => flip(tag)}>
        <span class="box" aria-hidden="true"></span>
        <span class="lbl"><TagChip {tag} /></span>
        <span class="count">{tag.count}</span>
      </button>
    {/each}
    {#if text.trim() && !exact}
      <button type="button" class="opt create" data-testid="tag-create" onclick={create}>
        <span class="glyph" aria-hidden="true"><Plus size={15} /></span>
        <span class="lbl">{m.tag_create_named({ name: text.trim() })}</span>
      </button>
    {/if}
  </div>
  {#if !browser.tags.list.length && !text.trim()}<p class="pop-note">{m.tag_none_yet()}</p>{/if}
  <div class="pop-sep" role="separator"></div>
  <button
    type="button"
    class="opt"
    data-testid="tag-picker-manage"
    onclick={() => {
      const at = { x: t.x, y: t.y };
      browser.tagger = null;
      browser.tagManager = at;
    }}
  >
    <span class="glyph" aria-hidden="true"><Tags size={15} /></span>
    <span class="lbl">{m.tag_manage()}</span>
  </button>
</Popover>

<style>
  .find {
    position: relative;
    display: flex;
    align-items: center;
    margin: 2px 2px 6px;
  }
  .find > :global(svg) {
    position: absolute;
    left: 10px;
    color: var(--ink-3);
    pointer-events: none;
  }
  .find .input {
    flex: 1;
    height: 30px;
    padding-left: 30px;
    font-size: var(--text-md);
  }
  .pick-error {
    margin: -2px 6px 6px;
  }
  .opts {
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-height: 280px;
    overflow-y: auto;
  }
  .create .lbl {
    color: var(--accent);
    font-weight: 600;
  }
  .create .glyph {
    color: var(--accent);
  }
</style>
