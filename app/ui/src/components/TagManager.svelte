<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Plus, Trash2 } from '@lucide/svelte';
  import { ACCENTS } from '../lib/accents';
  import { MAX_TAG_NAME, TAG_COLORS, tagColor } from '../lib/search';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import { tagErrorText } from '../lib/stores/search.svelte';
  import { radioKeys } from '../lib/radio';
  import type { Tag, TagColor, TagId } from '../lib/types';
  import Popover from './Popover.svelte';

  let { browser, onclosed }: { browser: FileBrowser; onclosed?: (restore: boolean) => void } = $props();

  const at = $derived(browser.tagManager ?? { x: 0, y: 0 });
  const list = $derived(browser.tags.list);
  let newName = $state('');
  // svelte-ignore state_referenced_locally
  let newColor = $state<TagColor>(TAG_COLORS[browser.tags.list.length % (TAG_COLORS.length - 1)]);
  let error = $state<{ tag: TagId | 'new'; text: string } | null>(null);
  let inking = $state<TagId | 'new' | null>(null);
  let confirming = $state<TagId | null>(null);

  const inkName = (c: TagColor) => (c === 'neutral' ? m.tag_neutral() : (ACCENTS.find((a) => a.id === c)?.name() ?? c));

  async function add() {
    const name = newName.trim();
    if (!name) return;
    error = null;
    try {
      await browser.tags.create(name, newColor);
      newName = '';
      newColor = TAG_COLORS[browser.tags.list.length % (TAG_COLORS.length - 1)];
      inking = null;
    } catch (e) {
      error = { tag: 'new', text: tagErrorText(e) };
    }
  }

  async function rename(tag: Tag, input: HTMLInputElement) {
    const name = input.value.trim();
    if (name === tag.name) return;
    if (!name) {
      input.value = tag.name;
      return;
    }
    error = null;
    try {
      await browser.tags.update(tag.id, { name });
    } catch (e) {
      error = { tag: tag.id, text: tagErrorText(e) };
      input.value = tag.name;
    }
  }

  async function recolor(tag: Tag, color: TagColor) {
    error = null;
    try {
      await browser.tags.update(tag.id, { color });
    } catch (e) {
      error = { tag: tag.id, text: tagErrorText(e) };
    }
  }

  async function remove(tag: Tag) {
    error = null;
    try {
      await browser.tags.remove(tag.id);
      confirming = null;
    } catch (e) {
      error = { tag: tag.id, text: tagErrorText(e) };
    }
  }

  function nameKeys(e: KeyboardEvent, tag: Tag) {
    const input = e.currentTarget as HTMLInputElement;
    if (e.key === 'Enter') {
      e.preventDefault();
      void rename(tag, input);
    } else if (e.key === 'Escape' && input.value !== tag.name) {
      e.preventDefault();
      e.stopPropagation();
      input.value = tag.name;
    }
  }

  function close(restore: boolean) {
    browser.tagManager = null;
    onclosed?.(restore);
  }
</script>

