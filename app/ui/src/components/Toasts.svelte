<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { CircleAlert, CircleCheck, Info, X } from '@lucide/svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
</script>

<section class="toasts" aria-label={m.notifications()}>
  <div class="live" role="alert" aria-live="assertive">
    {#each toasts.items.filter((t) => t.kind === 'error') as t (t.id)}
      {@render toast(t)}
    {/each}
  </div>
  <div class="live" role="status" aria-live="polite">
    {#each toasts.items.filter((t) => t.kind !== 'error') as t (t.id)}
      {@render toast(t)}
    {/each}
  </div>
</section>

{#snippet toast(t: { id: number; kind: string; text: string })}
  <div class="toast nv-enter" data-kind={t.kind}>
    <span class="glyph" aria-hidden="true">
      {#if t.kind === 'error'}<CircleAlert size={16} />{:else if t.kind === 'success'}<CircleCheck size={16} />{:else}<Info size={16} />{/if}
    </span>
    <span class="text">{t.text}</span>
    <button class="btn ghost icon close" type="button" aria-label={m.action_close()} onclick={() => toasts.dismiss(t.id)}>
      <X size={14} />
    </button>
  </div>
{/snippet}

<style>
  .toasts {
    position: fixed;
    left: calc(var(--sidebar-w) + var(--space-4));
    bottom: calc(var(--statusbar-h) + var(--space-3));
    z-index: var(--z-toast);
    display: flex;
    flex-direction: column-reverse;
    align-items: flex-start;
    gap: var(--space-2);
    width: min(400px, calc(100vw - var(--sidebar-w) - 32px));
    pointer-events: none;
  }
  :global(.app[data-world='locked']) .toasts {
    left: auto;
    right: var(--space-4);
    top: var(--space-4);
    bottom: auto;
    flex-direction: column;
    align-items: flex-end;
    width: min(400px, calc(100vw - 32px));
  }
  .live {
    display: contents;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    max-width: 100%;
    padding: 10px 6px 10px 12px;
    border-radius: var(--radius-md);
    background: var(--paper-hi);
    color: var(--ink);
    font-size: var(--text-md);
    line-height: 1.4;
    box-shadow:
      inset 3px 0 0 var(--mark, var(--ink-4)),
      inset 0 1px 0 var(--key-lip),
      var(--shadow-pop);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateY(6px);
    --enter-to: translateY(0);
  }
  .toast[data-kind='success'] {
    --mark: var(--accent);
  }
  .toast[data-kind='error'] {
    --mark: var(--danger);
  }
  .glyph {
    display: inline-flex;
    margin-top: 1px;
    color: var(--mark, var(--ink-3));
  }
  .text {
    flex: 1;
    min-width: 0;
    padding-top: 1px;
    overflow-wrap: anywhere;
  }
  .close {
    flex: none;
    width: 24px;
    min-height: 24px;
    margin: -2px 0;
  }
</style>
