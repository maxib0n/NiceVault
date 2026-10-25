<!-- Import/export/delete jobs with real progress from job-progress / job-done events. -->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Check, ChevronDown, ChevronUp, CircleAlert, Download, Trash2, Upload, X } from '@lucide/svelte';
  import { formatBytes } from '../lib/format';
  import { jobs, type Job } from '../lib/stores/jobs.svelte';
  import { etaParts, etaSeconds } from '../lib/jobSpeed';
  import { formatNumber } from '../lib/format';

  const visible = $derived(jobs.shown.length > 0);
  const pct = (j: Job) =>
    j.kind === 'delete'
      ? j.total_items > 0 ? Math.round((j.done_items / j.total_items) * 100) : 0
      : j.total_bytes > 0 ? Math.round((j.done_bytes / j.total_bytes) * 100) : 0;

  const summary = $derived.by(() => {
    const r = jobs.running;
    if (r.length === 0) {
      const failed = jobs.shown.filter((j) => j.status === 'error').length;
      return failed ? m.jobs_failed({ count: failed, n: String(failed) }) : m.jobs_done();
    }
    const kind = r.every((j) => j.kind === 'import')
      ? m.jobs_import()
      : r.every((j) => j.kind === 'export')
        ? m.jobs_export()
        : r.every((j) => j.kind === 'delete')
          ? m.jobs_delete()
          : m.jobs_transfer();
    return `${kind} · ${Math.round((jobs.progress ?? 0) * 100)}%`;
  });

  /** "12 MB/s · ~2 min": speed once there is one, the time left once it is stable */
  const pace = (j: Job): string => {
    const r = j.rate.rate;
    if (r === null) return '';
    const speed = j.kind === 'delete' ? m.job_rate_items({ n: formatNumber(Math.round(r), 0) }) : `${formatBytes(r)}/s`;
    const sec = etaSeconds(j.rate, j.kind === 'delete' ? j.total_items : j.total_bytes);
    if (sec === null) return speed;
    const e = etaParts(sec);
    const eta = e.unit === 's' ? m.job_eta_s({ n: String(e.n) }) : e.unit === 'min' ? m.job_eta_min({ n: String(e.n) }) : m.job_eta_h({ h: String(e.n), min: String(e.min) });
    return `${speed} · ${eta}`;
  };

  const STATUS: Record<Job['status'], () => string> = {
    running: () => '',
    done: m.job_done,
    error: m.job_error,
    cancelled: m.job_cancelled,
  };
</script>

