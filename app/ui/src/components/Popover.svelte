<script lang="ts">
  import { pointerLast } from '../lib/modality';
  import { isMobile } from '../lib/platform';
  import { sheetSwipe } from '../lib/sheetSwipe';
  import type { Snippet } from 'svelte';
  import { onMount, tick } from 'svelte';

  let {
    x,
    y,
    label,
    onclose,
    children,
    ignore = null,
    width = 260,
    testid,
  }: {
    x: number;
    y: number;
    label: string;
    onclose: (restoreFocus: boolean) => void;
    children: Snippet;
    ignore?: HTMLElement | null;
    width?: number;
    testid?: string;
  } = $props();

  let el: HTMLDivElement | undefined = $state();
  let pos = $state<{ left: number; top: number } | null>(null);

  function clamp() {
    if (!el) return;
    const r = el.getBoundingClientRect();
    const left = Math.max(8, Math.min(x, window.innerWidth - r.width - 8));
    const below = y;
    const top = below + r.height + 8 <= window.innerHeight ? below : Math.max(8, window.innerHeight - r.height - 8);
    if (!pos || pos.left !== left || pos.top !== top) pos = { left, top };
  }

  onMount(() => {
    let ro: ResizeObserver | undefined;
    void tick().then(() => {
      if (!el) return;
      clamp();
      ro = new ResizeObserver(clamp);
      ro.observe(el);
      const first = el.querySelector<HTMLElement>('[data-autofocus], input, [role="checkbox"], [role="radio"], button:not(:disabled)');
      const field = first?.matches('input:not([type="checkbox"], [type="radio"])');
      (pointerLast() && !field ? el : first)?.focus({ preventScroll: true });
    });
    return () => ro?.disconnect();
  });

  function outside(e: PointerEvent) {
    const t = e.target as Node;
    if (!el || el.contains(t) || (ignore && ignore.contains(t))) return;
    onclose(false);
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose(true);
    }
  }

  function focusout(e: FocusEvent) {
    const to = e.relatedTarget as Node | null;
    if (to && el && !el.contains(to) && !(ignore && ignore.contains(to))) onclose(false);
  }
</script>

<svelte:window onpointerdown={outside} onresize={() => isMobile || onclose(false)} />

<div
  bind:this={el}
  class="popover"
  role="dialog"
  aria-label={label}
  tabindex="-1"
  data-focus-panel
  data-testid={testid}
  style:left="{pos?.left ?? x}px"
  style:top="{pos?.top ?? y}px"
  style:width="{width}px"
  style:opacity={pos ? null : 0}
  class:nv-enter={pos !== null}
  onkeydown={keydown}
  onfocusout={focusout}
  oncontextmenu={(e) => e.preventDefault()}
  use:sheetSwipe={{ onclose: () => onclose(false), enabled: () => isMobile }}
>
  {@render children()}
</div>

<style>
  .popover {
    position: fixed;
    z-index: var(--z-menu);
    max-width: calc(100vw - 16px);
    max-height: calc(100vh - 16px);
    overflow-y: auto;
    padding: 6px;
    border-radius: var(--radius-md);
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      var(--shadow-pop);
  }
  .popover.nv-enter {
    animation: nv-enter 90ms var(--ease-out) backwards;
  }
  .popover:focus-visible {
    outline: none;
  }

  .popover :global(.pop-head) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px 6px 9px;
  }
  .popover :global(.pop-sep) {
    height: 1px;
    margin: 5px 6px;
    background: var(--paper-rule);
  }
  .popover :global(.pop-note) {
    padding: 8px 9px 10px;
    font-size: var(--text-sm);
    color: var(--ink-3);
    text-wrap: pretty;
  }
  .popover :global(.opt) {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
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
  .popover :global(.opt:hover:not(:disabled)) {
    background: color-mix(in oklab, var(--ink) 5%, transparent);
  }
  .popover :global(.opt:active:not(:disabled)) {
    background: color-mix(in oklab, var(--ink) 9%, transparent);
  }
  .popover :global(.opt:focus-visible) {
    outline: none;
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--accent) 45%, transparent);
  }
  .popover :global(.opt:disabled) {
    color: var(--ink-4);
  }
  .popover :global(.opt .lbl) {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .popover :global(.opt .count) {
    flex: none;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .popover :global(.opt .glyph) {
    flex: none;
    display: inline-flex;
    color: var(--ink-3);
  }
  .popover :global(.box) {
    position: relative;
    flex: none;
    width: 15px;
    height: 15px;
    border-radius: 3.5px;
    background: var(--field);
    box-shadow:
      inset 0 0 0 1px var(--field-edge),
      inset 0 1px 2px var(--field-shade);
  }
  .popover :global([role='radio'] .box) {
    border-radius: 50%;
  }
  .popover :global([aria-checked='true'] .box),
  .popover :global([aria-checked='mixed'] .box) {
    background: var(--accent-deep);
    box-shadow:
      inset 0 0 0 1px color-mix(in oklab, var(--accent-deep) 70%, black),
      inset 0 1px 0 rgb(255 255 255 / 0.18);
  }
  .popover :global([role='checkbox'][aria-checked='true'] .box::after) {
    content: '';
    position: absolute;
    left: 5px;
    top: 2px;
    width: 4px;
    height: 8px;
    border: solid var(--key-accent-ink, #fff);
    border-width: 0 1.75px 1.75px 0;
    transform: rotate(42deg);
  }
  .popover :global([aria-checked='mixed'] .box::after) {
    content: '';
    position: absolute;
    left: 3.5px;
    right: 3.5px;
    top: 6.75px;
    height: 1.5px;
    border-radius: 1px;
    background: var(--key-accent-ink, #fff);
  }
  .popover :global([role='radio'][aria-checked='true'] .box::after) {
    content: '';
    position: absolute;
    inset: 4.5px;
    border-radius: 50%;
    background: var(--key-accent-ink, #fff);
  }
</style>
