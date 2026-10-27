<script lang="ts">
  import * as api from '../lib/api';
  import { errorCode, errorText } from '../lib/errors';
  import { formatBytes } from '../lib/format';
  import { radioKeys } from '../lib/radio';
  import { fromBytes, plan, toBytes, unitFor, UNIT_BYTES, type ResizePlan, type SizeUnit } from '../lib/resize';
  import type { ResizeInfo, ResizeProgress, VaultId } from '../lib/types';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';

  let {
    open,
    vault,
    onclose,
    onresized,
    suggest,
  }: {
    open: boolean;
    vault: VaultId;
    onclose: () => void;
    onresized?: (info: ResizeInfo) => void;
    suggest?: number;
  } = $props();

  type Stage = 'loading' | 'edit' | 'running' | 'done' | 'error';
  let stage = $state<Stage>('loading');
  let info = $state<ResizeInfo | null>(null);
  let unit = $state<SizeUnit>('GB');
  let value = $state(0);
  let progress = $state<ResizeProgress | null>(null);
  let error = $state<string | null>(null);
  let bounce = $state(false);

  $effect(() => {
    if (open) void load();
  });

  async function load() {
    stage = 'loading';
    error = null;
    progress = null;
    try {
      const i = await api.vaultResizeInfo(vault);
      info = i;
      const cap = suggest ?? i.capacity ?? i.file_bytes;
      unit = unitFor(cap);
      value = suggest ? Math.ceil((cap / UNIT_BYTES[unit]) * 100) / 100 : fromBytes(cap, unit);
      stage = 'edit';
    } catch (e) {
      error = errorText(e);
      stage = 'error';
    }
  }

  const target = $derived(toBytes(value, unit));
  const what = $derived<ResizePlan>(info ? plan(info, target) : 'invalid');
  const canSubmit = $derived(stage === 'edit' && (what === 'grow' || what === 'shrink' || what === 'compact'));
  const STEP = 1024 * 1024;

  function planText(p: ResizePlan, i: ResizeInfo): string {
    const cap = i.capacity ?? 0;
    const f = formatBytes;
    switch (p) {
      case 'growable':
        return m.rsz_growable();
      case 'invalid':
        return m.rsz_plan_invalid();
      case 'same':
        return m.rsz_plan_same();
      case 'grow':
        return m.rsz_plan_grow({ delta: f(target - cap), free: f(i.drive_free) });
      case 'shrink':
        return m.rsz_plan_shrink({ delta: f(cap - target) });
      case 'compact':
        return m.rsz_plan_compact({ inplace: f(i.min_in_place), size: f(target), free: f(i.drive_free) });
      case 'too_small':
        return m.rsz_plan_too_small({ min: f(i.min_capacity) });
      case 'too_large':
        return m.rsz_plan_too_large({ max: f(i.max_capacity) });
      case 'no_space_compact':
        return m.rsz_plan_no_space_compact({ inplace: f(i.min_in_place), size: f(target), free: f(i.drive_free) });
    }
  }

  const PHASE: Record<ResizeProgress['phase'], () => string> = {
    compact: m.rsz_phase_compact,
    extend: m.rsz_phase_extend,
    commit: m.rsz_phase_commit,
    truncate: m.rsz_phase_truncate,
  };
  const fraction = $derived(progress?.phase === 'compact' && progress.total > 0 ? Math.min(1, progress.done / progress.total) : null);

  function setUnit(u: SizeUnit) {
    if (u === unit) return;
    const bytes = target;
    unit = u;
    if (Number.isFinite(bytes)) value = fromBytes(bytes, u);
  }

  function slide(e: Event) {
    value = fromBytes(Number((e.currentTarget as HTMLInputElement).value), unit);
  }

  async function submit(e?: Event) {
    e?.preventDefault();
    if (!canSubmit) return;
    stage = 'running';
    progress = null;
    const unlisten = await api.onVaultResize((p) => {
      if (p.vault === vault) progress = p;
    });
    try {
      const after = await api.vaultResize(vault, target);
      info = after;
      stage = 'done';
      onresized?.(after);
    } catch (err) {
      const code = errorCode(err);
      error =
        code === 'too_small'
          ? m.rsz_err_too_small()
          : code === 'no_space'
            ? m.rsz_err_no_space()
            : code === 'out_of_range'
              ? m.rsz_err_out_of_range()
              : code === 'not_fixed'
                ? m.rsz_growable()
                : errorText(err);
      stage = 'error';
    } finally {
      unlisten();
    }
  }

  function close() {
    if (stage === 'running') {
      bounce = true;
      setTimeout(() => (bounce = false), 0);
      return;
    }
    onclose();
  }
</script>

