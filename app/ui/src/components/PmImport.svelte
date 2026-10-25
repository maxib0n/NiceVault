<!--
  Import wizard for the password manager: 1 the source (Bitwarden JSON / KeePass KDBX / CSV),
  2 the file (+ KeePass master password and optional key file), 3 the job's real progress
  (job-progress: done_bytes / total_bytes = entries) and its result (job-done: imported).
  Hooks: [data-testid=pm-import] data-step, [data-testid=pm-import-result].
-->
<script lang="ts">
  import { Check, Database, FileJson, FileSpreadsheet, FileUp, KeyRound } from '@lucide/svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import { onDestroy } from 'svelte';
  import * as api from '../lib/api';
  import type { PmImportDone, PmImportKind } from '../lib/api';
  import { errorText } from '../lib/errors';
  import { radioKeys } from '../lib/radio';
  import type { PmStore } from '../lib/stores/pm.svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';
  import PasswordField from './PasswordField.svelte';

  let { open, store, onclose, initialStep = 1 }: { open: boolean; store: PmStore; onclose: () => void; initialStep?: 1 | 2 } = $props();

  let step = $state<1 | 2 | 3>(1);
  let kind = $state<PmImportKind>('bitwarden');
  let path = $state<string | null>(null);
  let master = $state('');
  let keyfile = $state<string | null>(null);
  let job = $state<string | null>(null);
  let done = $state(0);
  let total = $state(0);
  let result = $state<{ imported: number; error: string | null } | null>(null);
  let error = $state<string | null>(null);
  let wasOpen = false;
  const offs: (() => void)[] = [];
  // events can arrive before pm_import has returned the job id: kept until it is known
  const early = new Map<string, { progress?: { done: number; total: number }; done?: PmImportDone }>();
  function finished(r: PmImportDone) {
    result = { imported: r.imported ?? 0, error: r.error ? errorText(r.error) : null };
    void store.load().then(() => store.refreshIcons());
  }

  $effect(() => {
    if (open && !wasOpen) {
      step = initialStep;
      path = initialStep === 2 ? `/home/utente/Scaricati/export.${EXT[kind][0]}` : null;
      master = '';
      keyfile = null;
      job = null;
      done = total = 0;
      result = null;
      error = null;
    }
    wasOpen = open;
  });

  const KINDS: { id: PmImportKind; label: () => string; name: () => string; note: () => string; icon: typeof FileJson }[] = [
    { id: 'bitwarden', label: m.imp_bitwarden, name: m.imp_k_bitwarden, note: m.imp_k_bitwarden_note, icon: FileJson },
    { id: 'keepass', label: m.imp_keepass, name: m.imp_k_keepass, note: m.imp_k_keepass_note, icon: Database },
    { id: 'csv', label: m.imp_csv, name: m.imp_k_csv, note: m.imp_k_csv_note, icon: FileSpreadsheet },
  ];
  const STEPS = [m.imp_step_format, m.imp_step_file, m.imp_step_import];
  const EXT: Record<PmImportKind, string[]> = { bitwarden: ['json'], keepass: ['kdbx'], csv: ['csv'] };

  async function chooseFile() {
    const p = await api.pickFile(m.imp_pick_title(), EXT[kind]);
    if (p) path = p;
  }
  async function chooseKey() {
    const p = await api.pickFile(m.imp_pick_key_title());
    if (p) keyfile = p;
  }

  async function start(e?: Event) {
    e?.preventDefault();
    if (!path || (kind === 'keepass' && !master && !keyfile)) return;
    error = null;
    step = 3;
    done = total = 0;
    try {
      if (!offs.length) {
        offs.push(
          await api.onJobProgress((p) => {
            if (job === null) return void early.set(p.job, { ...early.get(p.job), progress: { done: p.done_bytes, total: p.total_bytes } });
            if (p.job !== job) return;
            done = p.done_bytes;
            total = p.total_bytes;
          }),
          await api.onJobDone((d) => {
            const r = d as unknown as PmImportDone;
            if (job === null) return void early.set(d.job, { ...early.get(d.job), done: r });
            if (d.job === job) finished(r);
          }),
        );
      }
      job = null;
      const id = await api.pmImport(store.vault, kind, { path, master_password: kind === 'keepass' ? master || undefined : undefined, keyfile: kind === 'keepass' ? (keyfile ?? undefined) : undefined });
      job = id;
      const e = early.get(id);
      early.clear();
      if (e?.progress) ({ done, total } = e.progress);
      if (e?.done) finished(e.done);
    } catch (err) {
      error = errorText(err);
      step = 2;
    }
  }

  onDestroy(() => offs.forEach((f) => f()));
  const busy = $derived(step === 3 && !result);
  const canStart = $derived(!!path && (kind !== 'keepass' || !!master || !!keyfile));