{#if visible}
  <section class="job-tray" class:open={jobs.open} aria-label={m.jobs_label()}>
    <button type="button" class="tray-head" aria-expanded={jobs.open} aria-controls="job-list" onclick={() => (jobs.open = !jobs.open)}>
      <span class="tray-summary" role="status" aria-live="polite">{summary}</span>
      {#if jobs.progress !== null}
        <progress class="tray-progress" max="1" value={jobs.progress} aria-label={m.jobs_progress()}></progress>
      {/if}
      {#if jobs.open}<ChevronDown size={16} aria-hidden="true" />{:else}<ChevronUp size={16} aria-hidden="true" />{/if}
    </button>

    {#if jobs.open}
      <ul class="job-list" id="job-list">
        {#each jobs.shown as j (j.id)}
          <li class="job" data-status={j.status} data-kind={j.kind}>
            <span class="job-icon" aria-hidden="true">
              {#if j.kind === 'import'}<Upload size={14} />{:else if j.kind === 'delete'}<Trash2 size={14} />{:else}<Download size={14} />{/if}
            </span>
            <div class="job-main">
              <p class="job-title">
                <span class="job-label">{j.label}</span>
                {#if j.target}<span class="job-target">→ {j.target}</span>{/if}
              </p>
              {#if j.status === 'running'}
                <progress max="100" value={pct(j)} aria-label={m.job_progress({ label: j.label })}></progress>
                <p class="job-meta">
                  {#if j.kind === 'delete'}
                    <span class="num">{m.job_delete_meta({ done: String(j.done_items), total: String(j.total_items), freed: formatBytes(j.done_bytes) })}</span>
                  {:else}
                    <span class="num">{formatBytes(j.done_bytes)} / {formatBytes(j.total_bytes)}</span>
                  {/if}
                  {#if pace(j)}· <span class="num" data-testid="job-pace">{pace(j)}</span>{/if}
                  {#if j.current}· <span class="job-current">{j.current}</span>{/if}
                </p>
              {:else}
                <p class="job-meta" data-status={j.status}>
                  {#if j.status === 'done'}<Check size={13} aria-hidden="true" />{:else if j.status === 'error'}<CircleAlert size={13} aria-hidden="true" />{/if}
                  {j.error ?? STATUS[j.status]()}
                </p>
              {/if}
            </div>
            {#if j.status === 'running'}
              <button type="button" class="btn ghost icon" aria-label={m.job_cancel({ label: j.label })} onclick={() => jobs.cancel(j.id)}>
                <X size={14} />
              </button>
            {:else}
              <button type="button" class="btn ghost icon" aria-label={m.job_dismiss({ label: j.label })} onclick={() => jobs.dismiss(j.id)}>
                <X size={14} />
              </button>
            {/if}
          </li>
        {/each}
      </ul>
      {#if jobs.shown.some((j) => j.status !== 'running')}
        <button type="button" class="btn ghost clear" onclick={() => jobs.clearFinished()}>{m.jobs_clear()}</button>
      {/if}
    {/if}
  </section>
{/if}

<style>
  /* a manifest slip, bottom right, lifted off the page */
  .job-tray {
    position: fixed;
    right: var(--space-4);
    bottom: calc(var(--statusbar-h) + var(--space-3));
    z-index: var(--z-tray);
    width: min(360px, calc(100vw - 32px));
    border-radius: var(--radius-md);
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      var(--shadow-pop);
    overflow: hidden;
  }
  .tray-head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 46px;
    padding: 0 var(--space-3) 0 var(--space-4);
    border: 0;
    background: none;
    text-align: left;
    color: var(--ink);
  }
  .tray-head:hover {
    background: color-mix(in oklab, var(--ink) 4%, transparent);
  }
  .tray-head:active {
    background: color-mix(in oklab, var(--ink) 8%, transparent);
  }
  .tray-head:focus-visible {
    outline-offset: -3px;
    border-radius: var(--radius-md);
  }
  .tray-head :global(svg) {
    color: var(--ink-3);
  }
  .tray-summary {
    flex: none;
    font-weight: 600;
    font-size: var(--text-md);
  }
  .tray-progress {
    flex: 1;
    min-width: 0;
  }
  .job-list {
    list-style: none;
    margin: 0;
    padding: 2px var(--space-2) var(--space-2);
    max-height: 300px;
    overflow: auto;
    border-top: 1px solid var(--paper-rule);
  }
  .job {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 4px 10px var(--space-2);
  }
  .job + .job {
    border-top: 1px solid var(--paper-rule);
  }
  .job-icon {
    display: grid;
    place-items: center;
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    color: var(--ink-3);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .job[data-status='running'] .job-icon {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .job-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .job-title {
    font-size: var(--text-md);
    font-weight: 600;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
  .job-target {
    color: var(--ink-3);
    font-weight: 400;
  }
  .job-meta {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
    font-size: var(--text-sm);
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .job-meta .num {
    font-family: var(--font-mono);
    font-size: 11.5px;
    font-variant-numeric: normal;
  }
  .job-current {
    overflow-wrap: anywhere;
  }
  .job-meta[data-status='error'] {
    color: var(--danger);
  }
  .job-meta[data-status='done'] {
    color: var(--ink);
  }
  .job-meta[data-status='done'] :global(svg) {
    color: var(--accent);
  }
  .job .btn.icon {
    width: 28px;
    min-height: 28px;
  }
  .clear {
    width: 100%;
    min-height: 40px;
    border-top: 1px solid var(--paper-rule);
    border-radius: 0;
    font-size: var(--text-sm);
  }
  .clear:focus-visible {
    outline-offset: -3px;
  }
  .job-tray .clear.btn.ghost:hover:not(:disabled) {
    --edge: transparent;
    --edge-lo: transparent;
    --lift: 0 0 0 transparent;
    --lip: transparent;
    --face-hi: var(--paper-sunk);
    --face-lo: var(--paper-sunk);
  }
</style>
