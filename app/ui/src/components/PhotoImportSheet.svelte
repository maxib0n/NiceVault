<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { isIos } from '../lib/platform';
  import { photos } from '../lib/stores/photos.svelte';
  import type { PickReason } from '../lib/types';

  const run = $derived(photos.run);
  const reasons: Record<PickReason, () => string> = {
    offline: m.pick_reason_offline,
    icloud: m.pick_reason_icloud,
    stalled: m.pick_reason_stalled,
    unsupported: m.pick_reason_unsupported,
    no_space: m.pick_reason_no_space,
    cancelled: m.pick_reason_cancelled,
    expired: m.pick_reason_expired,
    other: m.pick_reason_other,
  };
  const shown = $derived(run ? Math.min(run.total, run.done + (run.fetching > 0 ? 1 : 0)) : 0);
  const pct = $derived(run ? Math.round((run.finished ? 1 : run.fraction) * 1000) / 10 : 0);
</script>

{#if run}
  <section class="pk nv-enter" aria-label={m.pick_region()} data-testid="photo-import" data-finished={run.finished}>
    {#if !run.finished}
      <p class="head" aria-live="polite" data-testid="photo-import-status">
        {(isIos ? m.pick_fetch_ios : m.pick_fetch)({ n: String(shown), total: String(run.total) })}
      </p>
      <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={m.pick_region()}>
        <span style:transform={`scaleX(${pct / 100})`}></span>
      </div>
      <div class="row">
        <p class="sub">
          {run.failed > 0 ? m.pick_sub({ imported: String(run.imported), failed: String(run.failed) }) : m.pick_sub_ok({ imported: String(run.imported) })}
        </p>
        <button type="button" class="btn ghost" data-testid="photo-import-cancel" onclick={() => photos.cancel()}>{m.pick_cancel()}</button>
      </div>
    {:else}
      <p class="head" aria-live="polite" data-testid="photo-import-status">
        {(run.cancelled ? m.pick_cancelled : m.pick_done)({ imported: String(run.imported), total: String(run.total) })}
      </p>
      {#if run.failures.length}
        <p class="sub">{m.pick_failed_head({ count: String(run.failures.length) })}</p>
        <ul class="fails" data-testid="photo-import-failures">
          {#each run.failures as f (f.index)}
            <li><span class="name">{f.name || `#${f.index + 1}`}</span><span class="why">{reasons[f.reason]?.() ?? m.pick_reason_other()}</span></li>
          {/each}
        </ul>
      {/if}
      <div class="row end">
        <button type="button" class="btn ghost" data-testid="photo-import-close" onclick={() => photos.close()}>{m.pick_close()}</button>
        {#if run.failures.length}
          <button type="button" class="btn primary" data-testid="photo-import-retry" onclick={() => photos.retry()}>{m.pick_retry()}</button>
        {/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .pk {
    position: fixed;
    inset: auto 0 0;
    z-index: calc(var(--z-menu) - 1);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: min(60vh, 460px);
    padding: 14px max(16px, var(--safe-right, 0px)) calc(12px + var(--safe-bottom, 0px)) max(16px, var(--safe-left, 0px));
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--slip-lip, var(--key-lip)),
      0 -1px 0 rgb(var(--cast) / 0.2),
      0 -12px 28px -14px rgb(var(--cast) / 0.55);
    --enter-from: translate3d(0, 24px, 0);
    --enter-to: translate3d(0, 0, 0);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
  }
  @media (min-width: 641px) {
    .pk {
      inset: auto 16px 16px auto;
      width: 380px;
      border-radius: 10px;
    }
  }
  .head {
    margin: 0;
    font-weight: 600;
    font-size: var(--text-base);
    font-variant-numeric: tabular-nums;
  }
  .sub {
    flex: 1;
    min-width: 0;
    margin: 0;
    color: var(--ink-soft, var(--ink));
    font-size: var(--text-sm, 0.875rem);
    font-variant-numeric: tabular-nums;
  }
  .bar {
    height: 4px;
    border-radius: 2px;
    overflow: hidden;
    background: rgb(var(--cast) / 0.14);
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform 240ms linear;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row.end {
    justify-content: flex-end;
  }
  .btn {
    min-height: 44px;
  }
  .fails {
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-y: auto;
    font-size: var(--text-sm, 0.875rem);
  }
  .fails li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 0;
    border-top: 1px solid rgb(var(--cast) / 0.12);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .why {
    flex: none;
    color: var(--danger);
  }
  @media (prefers-reduced-motion: reduce) {
    .pk {
      animation: none;
    }
    .bar span {
      transition: none;
    }
  }
</style>