{#snippet inks(current: TagColor, label: string, pick: (c: TagColor) => void)}
  <div class="inks" role="radiogroup" aria-label={label} tabindex="-1" onkeydown={(e) => radioKeys(e, TAG_COLORS, current, pick)}>
    {#each TAG_COLORS as c (c)}
      <button
        type="button"
        role="radio"
        class="swatch"
        aria-checked={current === c}
        aria-label={inkName(c)}
        title={inkName(c)}
        tabindex={current === c ? 0 : -1}
        data-accent={c === 'neutral' ? undefined : c}
        data-neutral={c === 'neutral' ? '' : undefined}
        data-color={c}
        data-testid="tag-swatch"
        onclick={() => pick(c)}
      ></button>
    {/each}
  </div>
{/snippet}

<Popover x={at.x} y={at.y} label={m.tag_manager_title()} onclose={close} width={340} testid="tag-manager">
  <div class="pop-head">
    <h3 class="title display">{m.tag_manager_title()}</h3>
  </div>

  <div class="row new">
    <button
      type="button"
      class="ink-key"
      aria-label={m.tag_color()}
      aria-expanded={inking === 'new'}
      data-accent={newColor === 'neutral' ? undefined : newColor}
      data-neutral={newColor === 'neutral' ? '' : undefined}
      onclick={() => (inking = inking === 'new' ? null : 'new')}
    ><span class="bead"></span></button>
    <input
      class="input name"
      type="text"
      bind:value={newName}
      placeholder={m.tag_new()}
      aria-label={m.tag_name()}
      maxlength={MAX_TAG_NAME}
      spellcheck="false"
      autocomplete="off"
      aria-invalid={error?.tag === 'new' ? 'true' : undefined}
      data-autofocus
      data-testid="tag-new-name"
      oninput={() => error?.tag === 'new' && (error = null)}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          void add();
        }
      }}
    />
    <button type="button" class="btn sm" disabled={!newName.trim()} data-testid="tag-new-add" onclick={add}><Plus size={14} aria-hidden="true" /> {m.tag_add()}</button>
  </div>
  {#if inking === 'new'}{@render inks(newColor, m.tag_color(), (c) => (newColor = c))}{/if}
  {#if error?.tag === 'new'}<p class="error-text row-error" role="alert">{error.text}</p>{/if}

  {#if list.length}<div class="pop-sep" role="separator"></div>{/if}

  <ul class="tags">
    {#each list as tag (tag.id)}
      {@const ink = tagColor(tag.color)}
      <li class="tag" data-testid="tag-row" data-tag={tag.id}>
        {#if confirming === tag.id}
          <div class="confirm" role="group" aria-label={m.tag_delete_q({ name: tag.name })}>
            <p class="confirm-q">{m.tag_delete_q({ name: tag.name })}</p>
            <p class="hint">{tag.count ? m.tag_delete_desc({ count: tag.count, n: String(tag.count) }) : m.tag_delete_unused()}</p>
            <div class="confirm-keys">
              <button type="button" class="btn sm" onclick={() => (confirming = null)}>{m.action_cancel()}</button>
              <!-- svelte-ignore a11y_autofocus -->
              <button type="button" class="btn sm danger" autofocus data-testid="tag-delete-confirm" onclick={() => remove(tag)}>{m.action_delete()}</button>
            </div>
          </div>
        {:else}
          <div class="row">
            <button
              type="button"
              class="ink-key"
              aria-label={m.tag_color_of({ name: tag.name })}
              aria-expanded={inking === tag.id}
              data-accent={ink === 'neutral' ? undefined : ink}
              data-neutral={ink === 'neutral' ? '' : undefined}
              data-testid="tag-ink"
              onclick={() => (inking = inking === tag.id ? null : tag.id)}
            ><span class="bead"></span></button>
            <input
              class="input name quiet"
              type="text"
              value={tag.name}
              aria-label={m.tag_rename({ name: tag.name })}
              maxlength={MAX_TAG_NAME}
              spellcheck="false"
              autocomplete="off"
              aria-invalid={error?.tag === tag.id ? 'true' : undefined}
              data-testid="tag-name-input"
              onkeydown={(e) => nameKeys(e, tag)}
              onblur={(e) => rename(tag, e.currentTarget)}
            />
            <span class="count" title={m.tag_used({ count: tag.count, n: String(tag.count) })}>{tag.count}</span>
            <button
              type="button"
              class="btn ghost sm icon del"
              aria-label={m.tag_delete({ name: tag.name })}
              title={m.tag_delete({ name: tag.name })}
              data-testid="tag-delete"
              onclick={() => ((confirming = tag.id), (inking = null))}><Trash2 size={14} /></button
            >
          </div>
          {#if inking === tag.id}{@render inks(ink, m.tag_color_of({ name: tag.name }), (c) => recolor(tag, c))}{/if}
          {#if error?.tag === tag.id}<p class="error-text row-error" role="alert">{error.text}</p>{/if}
        {/if}
      </li>
    {/each}
  </ul>
</Popover>

<style>
  .title {
    font-size: 19px;
    line-height: 1.15;
    color: var(--title-ink);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px;
  }
  .new {
    padding-bottom: 4px;
  }
  .name {
    flex: 1;
    min-width: 0;
    height: 30px;
    font-size: var(--text-md);
  }
  .name.quiet:not(:focus-visible):not([aria-invalid='true']) {
    border-color: transparent;
    background: transparent;
    box-shadow: none;
  }
  .name.quiet:hover:not(:focus-visible) {
    background: color-mix(in oklab, var(--ink) 4%, transparent);
  }
  .count {
    flex: none;
    min-width: 22px;
    text-align: right;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .del {
    flex: none;
  }
  .del:hover:not(:disabled) {
    color: var(--danger);
  }

  .ink-key {
    display: grid;
    place-items: center;
    flex: none;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
  }
  .ink-key:hover {
    background: color-mix(in oklab, var(--ink) 6%, transparent);
  }
  .ink-key:active,
  .ink-key[aria-expanded='true'] {
    background: color-mix(in oklab, var(--ink) 10%, transparent);
  }
  .bead {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow:
      inset 0 -1px 0 rgb(0 0 0 / 0.2),
      0 0 0 1.5px var(--paper-hi),
      0 0 0 2.5px color-mix(in oklab, var(--accent-line) 60%, transparent);
  }
  [data-neutral] > .bead {
    background: var(--ink-3);
    box-shadow:
      inset 0 -1px 0 rgb(0 0 0 / 0.2),
      0 0 0 1.5px var(--paper-hi),
      0 0 0 2.5px var(--paper-rule-2);
  }
  .inks {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 4px 8px 8px 38px;
  }
  .swatch {
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.18),
      inset 0 -1px 0 rgb(0 0 0 / 0.25);
  }
  .swatch[data-neutral] {
    background: var(--ink-3);
  }
  .swatch:hover {
    transform: translateY(-1px);
  }
  .swatch:active {
    transform: none;
  }
  .swatch[aria-checked='true'] {
    box-shadow:
      inset 0 -1px 0 rgb(0 0 0 / 0.25),
      0 0 0 2px var(--paper-hi),
      0 0 0 3.5px var(--ink-2);
  }
  .swatch:focus-visible {
    outline-offset: 3px;
  }
  .tags {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 320px;
    overflow-y: auto;
  }
  .tag + .tag {
    border-top: 1px solid var(--paper-rule);
  }
  .row-error {
    margin: 0 8px 6px 38px;
  }
  .confirm {
    padding: 8px 8px 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--danger-wash);
  }
  .confirm-q {
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .confirm .hint {
    margin-top: 2px;
  }
  .confirm-keys {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 8px;
  }
</style>
