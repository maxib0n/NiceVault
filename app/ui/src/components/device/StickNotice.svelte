<script lang="ts">
  import { Usb } from '@lucide/svelte';
  import { fraction, needsUpdate, OS_NAMES, stale, type Stick } from '../../lib/stick';
  import { sticks } from '../../lib/stores/stick.svelte';
  import { vaults } from '../../lib/stores/vaults.svelte';
  import { m } from '../../paraglide/messages.js';

  let { on = 'paper' }: { on?: 'paper' | 'cloth' } = $props();

  const path = $derived(on === 'cloth' ? (vaults.active?.path ?? null) : null);
  $effect(() => {
    if (on === 'paper' || path) void sticks.load(path);
  });

  const candidates = $derived<Stick[]>(on === 'cloth' ? (path ? [sticks.forPath(path)].filter((s): s is Stick => !!s) : []) : sticks.all());
  const stick = $derived(
    candidates.find((s) => sticks.runs[s.disk_id] && !sticks.dismissed[s.disk_id]) ?? candidates.find((s) => needsUpdate(s) && !sticks.dismissed[s.disk_id]) ?? null,
  );
  const run = $derived(stick ? (sticks.runs[stick.disk_id] ?? null) : null);
  const phase = $derived(run?.phase ?? 'available');
  const pct = $derived.by(() => {
    const f = run ? fraction(run) : null;
    return f === null ? null : Math.round(f * 100);
  });
  const partial = $derived(run?.phase === 'done' && run.results.some((r) => r.result === 'offline' || r.result === 'failed'));
  const summary = $derived.by(() => {
    if (!stick) return '';
    const s = stale(run?.before ?? stick);
    const shown = on === 'cloth' ? 1 : 2;
    const first = s.slice(0, shown).map((p) => `${OS_NAMES[p.os]} ${p.present ? (p.version ?? m.stk_unknown()) : m.stk_missing()} → ${p.target ?? m.stk_latest()}`);
    return s.length > shown ? `${first.join(' · ')} ${m.stk_notice_more({ n: String(s.length - shown) })}` : first.join(' · ');
  });
  function start(s: Stick) {
    sticks.clear(s.disk_id);
    void sticks.update(s);
  }
  function putAway(id: string) {
    sticks.dismiss(id);
    sticks.clear(id);
  }
</script>

{#if stick}
  <section class="stick-notice nv-enter" data-on={on} data-phase={phase} aria-live="polite" aria-label={m.stk_title()} data-testid="stick-notice">
    <span class="mark" aria-hidden="true"><Usb size={16} strokeWidth={1.6} /></span>
    <div class="text">
      <p class="title">{phase === 'done' ? (partial ? m.stk_done_partial() : m.stk_done()) : m.stk_notice_title()}</p>
      {#if phase === 'available'}
        <p class="status">{stick.name || m.dev_unnamed()} · <span class="mono">{summary}</span></p>
      {:else if phase === 'running'}
        <progress max="100" value={pct ?? undefined} aria-label={m.stk_updating()}></progress>
      {:else if phase === 'error'}
        <p class="status error" role="alert">{m.stk_failed({ why: run?.error ?? '' })}</p>
      {/if}
      <div class="actions">
        {#if phase === 'available' || phase === 'error'}
          <button type="button" class="btn sm primary" onclick={() => start(stick)} title={m.stk_update()} data-testid="stick-notice-update">{on === 'cloth' ? m.stk_update_short() : m.stk_update()}</button>
        {/if}
        {#if phase !== 'running'}
          <button type="button" class="btn sm ghost" onclick={() => putAway(stick.disk_id)}>{phase === 'done' ? m.action_close() : m.stk_not_now()}</button>
        {/if}
      </div>
    </div>
  </section>
{/if}

<style>
  .stick-notice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    font-size: var(--text-sm);
    color: var(--ink-2);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateY(4px);
    --enter-to: translateY(0);
  }
  .stick-notice[data-on='paper'] {
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
  .stick-notice[data-on='cloth'] {
    margin-top: var(--space-2);
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
  .status .mono {
    font-size: 11px;
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
