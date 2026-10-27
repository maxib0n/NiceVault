<script lang="ts">
  import { pointerLast } from '../lib/modality';
  import type { Snippet } from 'svelte';
  import Band from '../ornament/Band.svelte';
  import { isMobile } from '../lib/platform';
  import { sheetSwipe } from '../lib/sheetSwipe';

  let {
    open,
    title,
    onclose,
    children,
    actions,
    variant = 'default',
    describedby,
  }: {
    open: boolean;
    title: string;
    onclose: () => void;
    children?: Snippet;
    actions?: Snippet;
    variant?: 'default' | 'danger' | 'wide';
    describedby?: string;
  } = $props();

  const SHEET = ':is([data-variant="danger"], :not(:has(input:not([type="hidden"]), textarea, select)))';
  let el: HTMLDialogElement | undefined = $state();
  const titleId = `dlg-${Math.random().toString(36).slice(2, 8)}`;

  $effect(() => {
    if (!el) return;
    if (open && !el.open) {
      el.showModal();
      queueMicrotask(() => {
        const a = document.activeElement;
        if (!(a instanceof HTMLElement) || !el?.contains(a)) return;
        if (a.matches('input:not([type="checkbox"], [type="radio"], [type="range"], [type="button"]), textarea, [contenteditable="true"]')) return;
        if (pointerLast()) {
          el.focus();
          return;
        }
        if (a.getAttribute('tabindex') !== '-1') return;
        const into = a.querySelector<HTMLElement>('[role="radio"][tabindex="0"], [role="radio"][aria-checked="true"]');
        (into ?? el.querySelector<HTMLElement>('input, select, textarea, button:not([tabindex="-1"]), [tabindex="0"]'))?.focus();
      });
    } else if (!open && el.open) el.close();
  });

  function backdrop(e: MouseEvent) {
    if (e.target === el) onclose();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={el}
  class="dialog nv-enter"
  tabindex="-1"
  data-variant={variant}
  aria-labelledby={titleId}
  aria-describedby={describedby}
  onclose={() => {
    // a stale close event (queued before the dialog was opened again) is ignored
    if (!el?.open) onclose();
  }}
  onclick={backdrop}
  use:sheetSwipe={{ onclose, enabled: () => isMobile && !!el?.matches(SHEET) }}
>
  {#if open}
    <div class="panel">
      <Band height={8} />
      <h2 class="title" id={titleId}>{title}</h2>
      <div class="body">
        {@render children?.()}
      </div>
      {#if actions}
        <div class="actions">
          {@render actions()}
        </div>
      {/if}
    </div>
  {/if}
</dialog>

<style>
  .dialog {
    padding: 0;
    border: 0;
    border-radius: var(--radius-lg);
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      var(--shadow-pop);
    width: min(460px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow: auto;
  }
  .dialog[open] {
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateY(4px) scale(0.985);
    --enter-to: translateY(0) scale(1);
  }
  .dialog[data-variant='wide'] {
    width: min(600px, calc(100vw - 32px));
  }
  .dialog::backdrop {
    background: var(--scrim);
  }
  .panel {
    --bleed-start: var(--space-5);
    --bleed-end: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: 14px var(--space-5) var(--space-5);
  }
  .title {
    font-family: var(--font-display);
    font-size: 24px;
    font-weight: 500;
    line-height: 1.15;
    letter-spacing: -0.01em;
    color: var(--title-ink);
    overflow-wrap: break-word;
    text-wrap: balance;
  }
  .dialog[data-variant='danger'] .title {
    color: var(--ink);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    font-size: var(--text-base);
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }
  .body:empty {
    display: none;
  }
  .body > :global(:last-child) {
    margin-bottom: 0;
  }
  .body :global(.field),
  .body :global(.password-field) {
    color: var(--ink);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding-top: var(--space-4);
    border-top: 1px solid var(--paper-rule);
  }
  .actions :global(.btn) {
    min-width: 92px;
  }
</style>