</script>

<Dialog {open} title={m.imp_title()} variant="wide" onclose={() => !busy && onclose()}>
  <form class="imp" id="pm-import-form" data-testid="pm-import" data-step={step} onsubmit={step === 1 ? (e) => (e.preventDefault(), (step = 2)) : start}>
    <ol class="rail" aria-label={m.imp_title()}>
      {#each STEPS as label, i (i)}
        <li class="rail-step" data-state={i + 1 < step ? 'done' : i + 1 === step ? 'current' : 'todo'} aria-current={i + 1 === step ? 'step' : undefined}>
          <span class="rail-n">{#if i + 1 < step}<Check size={11} strokeWidth={2.6} aria-hidden="true" />{:else}{i + 1}{/if}</span>
          <span class="rail-label">{label()}</span>
        </li>
      {/each}
    </ol>
    {#if step === 1}
      <div class="kinds" role="radiogroup" aria-label={m.imp_format()} tabindex="-1" onkeydown={(e) => radioKeys(e, KINDS.map((k) => k.id), kind, (v) => (kind = v))}>
        {#each KINDS as k (k.id)}
          {@const Icon = k.icon}
          <!-- svelte-ignore a11y_autofocus -->
          <button type="button" role="radio" class="kind" aria-checked={kind === k.id} aria-label={k.label()} tabindex={kind === k.id ? 0 : -1} autofocus={kind === k.id} data-kind={k.id} onclick={() => (kind = k.id)}>
            <span class="kind-icon" aria-hidden="true"><Icon size={20} strokeWidth={1.5} /></span>
            <span class="kind-name">{k.name()}</span>
            <span class="kind-note">{k.note()}</span>
            <span class="dot" aria-hidden="true"></span>
          </button>
        {/each}
      </div>
      <p class="hint">{m.imp_note()}</p>
      {#if store.all.length > 0}
        <div class="repair">
          <button type="button" class="btn sm" onclick={() => store.repair()} data-testid="pm-repair">{m.pm_repair()}</button>
          <p class="hint">{m.pm_repair_hint()}</p>
        </div>
      {/if}
    {:else if step === 2}
      <div class="field">
        <span class="field-label" id="imp-file-label">{m.imp_file()}</span>
        <!-- the file slot: pressed into the page, the chosen file's path set in it -->
        <div class="slot" data-filled={!!path}>
          <span class="slot-icon" aria-hidden="true"><FileUp size={17} strokeWidth={1.5} /></span>
          <span class="file mono" class:none={!path} aria-labelledby="imp-file-label">{path ?? m.imp_no_file()}</span>
          <button type="button" class="btn sm" onclick={chooseFile} data-testid="imp-choose">{m.imp_choose_file()}</button>
        </div>
      </div>
      {#if kind === 'keepass'}
        <PasswordField bind:value={master} label={m.imp_master()} autocomplete="off" />
        <div class="field">
          <span class="field-label" id="imp-key-label">{m.imp_keyfile()}</span>
          <div class="slot" data-filled={!!keyfile}>
            <span class="slot-icon" aria-hidden="true"><KeyRound size={17} strokeWidth={1.5} /></span>
            <span class="file mono" class:none={!keyfile} aria-labelledby="imp-key-label">{keyfile ?? m.imp_no_file()}</span>
            <button type="button" class="btn sm" onclick={chooseKey}>{m.imp_choose_keyfile()}</button>
          </div>
        </div>
      {/if}
      {#if error}<p class="error-text" role="alert">{error}</p>{/if}
    {:else}
      <div class="run" aria-live="polite" data-result={result ? (result.error ? (result.imported > 0 ? 'partial' : 'failed') : 'done') : 'running'}>
        {#if result}
          <!-- the receipt: stamped when every entry came in -->
          {#if !result.error}<span class="stamp" aria-hidden="true"><Rosette size={72} kind="stamp" finish="flat"><Check size={20} strokeWidth={1.8} /></Rosette></span>{/if}
          <p class={result.error ? 'error-text' : 'done display'} data-testid="pm-import-result" data-imported={result.imported}>
            {#if result.error && result.imported > 0}
              {m.imp_partial({ count: result.imported, n: String(result.imported), error: result.error })}
            {:else if result.error}
              {m.imp_failed({ error: result.error })}
            {:else}
              {m.imp_done({ count: result.imported, n: String(result.imported) })}
            {/if}
          </p>
          {#if !result.error}<p class="hint">{m.imp_note_after()}</p>{/if}
        {:else}
          <p class="run-line">{total > 0 ? m.imp_running({ done: String(done), total: String(total) }) : m.imp_reading()}</p>
          <progress max={Math.max(1, total)} value={done} aria-label={m.imp_title()}></progress>
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
        <button type="submit" form="pm-import-form" class="btn primary" data-testid="imp-next">{m.create_next()}</button>
      {:else}
        <button type="submit" form="pm-import-form" class="btn primary" disabled={!canStart} data-testid="imp-start">{m.imp_start()}</button>
      {/if}
    {/if}
  {/snippet}
</Dialog>

<style>
  .imp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* where the import stands: three numbered steps, the current one in the ink */
  .rail {
    display: flex;
    flex-wrap: wrap;
    gap: 4px var(--space-4);
    margin: 0 0 var(--space-1);
    padding: 0;
    list-style: none;
  }
  .rail-step {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    font-weight: 500;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-4);
  }
  .rail-n {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    font-size: 10px;
    letter-spacing: 0;
    background: var(--paper-sunk);
    box-shadow: inset 0 1px 2px var(--field-shade);
  }
  .rail-step[data-state='current'] {
    color: var(--ink);
  }
  .rail-step[data-state='current'] .rail-n {
    color: var(--accent-ink);
    background: var(--accent);
    box-shadow: none;
  }
  .rail-step[data-state='done'] .rail-n {
    color: var(--accent);
  }

  /* the sources: three cards, each with its mark, its name and one line */
  .kinds {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: var(--space-2);
  }
  .kind {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    min-height: 116px;
    padding: 14px 14px 12px;
    border: 0;
    border-radius: var(--radius-lg);
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    color: var(--ink-2);
    text-align: left;
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1px var(--key-edge),
      var(--lift-key);
  }
  .kind:hover {
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1px var(--key-edge-lo),
      var(--lift-key-hover);
  }
  .kind:active {
    transform: translateY(1px);
  }
  .kind-icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    margin-bottom: 6px;
    border-radius: 50%;
    color: var(--ink-3);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .kind-name {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--ink);
  }
  .kind-note {
    font-size: var(--text-xs);
    line-height: 1.35;
    color: var(--ink-3);
  }
  .kind[aria-checked='true'] {
    background: linear-gradient(180deg, color-mix(in oklab, var(--accent-soft) 70%, var(--key-hi)), var(--accent-soft));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1.5px var(--accent),
      var(--lift-key);
  }
  .kind[aria-checked='true'] .kind-icon {
    color: var(--accent);
  }
  /* the radio's own mark, top right */
  .dot {
    position: absolute;
    top: 12px;
    right: 12px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--field);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      inset 0 0 0 1px var(--field-edge);
  }
  .kind[aria-checked='true'] .dot {
    background: var(--accent);
    box-shadow:
      inset 0 0 0 3.5px var(--paper-hi),
      inset 0 0 0 4.5px var(--accent);
  }
  .repair {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px solid var(--paper-rule);
  }

  /* the file slot */
  .slot {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 6px 6px 6px 12px;
    border-radius: var(--radius-md);
    background: var(--field);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      inset 0 0 0 1px var(--field-edge),
      0 1px 0 var(--paper-hi);
  }
  .slot[data-filled='false'] {
    background: repeating-linear-gradient(135deg, transparent 0 6px, color-mix(in srgb, var(--ink) 3%, transparent) 6px 7px), var(--field);
  }
  .slot-icon {
    display: inline-flex;
    flex: none;
    color: var(--ink-3);
  }
  .slot[data-filled='true'] .slot-icon {
    color: var(--accent);
  }
  .file {
    flex: 1;
    min-width: 0;
    font-size: var(--text-sm);
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .file.none {
    font-family: var(--font-ui);
    color: var(--ink-3);
  }

  /* the job, then its receipt */
  .run {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 96px;
    justify-content: center;
  }
  .run[data-result='done'] {
    align-items: center;
    text-align: center;
  }
  .run-line {
    font-size: var(--text-base);
    color: var(--ink-2);
  }
  .stamp {
    display: grid;
    color: var(--accent);
    transform: rotate(-8deg);
  }
  .done {
    font-size: var(--display-s);
    color: var(--title-ink);
  }
</style>
