<script lang="ts">
  import { KeyRound, RotateCcwKey, TriangleAlert } from '@lucide/svelte';
  import { onDestroy } from 'svelte';
  import * as api from '../lib/api';
  import { errorCode, errorText } from '../lib/errors';
  import { formatBytes } from '../lib/format';
  import { radioKeys } from '../lib/radio';
  import type { SlotInfo, VaultId, VaultStats } from '../lib/types';
  import { m } from '../paraglide/messages.js';
  import Rosette from '../ornament/Rosette.svelte';
  import Dialog from './Dialog.svelte';
  import PasswordField from './PasswordField.svelte';

  let {
    open,
    vault,
    slots,
    stats,
    onclose,
    onrotated,
  }: {
    open: boolean;
    vault: VaultId;
    slots: SlotInfo[];
    stats: VaultStats | null;
    onclose: () => void;
    onrotated: (slots: SlotInfo[]) => void;
  } = $props();

  let step = $state<1 | 2 | 3>(1);
  let keep = $state<number | null>(null);
  let password = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);
  let done = $state<{ kept: string; dropped: number } | null>(null);
  let startedAt = $state(0);
  let now = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;
  let wasOpen = false;

  const keepable = $derived(slots.filter((s) => s.kind === 'password'));
  const nameOf = (s: SlotInfo | undefined) => s?.label || m.slot_password();

  $effect(() => {
    if (open && !wasOpen) {
      step = 1;
      keep = (keepable.find((s) => s.in_use) ?? keepable[0])?.index ?? null;
      password = '';
      error = null;
      busy = false;
      done = null;
    }
    wasOpen = open;
  });

  const need = $derived(stats?.file_size_bytes ?? 0);
  const free = $derived(stats?.free_bytes);
  const roomy = $derived(free === undefined || free >= need);

  async function run(e?: Event) {
    e?.preventDefault();
    if (keep === null || !password || busy) return;
    const kept = nameOf(slots.find((s) => s.index === keep));
    const before = slots.length;
    busy = true;
    error = null;
    step = 3;
    startedAt = now = Date.now();
    timer = setInterval(() => (now = Date.now()), 500);
    try {
      const after = await api.slotRotateKey(vault, keep, password);
      password = '';
      done = { kept, dropped: Math.max(0, before - after.length) };
      onrotated(after);
    } catch (err) {
      step = 2;
      error = errorCode(err) === 'wrong_password' ? m.err_wrong_password() : errorText(err);
    } finally {
      busy = false;
      clearInterval(timer);
    }
  }
  onDestroy(() => clearInterval(timer));

  const elapsed = $derived.by(() => {
    const s = Math.max(0, Math.floor((now - startedAt) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  });
</script>

<Dialog {open} title={m.rot_title()} variant={step === 3 ? 'default' : 'danger'} onclose={() => !busy && onclose()} describedby="rot-what">
  <form class="rot" id="rotate-form" data-testid="rotate-key" data-step={step} onsubmit={step === 1 ? (e) => (e.preventDefault(), (step = 2)) : run}>
    {#if step === 1}
      <p id="rot-what">{m.rot_what()}</p>
      <ul class="terms">
        <li data-ok={roomy}>
          <span class="t-mark" aria-hidden="true">{#if roomy}<KeyRound size={14} />{:else}<TriangleAlert size={14} />{/if}</span>
          <span>{free === undefined ? m.rot_space_unknown({ need: formatBytes(need) }) : m.rot_space({ need: formatBytes(need), free: formatBytes(free) })}</span>
        </li>
        <li>
          <span class="t-mark warn" aria-hidden="true"><TriangleAlert size={14} /></span>
          <span>{m.rot_keeps()}</span>
        </li>
      </ul>
      {#if !roomy}<p class="error-text" role="alert">{m.rot_no_space()}</p>{/if}
    {:else if step === 2}
      <p id="rot-what" class="hint">{m.rot_keeps()}</p>
      {#if keepable.length > 1}
        <div class="field">
          <span class="field-label" id="rot-keep">{m.rot_keep()}</span>
          <div class="keeps" role="radiogroup" aria-labelledby="rot-keep" tabindex="-1" onkeydown={(e) => radioKeys(e, keepable.map((s) => s.index), keep ?? keepable[0].index, (v) => (keep = v))}>
            {#each keepable as s (s.index)}
              <button type="button" role="radio" class="keep" aria-checked={keep === s.index} tabindex={keep === s.index ? 0 : -1} onclick={() => (keep = s.index)}>
                <span class="dot" aria-hidden="true"></span>
                <span class="k-name">{nameOf(s)}</span>
                {#if s.in_use}<span class="badge">{m.det_in_use()}</span>{/if}
              </button>
            {/each}
          </div>
        </div>
      {/if}
      <PasswordField bind:value={password} label={m.rot_password({ name: nameOf(slots.find((s) => s.index === keep)) })} autocomplete="current-password" autofocus invalid={!!error} />
      {#if error}<p class="error-text" role="alert">{error}</p>{/if}
    {:else}
      <div class="work" aria-live="polite" data-done={!!done}>
        <span class="seal" class:turning={busy} aria-hidden="true"><Rosette size={92} kind="seal"><RotateCcwKey size={24} strokeWidth={1.4} /></Rosette></span>
        {#if done}
          <p class="done display" data-testid="rotate-done">{m.rot_done()}</p>
          <p class="hint">{m.rot_done_kept({ name: done.kept })} {#if done.dropped > 0}{m.rot_done_dropped({ count: done.dropped, n: String(done.dropped) })}{/if}</p>
        {:else}
          <p class="run-line">{m.rot_running()} <span class="mono elapsed">{elapsed}</span></p>
          <p class="hint">{m.rot_running_hint()}</p>
        {/if}
      </div>
    {/if}
  </form>
  {#snippet actions()}
    {#if step === 3}
      <button type="button" class="btn primary" onclick={onclose} disabled={busy}>{m.action_close()}</button>
    {:else}
      <button type="button" class="btn" onclick={() => (step === 2 ? (step = 1) : onclose())}>{step === 2 ? m.action_back() : m.action_cancel()}</button>
      {#if step === 1}
        <button type="submit" form="rotate-form" class="btn danger" disabled={!roomy || keepable.length === 0} data-testid="rotate-next">{m.duress_continue()}</button>
      {:else}
        <button type="submit" form="rotate-form" class="btn danger" disabled={!password || keep === null} data-testid="rotate-confirm">{m.rot_confirm()}</button>
      {/if}
    {/if}
  {/snippet}
</Dialog>

<style>
  .rot {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .terms {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .terms li {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 9px 12px;
    border-radius: var(--radius-md);
    color: var(--ink);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .terms li[data-ok='false'] {
    background: var(--danger-wash);
  }
  .t-mark {
    display: inline-flex;
    flex: none;
    margin-top: 2px;
    color: var(--accent);
  }
  .t-mark.warn,
  .terms li[data-ok='false'] .t-mark {
    color: var(--danger);
  }
  .keeps {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .keep {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 38px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    color: var(--ink-2);
    text-align: left;
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1px var(--key-edge);
  }
  .keep[aria-checked='true'] {
    color: var(--ink);
    font-weight: 600;
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1.5px var(--accent);
  }
  .dot {
    flex: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--field);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      inset 0 0 0 1px var(--field-edge);
  }
  .keep[aria-checked='true'] .dot {
    background: var(--accent);
    box-shadow:
      inset 0 0 0 3.5px var(--paper-hi),
      inset 0 0 0 4.5px var(--accent);
  }
  .k-name {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .work {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) 0;
    text-align: center;
  }
  .seal {
    display: grid;
    margin-bottom: var(--space-2);
    color: var(--accent);
  }
  .seal.turning :global(.lathe-a) {
    animation: lathe-a 4.8s linear infinite;
  }
  .seal.turning :global(.lathe-b) {
    animation: lathe-b 6.4s linear infinite;
  }
  @keyframes lathe-a {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes lathe-b {
    to {
      transform: rotate(-360deg);
    }
  }
  .run-line {
    font-size: var(--text-base);
    color: var(--ink);
  }
  .elapsed {
    margin-left: 6px;
    color: var(--ink-3);
  }
  .done {
    font-size: var(--display-s);
    color: var(--title-ink);
  }
</style>
