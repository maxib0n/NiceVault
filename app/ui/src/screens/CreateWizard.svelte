<!-- Nuovo volume: 1 Nome e posizione (drive picker) → 2 Chiave → 3 Fatto (+ emblema). Enter advances, Esc goes back. -->
<script lang="ts">
  import { ArrowLeft, Check } from '@lucide/svelte';
  import { m } from '../paraglide/messages.js';
  import { onMount } from 'svelte';
  import * as api from '../lib/api';
  import { errorText } from '../lib/errors';
  import { preloadStrength, type Strength } from '../lib/strength';
  import { switches } from '../lib/switches';
  import { app } from '../lib/stores/app.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import type { VaultInfo } from '../lib/types';
  import DrivePicker from '../components/DrivePicker.svelte';
  import Emblem from '../components/Emblem.svelte';
  import EmblemPicker from '../components/EmblemPicker.svelte';
  import PasswordField from '../components/PasswordField.svelte';
  import Brand from '../components/Brand.svelte';
  import Band from '../ornament/Band.svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import Stitch from '../ornament/Stitch.svelte';
  import StrengthMeter from '../components/StrengthMeter.svelte';

  const STEPS = [m.create_step_1, m.create_step_2, m.create_step_3] as const;

  const initialStep: 1 | 2 | 3 = !api.isTauri && switches.step ? switches.step : 1;
  let step = $state<1 | 2 | 3>(initialStep);
  let name = $state(m.create_default_name());
  let dir = $state('');
  let password = $state('');
  let confirm = $state('');
  let strength = $state<Strength | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let created = $state<VaultInfo | null>(
    initialStep === 3
      ? {
          id: 'preview',
          name: m.create_default_name(),
          path: `/run/media/utente/CHIAVETTA/${m.create_default_name()}.nvault`,
          mountpoint: null,
          emblem: { kind: 'initial' },
          show_when_locked: true,
        }
      : null,
  );
  let emblemOpen = $state(false);
  // the open volume's info (emblem updates land there), else what vault_create returned
  const createdInfo = $derived(vaults.open.find((v) => v.id === created?.id) ?? created);
  let nameInput: HTMLInputElement | undefined = $state();

  const sep = $derived(dir.includes('\\') && !dir.includes('/') ? '\\' : '/');
  const cleanName = $derived(name.trim().replace(/\.nvault$/i, ''));
  const nameError = $derived(
    cleanName === ''
      ? m.create_name_needed()
      : /[\\/:*?"<>|\0]/.test(cleanName)
        ? m.create_name_chars()
        : null,
  );
  const fullPath = $derived(dir ? `${dir.replace(/[\\/]+$/, '')}${sep}${cleanName}.nvault` : '');
  const exists = $derived(app.recents.some((r) => r.path === fullPath));
  const step1Ok = $derived(!nameError && !!dir && !exists);
  const mismatch = $derived(confirm.length > 0 && confirm !== password);
  const step2Ok = $derived(!!strength?.acceptable && password === confirm && !busy);
  // "Fatto" tells the truth: the emblem can be chosen only while the new volume is open
  const createdOpen = $derived(!!created && vaults.open.some((v) => v.id === created!.id));

  onMount(async () => {
    try {
      dir = await api.defaultVolumeDir();
    } catch {
      dir = '';
    }
    preloadStrength();
    nameInput?.select();
  });

  async function next(e?: Event) {
    e?.preventDefault();
    error = null;
    if (step === 1 && step1Ok) step = 2;
    else if (step === 2 && step2Ok) {
      busy = true;
      try {
        created = await vaults.createVault(fullPath, password);
        password = confirm = '';
        step = 3;
      } catch (err) {
        error = errorText(err);
      } finally {
        busy = false;
      }
    } else if (step === 3) {
      finish();
    }
  }

  function back() {
    error = null;
    if (step === 2) step = 1;
    else if (step === 1) app.goto(vaults.open.length ? 'vault' : 'welcome');
  }

  function finish() {
    if (created && vaults.open.some((v) => v.id === created!.id)) {
      vaults.activate(created.id);
      app.goto('vault');
    } else app.goto('welcome');
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !busy && step !== 3) {
      e.preventDefault();
      back();
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<main class="wizard ground scroller" data-step={step} aria-labelledby="wizard-title">
  <div class="stage">
    <article class="form-sheet sheet">
      <Stitch inset={6} />
      <Band height={9} />
      <header class="head">
        <h1 id="wizard-title" class="title display">{m.create_title()}</h1>
        <ol class="steps" aria-label={m.create_steps()}>
          {#each STEPS as label, i (i)}
            <li class="step" data-state={i + 1 < step ? 'done' : i + 1 === step ? 'current' : 'todo'} aria-current={i + 1 === step ? 'step' : undefined}>
              <span class="step-n"><span class="step-digit">{i + 1}</span><Check size={12} strokeWidth={2.6} aria-hidden="true" class="step-check" /></span>
              <span class="step-label">{label()}</span>
            </li>
          {/each}
        </ol>
      </header>

      <form class="body" onsubmit={next} novalidate>
        {#if step === 1}
          <div class="field">
            <label for="vol-name">{m.create_name()}</label>
            <input
              id="vol-name"
              class="input"
              bind:this={nameInput}
              bind:value={name}
              autocomplete="off"
              spellcheck="false"
              aria-invalid={!!nameError}
              aria-describedby="vol-name-hint"
            />
            <p id="vol-name-hint" class={nameError ? 'error-text' : 'hint'}>{nameError ?? m.create_name_hint()}</p>
          </div>
          <div class="field">
            <span class="field-label" id="vol-dir-label">{m.create_location()}</span>
            <DrivePicker bind:dir labelledby="vol-dir-label" />
            {#if fullPath}
              <p class={exists ? 'error-text' : 'hint path'}>
                {exists ? m.create_exists() : fullPath}
              </p>
            {/if}
          </div>
        {:else if step === 2}
          <PasswordField
            bind:value={password}
            label={m.password()}
            autocomplete="new-password"
            autofocus
            describedby="new-pw-strength"
            disabled={busy}
          />
          <StrengthMeter id="new-pw-strength" {password} userInputs={[cleanName, 'nicevault']} bind:strength />
          <PasswordField
            bind:value={confirm}
            label={m.create_repeat()}
            autocomplete="new-password"
            invalid={mismatch}
            describedby={mismatch ? 'confirm-err' : undefined}
            disabled={busy}
          />
          {#if mismatch}<p id="confirm-err" class="error-text">{m.passwords_mismatch()}</p>{/if}
          <p class="warn">{m.create_warn()}</p>
        {:else}
          <div class="done" role="status">
            {#if createdInfo}
              <span class="done-seal" aria-hidden="true">
                <Rosette size={104} kind="seal"><Emblem emblem={createdInfo.emblem} name={createdInfo.name} size={36} decorative /></Rosette>
              </span>
            {/if}
            <p class="done-title display">{m.create_done()}</p>
            <p class="hint path">{created?.path}</p>
            <p class="hint">{m.create_done_hint()}</p>
            {#if createdInfo}
              <button type="button" class="btn sm" disabled={!createdOpen} onclick={() => (emblemOpen = true)} data-testid="emblem-open">
                {m.create_choose_emblem()}
              </button>
            {/if}
          </div>
        {/if}

        {#if error}<p class="error-text" role="alert">{error}</p>{/if}

        <footer class="nav">
          {#if step < 3}
            <button type="button" class="btn ghost" onclick={back} disabled={busy}>
              <ArrowLeft size={16} aria-hidden="true" />
              {step === 1 ? m.action_cancel() : m.action_back()}
            </button>
          {/if}
          <span class="spacer"></span>
          {#if step === 1}
            <button type="submit" class="btn primary" disabled={!step1Ok}>{m.create_next()}</button>
          {:else if step === 2}
            <button type="submit" class="btn primary" disabled={!step2Ok}>{busy ? m.create_busy() : m.create_submit()}</button>
          {:else}
            <!-- svelte-ignore a11y_autofocus -->
            <button type="submit" class="btn primary" autofocus>{m.create_open()}</button>
          {/if}
        </footer>
      </form>
    </article>
  </div>

  <footer class="imprint"><Brand /></footer>
</main>

{#if createdInfo}
  <EmblemPicker
    open={emblemOpen}
    name={createdInfo.name}
    value={{ emblem: createdInfo.emblem, show_when_locked: createdInfo.show_when_locked }}
    onsave={(st) => vaults.setEmblem(createdInfo!.id, st.emblem, st.show_when_locked)}
    onclose={() => (emblemOpen = false)}
  />
{/if}

<style>
  /* issuing a new document: a form sheet on the board */
  .wizard {
    position: relative;
    z-index: 2;
    height: 100%;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
  .stage {
    flex: 1;
    display: grid;
    place-items: center;
    padding: var(--space-6) var(--space-5) var(--space-4);
  }
  .imprint {
    display: flex;
    justify-content: center;
    padding: 0 0 var(--space-4);
  }
  .form-sheet {
    --bleed-start: var(--space-6);
    --bleed-end: var(--space-6);
    width: min(600px, 100%);
    padding: 18px var(--space-6) var(--space-5);
    border-radius: var(--radius-stitched);
    box-shadow: var(--shadow-sheet);
  }
  .head {
    margin: var(--space-4) 0 var(--space-5);
  }
  .title {
    font-size: 38px;
    font-weight: 500;
    color: var(--title-ink);
  }

  /* the three stations, joined by a printed rule */
  .steps {
    list-style: none;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    row-gap: var(--space-2);
    padding: 0;
    margin: var(--space-3) 0 0;
  }
  .step {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--ink-3);
  }
  .step + .step::before {
    content: '';
    width: 22px;
    height: 1px;
    margin: 0 var(--space-2) 0 var(--space-1);
    background: var(--paper-rule-2);
  }
  .step[data-state='current'] {
    color: var(--ink);
  }
  /* a station: an empty seat (todo), the volume's ink (current), a tick (done) */
  .step-n {
    position: relative;
    display: inline-grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    font-size: 10.5px;
    letter-spacing: 0;
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .step-n > :global(*) {
    grid-area: 1 / 1;
  }
  .step-n :global(.step-check) {
    display: none;
  }
  .step[data-state='current'] .step-n {
    color: var(--accent-ink);
    background: linear-gradient(180deg, var(--key-accent-hi), var(--key-accent-lo));
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.3),
      0 0 0 1px var(--key-accent-edge),
      0 1px 2px rgb(var(--cast) / 0.25);
  }
  .step[data-state='done'] .step-n {
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  .step[data-state='done'] .step-digit {
    visibility: hidden;
  }
  .step[data-state='done'] .step-n :global(.step-check) {
    display: block;
  }

  /* ---- the form ------------------------------------------------------------- */
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }
  .body > :global(.field),
  .body > :global(.password-field) {
    gap: var(--space-2);
  }
  .body :global(.input) {
    height: 38px;
    font-size: var(--text-base);
  }
  /* the meter and the mismatch line belong to the field above them */
  .body > :global(.strength),
  .body > :global(#confirm-err) {
    margin-top: calc(var(--space-3) - var(--space-5));
  }
  .body :global(.hint),
  .body :global(.error-text) {
    overflow-wrap: anywhere;
  }
  .path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }
  /* the one sentence that matters on this step */
  .warn {
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
    color: var(--ink);
    background: var(--accent-soft);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  /* ---- Fatto: the document is issued --------------------------------------- */
  .done {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    text-align: center;
    padding: 0 0 var(--space-2);
  }
  .done-seal {
    display: grid;
    margin-bottom: var(--space-2);
  }
  .done-title {
    font-size: 30px;
    color: var(--title-ink);
  }
  .done :global(.btn) {
    margin-top: var(--space-2);
  }

  .nav {
    display: flex;
    gap: var(--space-3);
    margin-top: var(--space-1);
    padding-top: var(--space-4);
    border-top: 1px solid var(--paper-rule);
  }
  .spacer {
    flex: 1;
  }
</style>
