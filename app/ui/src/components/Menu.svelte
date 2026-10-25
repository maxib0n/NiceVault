<!-- Floating menu (context menu, sort menu). Keyboard: ↑ ↓ Home End, Enter/Space, Esc, Tab closes. -->
<script lang="ts" module>
  import type { Component } from 'svelte';

  export interface MenuItem {
    id: string;
    label: string;
    icon?: Component<{ size?: number }>;
    shortcut?: string;
    danger?: boolean;
    disabled?: boolean;
    /** renders as menuitemradio/checkbox with aria-checked */
    checked?: boolean;
    separatorBefore?: boolean;
    action: () => void;
  }
</script>

<script lang="ts">
  import { pointerLast } from '../lib/modality';
  import { isMobile } from '../lib/platform';
  import { sheetSwipe } from '../lib/sheetSwipe';
  import { Check } from '@lucide/svelte';
  import { onMount, tick } from 'svelte';
  import { compose } from '../lib/reveal';

  let {
    items,
    x,
    y,
    label,
    title,
    onclose,
  }: {
    items: MenuItem[];
    x: number;
    y: number;
    label: string;
    /** a phone's sheet names what it acts on at its head (the item, "3 selezionati") */
    title?: string;
    onclose: (restoreFocus: boolean) => void;
  } = $props();

  let el: HTMLDivElement | undefined = $state();
  let adjusted = $state<{ left: number; top: number } | null>(null);
  const pos = $derived(adjusted ?? { left: x, top: y });

  function buttons(): HTMLButtonElement[] {
    return el ? [...el.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]:not(:disabled)')] : [];
  }

  onMount(async () => {
    await tick();
    if (!el) return;
    // keep inside the viewport
    const r = el.getBoundingClientRect();
    adjusted = {
      left: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)),
      top: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)),
    };
    // opened by a click: the menu itself takes focus (no item lit until a key moves); opened
    // from the keyboard: its first item, as before (lib/modality.ts)
    if (pointerLast()) el.focus();
    else buttons()[0]?.focus();
  });

  function keydown(e: KeyboardEvent) {
    const list = buttons();
    const i = list.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === 'ArrowDown') list[(i + 1) % list.length]?.focus();
    else if (e.key === 'ArrowUp') list[i < 0 ? list.length - 1 : (i - 1 + list.length) % list.length]?.focus();
    else if (e.key === 'Home') list[0]?.focus();
    else if (e.key === 'End') list[list.length - 1]?.focus();
    else if (e.key === 'Escape') onclose(true);
    else if (e.key === 'Tab') onclose(false);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function outside(e: PointerEvent) {
    if (el && !el.contains(e.target as Node)) onclose(false);
  }

  function run(item: MenuItem) {
    onclose(true);
    item.action();
  }
</script>

<!-- on a phone it is a sheet from the bottom (styles/mobile.css): the keyboard coming up resizes the
     window, which must not close it -->
<svelte:window onpointerdown={outside} onblur={() => onclose(false)} onresize={() => isMobile || onclose(false)} />

<div
  bind:this={el}
  class="menu"
  role="menu"
  tabindex="-1"
  data-focus-panel
  aria-label={label}
  style="left: {pos.left}px; top: {pos.top}px; opacity: {adjusted ? 1 : 0}"
  onkeydown={keydown}
  oncontextmenu={(e) => e.preventDefault()}
  use:compose={adjusted !== null}
  use:sheetSwipe={{ onclose: () => onclose(false), enabled: () => isMobile }}
>
  {#if isMobile && title}<p class="sheet-title" aria-hidden="true">{title}</p>{/if}
  {#each items as item (item.id)}
    {#if item.separatorBefore}<div class="sep" role="separator"></div>{/if}
    <button
      type="button"
      class="item"
      class:danger={item.danger}
      role={item.checked === undefined ? 'menuitem' : 'menuitemradio'}
      aria-checked={item.checked}
      disabled={item.disabled}
      tabindex="-1"
      onclick={() => run(item)}
    >
      <span class="icon" aria-hidden="true">
        {#if item.icon}<item.icon size={15} />{:else if item.checked}<Check size={15} />{/if}
      </span>
      <span class="label">{item.label}</span>
      {#if item.shortcut}<kbd class="shortcut">{item.shortcut}</kbd>{/if}
    </button>
  {/each}
</div>

<style>
  /* a slip of paper lifted off the page: one soft elevation, hairline edge */
  .menu {
    position: fixed;
    z-index: var(--z-menu);
    min-width: 210px;
    max-width: min(340px, calc(100vw - 8px));
    padding: 5px;
    border-radius: var(--radius-md);
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      var(--shadow-pop);
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 31px;
    padding: 0 10px 0 9px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    font-size: var(--text-md);
    font-weight: 500;
    text-align: left;
    color: var(--ink);
  }
  .item:hover:not(:disabled) {
    background: color-mix(in oklab, var(--ink) 5%, transparent);
  }
  /* keyboard: the row is washed in the volume's ink, a hairline round it */
  .item:focus-visible {
    outline: none;
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--accent) 45%, transparent);
  }
  .item:active:not(:disabled) {
    background: color-mix(in oklab, var(--ink) 9%, transparent);
  }
  .item:disabled {
    color: var(--ink-4);
  }
  .item:disabled .icon {
    opacity: 0.6;
  }
  .item.danger {
    color: var(--danger);
  }
  .item.danger:hover:not(:disabled),
  .item.danger:focus-visible {
    background: var(--danger-wash);
  }
  .item.danger:focus-visible {
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--danger) 45%, transparent);
  }
  .icon {
    width: 16px;
    flex: none;
    display: inline-flex;
    justify-content: center;
    color: var(--ink-3);
  }
  .item[aria-checked='true'] .icon {
    color: var(--accent);
  }
  .danger .icon {
    color: inherit;
  }
  .label {
    flex: 1;
    white-space: nowrap;
  }
  .shortcut {
    margin-left: var(--space-4);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .danger .shortcut {
    color: inherit;
    opacity: 0.8;
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--paper-rule);
  }
</style>
