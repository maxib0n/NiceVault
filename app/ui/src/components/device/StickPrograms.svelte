<!--
  "Programmi sulla chiavetta" (Dettagli > Volume): shown only when the active volume lives on a
  NiceVault device (device_sticks { path }). One ledger line per OS — the version on the stick,
  an arrow, the version it would get (this app's for its own OS; the latest, signed, for the
  others) — then "Aggiorna i programmi sulla chiavetta". During the update each line says what
  is happening to it; afterwards what happened (aggiornato · già aggiornato · da scaricare quando
  online · non aggiornato). Not mounted: says so and offers "Monta NiceVault".
  Set like FormatUpgrade beside it: a dashed rule over it, a small seal on the left, the keys on
  the right. Hooks: [data-testid=stick-programs][data-state], [data-os].
-->
<script lang="ts">
  import { Usb } from '@lucide/svelte';
  import type { OsId } from '../../lib/device';
  import { fraction, OS_NAMES, type StickOs, type StickOsResult } from '../../lib/stick';
  import { sticks } from '../../lib/stores/stick.svelte';
  import { vaults } from '../../lib/stores/vaults.svelte';
  import { m } from '../../paraglide/messages.js';
  import StickSpace from './StickSpace.svelte';

  const path = $derived(vaults.active?.path ?? null);
  $effect(() => {
    if (path) void sticks.load(path);
  });

  const stick = $derived(path ? sticks.forPath(path) : null);
  const run = $derived(stick ? (sticks.runs[stick.disk_id] ?? null) : null);
  const running = $derived(run?.phase === 'running');
  const state = $derived(
    !stick ? 'none' : running ? 'running' : run?.phase === 'error' ? 'error' : run?.phase === 'done' ? 'done' : !stick.mounted ? 'unmounted' : !stick.complete ? 'incomplete' : stick.outdated ? 'outdated' : 'current',
  );
  const pct = $derived.by(() => {
    const f = run ? fraction(run) : null;
    return f === null ? null : Math.round(f * 100);
  });
  const partial = $derived(run?.phase === 'done' && run.results.some((r) => r.result === 'offline' || r.result === 'failed' || r.result === 'rewrite'));

  /** the programs as the lines show them: during / after a run, the versions from before it */
  const lines = $derived<StickOs[]>(run && run.phase !== 'error' ? run.before.programs : (stick?.programs ?? []));

  const resultOf = (os: OsId): StickOsResult | null => run?.results.find((r) => r.os === os) ?? null;
  const from = (p: StickOs) => (p.present ? (p.version ?? m.stk_unknown()) : m.stk_missing());
  const to = (p: StickOs) => {
    const r = resultOf(p.os);
    if (r?.result === 'updated' && r.to) return r.to;
    return p.target ?? m.stk_latest();
  };
  function lineState(p: StickOs): string {
    const r = resultOf(p.os);
    if (r) {
      if (r.result === 'updated') return r.mode === 'tight' ? m.stk_r_updated_tight() : m.stk_r_updated();
      if (r.result === 'rewrite') return m.stk_r_rewrite();
      if (r.result === 'in_use') return m.stk_r_in_use();
      if (r.result === 'unchanged') return m.stk_r_unchanged();
      if (r.result === 'offline') return m.stk_r_offline();
      return m.stk_r_failed({ why: r.error?.message ?? '' });
    }
    if (running) return run?.os === p.os ? m.stk_r_working() : m.stk_r_pending();
    if (p.current) return p.source === 'running' && stick?.runs_from_stick ? m.stk_r_unchanged() : '';
    return p.source === 'running' ? m.stk_from_app() : m.stk_from_net();
  }
  const stepLabel = $derived.by(() => {
    if (!run?.step) return m.stk_updating();
    const s = { check: m.stk_s_check, download: m.stk_s_download, copy: m.stk_s_copy, verify: m.stk_s_verify, marker: m.stk_s_marker }[run.step]();
    return run.os && (run.step === 'copy' || run.step === 'verify' || run.step === 'download') ? `${s} · ${OS_NAMES[run.os]}` : s;
  });
</script>

