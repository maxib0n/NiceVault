<!--
  The status line of a step the wizard is waiting on (DeviceWizard.svelte, in its footer, so it
  is always on screen): turning lathes + what is happening; after HINT_MS the seconds and a
  reassurance; after `stuckMs` Annulla / Riprova (when the step can be abandoned safely).
-->
<script lang="ts">
  import { m } from '../../paraglide/messages.js';
  import { HINT_MS } from '../../lib/device';
  import Lathes from './Lathes.svelte';

  let {
    label,
    elapsed,
    stuckMs,
    note = null,
    oncancel,
    onretry,
  }: { label: string; elapsed: number; stuckMs: number; note?: string | null; oncancel?: () => void; onretry?: () => void } = $props();

  const long = $derived(elapsed >= HINT_MS);
  const stuck = $derived(elapsed >= stuckMs && !!(oncancel || onretry));
</script>

<div class="waiting" role="status" aria-live="polite" data-testid="device-waiting" data-long={long} data-stuck={stuck}>
  <span class="mark"><Lathes size={26} kind="seal" /></span>
  <span class="text">
    <span class="label">{label}{#if long}<span class="secs num" data-testid="device-waiting-secs">{m.dev_busy_seconds({ s: Math.floor(elapsed / 1000) })}</span>{/if}</span>
    {#if stuck}
      <span class="hint slow" data-testid="device-waiting-slow">{m.dev_busy_slow()}</span>
    {:else if note}
      <span class="hint">{note}</span>
    {:else if long}
      <span class="hint" data-testid="device-waiting-hint">{m.dev_busy_hint()}</span>
    {/if}
  </span>
  {#if stuck}
    <span class="acts">
      {#if oncancel}<button type="button" class="btn ghost sm" onclick={oncancel} data-testid="device-waiting-cancel">{m.action_cancel()}</button>{/if}
      {#if onretry}<button type="button" class="btn sm" onclick={onretry} data-testid="device-waiting-retry">{m.dev_busy_retry()}</button>{/if}
    </span>
  {/if}
</div>

<style>
  .waiting {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    padding: 6px 12px 6px 8px;
    border-radius: var(--radius-pill);
    color: var(--ink);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
    animation: rise var(--dur-micro, 160ms) var(--ease-out, ease-out) both;
  }
  .waiting[data-stuck='true'] {
    border-radius: var(--radius-md);
  }
  .mark {
    display: grid;
    color: var(--accent);
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 1px;
  }
  .label {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-md);
    font-weight: 500;
  }
  .secs {
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    letter-spacing: 0.06em;
    color: var(--ink-3);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .slow {
    color: var(--ink-2);
  }
  .acts {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
    flex: none;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(3px);
    }
  }
</style>