<Dialog open={open && !bounce} title={m.rsz_title()} onclose={close} describedby="resize-plan">
  <form class="resize" id="resize-form" onsubmit={submit} data-testid="resize-dialog" data-stage={stage} aria-busy={stage === 'loading' || stage === 'running'}>
    {#if info}
      <dl class="facts">
        {#if info.capacity !== null}
          <div><dt class="caps">{m.rsz_capacity()}</dt><dd class="num" data-testid="resize-capacity">{formatBytes(info.capacity)}</dd></div>
        {:else}
          <div><dt class="caps">{m.rsz_file()}</dt><dd class="num" data-testid="resize-capacity">{formatBytes(info.file_bytes)}</dd></div>
        {/if}
        <div><dt class="caps">{m.rsz_used()}</dt><dd class="num">{formatBytes(info.used_bytes)}</dd></div>
        <div><dt class="caps">{m.rsz_drive_free()}</dt><dd class="num">{formatBytes(info.drive_free)}</dd></div>
      </dl>
    {/if}

    {#if stage === 'edit' && info && info.capacity === null}
      <p class="hint" id="resize-plan" data-testid="resize-growable">{m.rsz_growable()}</p>
    {:else if stage === 'edit' && info}
      <div class="field">
        <label for="resize-size">{m.rsz_new_size()}</label>
        <div class="size-row">
          <input
            id="resize-size"
            class="input num"
            type="number"
            inputmode="decimal"
            min="0"
            step="any"
            bind:value
            aria-invalid={what === 'too_small' || what === 'too_large' || what === 'no_space_compact' || what === 'invalid'}
            aria-describedby="resize-plan resize-range"
            data-testid="resize-size"
          />
          <div class="segmented unit" role="radiogroup" aria-label={m.rsz_unit()} tabindex="-1" data-testid="resize-unit" onkeydown={(e) => radioKeys(e, Object.keys(UNIT_BYTES) as SizeUnit[], unit, (v) => setUnit(v))}>
            {#each Object.keys(UNIT_BYTES) as SizeUnit[] as u (u)}
              <button type="button" role="radio" aria-checked={unit === u} tabindex={unit === u ? 0 : -1} onclick={() => setUnit(u)}>{u}</button>
            {/each}
          </div>
          <button type="button" class="btn sm" onclick={() => (value = fromBytes(info!.min_capacity, unit))} disabled={info.min_capacity >= (info.capacity ?? 0)}>{m.rsz_to_min()}</button>
        </div>
        <input
          class="slider"
          type="range"
          min={info.min_capacity}
          max={info.max_capacity}
          step={STEP}
          value={Number.isFinite(target) ? Math.min(Math.max(target, info.min_capacity), info.max_capacity) : info.capacity}
          oninput={slide}
          style:--fill="{info.max_capacity > info.min_capacity ? ((Math.min(Math.max(Number.isFinite(target) ? target : (info.capacity ?? info.min_capacity), info.min_capacity), info.max_capacity) - info.min_capacity) / (info.max_capacity - info.min_capacity)) * 100 : 0}%"
          aria-label={m.rsz_slider()}
          aria-valuetext={Number.isFinite(target) ? formatBytes(target) : undefined}
          disabled={info.min_capacity >= info.max_capacity}
        />
        <p class="hint" id="resize-range">{m.rsz_range({ min: formatBytes(info.min_capacity), max: formatBytes(info.max_capacity) })}</p>
      </div>
      <p class={['plan', !canSubmit && what !== 'same' && 'error-text']} id="resize-plan" data-testid="resize-plan" data-plan={what} role="status">{planText(what, info)}</p>
    {:else if stage === 'running'}
      <div class="running" data-testid="resize-progress">
        <p id="resize-plan" role="status" data-phase={progress?.phase ?? ''}>{progress ? PHASE[progress.phase]() : m.rsz_phase_commit()}</p>
        {#if fraction !== null}
          <progress max="1" value={fraction} aria-label={m.rsz_progress()}></progress>
        {:else}
          <progress aria-label={m.rsz_progress()}></progress>
        {/if}
        <p class="hint">{m.rsz_wait_note()}</p>
      </div>
    {:else if stage === 'done' && info}
      <p id="resize-plan" role="status" data-testid="resize-done">{m.rsz_done({ size: formatBytes(info.capacity ?? info.file_bytes) })}</p>
    {:else if stage === 'error'}
      <div role="alert" data-testid="resize-error">
        <p class="error-text" id="resize-plan">{m.rsz_failed()} {error}</p>
        <p class="hint">{m.rsz_failed_safe()}</p>
      </div>
    {:else}
      <p class="hint" id="resize-plan">…</p>
    {/if}
  </form>
  {#snippet actions()}
    {#if stage === 'edit' && info?.capacity !== null && info}
      <button type="button" class="btn" onclick={close}>{m.action_cancel()}</button>
      <button type="submit" form="resize-form" class="btn primary" disabled={!canSubmit} data-testid="resize-submit">
        {Number.isFinite(target) && target < (info.capacity ?? 0) ? m.rsz_shrink() : m.rsz_extend()}
      </button>
    {:else if stage === 'error'}
      <button type="button" class="btn" onclick={close}>{m.action_close()}</button>
      <button type="button" class="btn primary" onclick={() => load()}>{m.rsz_retry()}</button>
    {:else}
      <button type="button" class="btn" onclick={close} disabled={stage === 'running'}>{m.action_close()}</button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .resize {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .facts {
    display: grid;
    grid-template-columns: repeat(3, auto);
    justify-content: start;
    gap: var(--space-3) var(--space-6);
    margin: 0;
  }
  .facts dt {
    white-space: nowrap;
  }
  .facts dd {
    margin: 2px 0 0;
    color: var(--ink);
    font-size: var(--text-md);
  }
  .size-row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .size-row .input.num {
    flex: 1;
    min-width: 0;
  }
  .size-row .unit {
    flex: none;
  }
  .slider {
    width: 100%;
    margin-top: var(--space-2);
  }
  .plan {
    margin: 0;
  }
  .running {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .running progress {
    width: 100%;
  }
  @media (max-width: 420px) {
    .facts {
      grid-template-columns: auto auto;
    }
  }
</style>
