<script lang="ts">
  import { app } from '../lib/stores/app.svelte';
  import { update } from '../lib/stores/update.svelte';
  import { CircleArrowUp } from '@lucide/svelte';
  import { m } from '../paraglide/messages.js';

  let { on = 'paper' }: { on?: 'paper' | 'cloth' } = $props();

  const pct = $derived(update.progress === null ? null : Math.round(update.progress * 100));
  const show = $derived(update.pending || update.phase === 'ready' || update.phase === 'saved');
</script>

{#if show}
  <section class="update-notice" data-phase={update.phase} data-on={on} aria-live="polite" aria-label={m.set_updates()} data-testid="update-notice">
    <span class="mark" aria-hidden="true"><CircleArrowUp size={17} strokeWidth={1.6} /></span>
    <div class="text">
      {#if update.release}
        <p class="title">{m.upd_new()} <span class="version mono">{update.release.version}</span></p>
      {/if}
      {#if update.phase === 'downloading'}
        <progress max="100" value={pct ?? undefined} aria-label={pct === null ? m.upd_downloading() : m.upd_downloading_pct({ pct: String(pct) })}></progress>
      {:else if update.phase === 'installing'}
        <p class="status">{m.upd_installing()}</p>
      {:else if update.phase === 'ready'}
        <p class="status">{m.upd_ready()}</p>
      {:else if update.phase === 'error'}
        <p class="status error" role="alert">{update.error}</p>
      {/if}
      <div class="actions">
        {#if update.phase === 'available'}
          <button type="button" class="btn sm primary" onclick={() => update.install()}>{m.upd_install()}</button>
        {:else if update.phase === 'ready'}
          <button type="button" class="btn sm primary" onclick={() => update.restart()}>{m.upd_restart()}</button>
        {/if}
        <button type="button" class="btn sm ghost" onclick={() => app.openSettings()}>{m.upd_more()}</button>
      </div>
    </div>
  </section>
{/if}

<style>
  .update-notice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .update-notice[data-on='paper'] {
    align-self: flex-start;
    margin-top: var(--space-3);
    padding: 10px 12px 10px 10px;
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1px var(--key-edge),
      var(--lift-key);
  }
  .update-notice[data-on='cloth'] {
    padding: 9px 10px 10px 9px;
    border-radius: var(--radius-md);
    color: var(--cloth-ink-3);
    background: var(--cloth-bed);
    box-shadow:
      inset 0 1px 3px rgb(0 0 0 / 0.35),
      0 1px 0 rgb(255 255 255 / 0.07);
  }
  .mark {
    display: grid;
    place-items: center;
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  [data-on='cloth'] .mark {
    color: var(--foil);
    background: color-mix(in oklab, var(--cloth-bed) 70%, #000);
    box-shadow: inset 0 1px 2px rgb(0 0 0 / 0.4);
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding-top: 4px;
  }
  .title {
    font-weight: 600;
    color: var(--ink);
    line-height: 1.3;
  }
  [data-on='cloth'] .title {
    color: var(--cloth-ink);
  }
  .version {
    margin-left: 2px;
    padding: 0 5px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    font-weight: 600;
    color: var(--accent);
    background: var(--accent-soft);
  }
  [data-on='cloth'] .version {
    color: var(--foil);
    background: color-mix(in oklab, var(--cloth-bed) 60%, #000);
  }
  .status {
    color: inherit;
  }
  .status.error {
    color: var(--danger);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
</style>