{#if stick}
  <div class="stick" data-testid="stick-programs" data-state={state}>
    <span class="mark" data-ok={state === 'current' || (state === 'done' && !partial)} aria-hidden="true"><Usb size={15} strokeWidth={1.6} /></span>
    <h3 class="sub-title caps">{m.stk_title()}</h3>
    <p class="note">
      {#if state === 'unmounted'}{m.stk_not_mounted()}
      {:else if state === 'incomplete'}{m.stk_incomplete()}
      {:else if state === 'running'}{stepLabel}
      {:else if state === 'done'}{partial ? m.stk_done_partial() : m.stk_done()}
      {:else if state === 'outdated'}{m.stk_outdated()}
      {:else if stick.outdated}{m.stk_outdated()}
      {:else}{m.stk_current()}{/if}
    </p>

    <div class="actions">
      {#if state === 'unmounted'}
        <button type="button" class="btn sm" disabled={sticks.mounting[stick.disk_id]} onclick={() => sticks.mount(stick.disk_id)} data-testid="stick-mount">
          {sticks.mounting[stick.disk_id] ? m.stk_mounting() : m.stk_mount()}
        </button>
      {:else if state !== 'incomplete'}
        <button
          type="button"
          class="btn sm"
          class:primary={state === 'outdated'}
          disabled={running}
          onclick={() => {
            const s = stick;
            sticks.clear(s.disk_id);
            void sticks.update(s);
          }}
          data-testid="stick-update">{running ? m.stk_updating() : m.stk_update()}</button
        >
      {/if}
    </div>

    {#if stick.mounted && stick.complete}
      <p class="device">{m.stk_device({ name: stick.name || m.dev_unnamed() })}</p>
      <ul class="ledger" aria-label={m.stk_title()}>
        {#each lines as p (p.os)}
          {@const r = resultOf(p.os)}
          <li data-os={p.os} data-current={p.current} data-result={r?.result ?? (running && run?.os === p.os ? 'working' : null)} aria-label={m.stk_line({ os: OS_NAMES[p.os], from: from(p), to: to(p) })}>
            <span class="os">{OS_NAMES[p.os]}</span>
            <span class="ver mono" data-missing={!p.present}>{from(p)}</span>
            {#if !p.current || r}
              <span class="arrow" aria-hidden="true">→</span>
              <span class="ver mono new" data-word={!(resultOf(p.os)?.result === 'updated' && resultOf(p.os)?.to) && !p.target}>{to(p)}</span>
            {/if}
            <span class="state">{lineState(p)}</span>
          </li>
        {/each}
      </ul>
      {#if running}
        <progress class="bar" max="100" value={pct ?? undefined} aria-label={stepLabel}></progress>
      {/if}
      <StickSpace {stick} {run} />
      {#if stick.runs_from_stick}<p class="hint-line">{m.stk_runs_here()}</p>{/if}
      <p class="hint-line">{m.stk_safe_note()}</p>
    {/if}
    {#if state === 'error' && run}
      <p class="warn nv-enter" role="alert">{m.stk_failed({ why: run.error ?? '' })} {m.stk_failed_safe()}</p>
    {/if}
    {#if sticks.mountError[stick.disk_id]}
      <p class="warn nv-enter" role="alert">{sticks.mountError[stick.disk_id]}</p>
    {/if}
  </div>
{/if}

<style>
  .stick {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    column-gap: var(--space-3);
    row-gap: 4px;
    align-items: center;
    margin-top: var(--space-4);
    padding-top: var(--space-3);
    border-top: 1px dashed var(--paper-rule-2);
  }
  /* the small engraved seal of the row (as FormatUpgrade's): quiet, in ink when all is current */
  .mark {
    grid-row: span 2;
    align-self: start;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    color: var(--ink-3);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .mark[data-ok='true'] {
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  .sub-title {
    grid-column: 2;
    margin: 0;
    color: var(--ink-3);
  }
  .note {
    grid-column: 2;
    margin: 0;
    max-width: 46ch;
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .actions {
    grid-column: 3;
    grid-row: 1 / span 2;
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }
  .device,
  .hint-line {
    font-size: var(--text-xs);
  }
  .device {
    grid-column: 2 / -1;
    margin: var(--space-2) 0 0;
    color: var(--ink-3);
  }
  /* the ledger: one ruled line per system, versions in the mono like figures in a register */
  .ledger {
    grid-column: 2 / -1;
    list-style: none;
    margin: 2px 0 0;
    padding: 0;
  }
  .ledger li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 8px;
    padding: 5px 0;
    border-bottom: 1px dotted var(--paper-rule);
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .ledger li:last-child {
    border-bottom: 0;
  }
  .os {
    flex: none;
    width: 6.5em;
    font-weight: 500;
    color: var(--ink);
  }
  .ver {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--ink-2);
  }
  .ver[data-missing='true'] {
    font-family: inherit;
    font-style: italic;
    color: var(--ink-3);
  }
  .ver.new {
    padding: 0 5px;
    border-radius: var(--radius-sm);
    color: var(--accent);
    background: var(--accent-soft);
  }
  /* "ultima versione": words, not a figure */
  .ver.new[data-word='true'] {
    font-family: inherit;
    font-size: 12px;
    font-style: italic;
  }
  .arrow {
    color: var(--ink-3);
  }
  .state {
    margin-left: auto;
    text-align: right;
    min-width: 0;
    font-size: 12px;
    color: var(--ink-3);
  }
  li[data-result='updated'] .state {
    color: var(--accent);
    font-weight: 500;
  }
  li[data-result='failed'] .state {
    color: var(--danger);
  }
  li[data-result='working'] .state {
    color: var(--ink);
  }
  .bar {
    grid-column: 2 / -1;
    width: 100%;
    margin-top: var(--space-2);
  }
  .hint-line {
    grid-column: 2 / -1;
    margin: 2px 0 0;
    color: var(--ink-3);
  }
  .warn {
    grid-column: 2 / -1;
    margin: var(--space-2) 0 0;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--danger-wash);
    color: var(--ink);
    font-size: var(--text-sm);
    font-weight: 500;
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateY(3px);
    --enter-to: translateY(0);
  }
</style>
