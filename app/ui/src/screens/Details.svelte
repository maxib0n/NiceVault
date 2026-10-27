<script lang="ts">
  import { KeyRound, Plus, RotateCcwKey, ShieldAlert } from '@lucide/svelte';
  import * as api from '../lib/api';
  import { backendLabel, backendReason } from '../lib/backends';
  import { errorCode, errorText } from '../lib/errors';
  import { fileCount, formatBytes } from '../lib/format';
  import { radioKeys } from '../lib/radio';
  import { preloadStrength, type Strength } from '../lib/strength';
  import { switches } from '../lib/switches';
  import { idle } from '../lib/stores/idle.svelte';
  import { prefs, type ThemePref } from '../lib/stores/prefs.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import { ROOT, type LockScope, type Settings, type SlotInfo, type VaultStats, type Visibility } from '../lib/types';
  import { mrzInside } from '../ornament/document';
  import { ACCENTS, type AccentId } from '../lib/accents';
  import { accent } from '../lib/stores/accent.svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from '../components/Dialog.svelte';
  import Emblem from '../components/Emblem.svelte';
  import EmblemPicker from '../components/EmblemPicker.svelte';
  import RotateKey from '../components/RotateKey.svelte';
  import FormatUpgrade from '../components/FormatUpgrade.svelte';
  import StickPrograms from '../components/device/StickPrograms.svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import InkChip from '../components/InkChip.svelte';
  import Band from '../ornament/Band.svelte';
  import PasswordField from '../components/PasswordField.svelte';
  import StrengthMeter from '../components/StrengthMeter.svelte';
  import ResizeDialog from '../components/ResizeDialog.svelte';

  type DialogKind = 'add-password' | 'change-password' | 'remove-slot' | 'add-duress';

  const active = $derived(vaults.active);
  let stats = $state<VaultStats | null>(null);
  let created = $state<number | null>(null);
  let slots = $state<SlotInfo[]>([]);
  let settings = $state<Settings | null>(null);

  let dialog = $state<DialogKind | null>(null);
  let slotTarget = $state<SlotInfo | null>(null);
  let pwOld = $state('');
  let pwNew = $state('');
  let pwConfirm = $state('');
  let label = $state('');
  let strength = $state<Strength | null>(null);
  let busy = $state(false);
  let dialogError = $state<string | null>(null);
  let emblemOpen = $state(false);
  let rotateOpen = $state(false);
  let resizeOpen = $state(false);
  let duressStep = $state<1 | 2>(1);
  let duressUnderstood = $state(false);

  const AUTO_LOCK_OPTIONS: { min: number; label: () => string }[] = [
    { min: 5, label: () => m.det_minutes({ n: '5' }) },
    { min: 15, label: () => m.det_minutes({ n: '15' }) },
    { min: 30, label: () => m.det_minutes({ n: '30' }) },
    { min: 60, label: m.det_hour },
    { min: 0, label: m.det_never },
  ];
  const SCOPES: { id: LockScope; label: () => string }[] = [
    { id: 'volume', label: m.det_scope_volume },
    { id: 'all', label: m.det_scope_all },
  ];
  const THEMES: { id: ThemePref; label: () => string }[] = [
    { id: 'system', label: m.theme_system },
    { id: 'light', label: m.theme_light },
    { id: 'dark', label: m.theme_dark },
  ];
  const LOOKS: { id: Visibility; label: () => string; hint: () => string }[] = [
    { id: 'visible', label: m.look_visible, hint: m.look_visible_hint },
    { id: 'encrypted', label: m.look_encrypted, hint: m.look_encrypted_hint },
  ];
  const look = $derived<Visibility>(active?.visibility ?? (active?.show_when_locked === false ? 'encrypted' : 'visible'));
  let lookBusy = $state(false);
  async function setLook(v: Visibility) {
    if (!active || lookBusy || v === look) return;
    lookBusy = true;
    await vaults.setVisibility(active.id, v);
    lookBusy = false;
  }

  $effect(() => {
    const v = active;
    if (!v) return;
    void load(v.id);
  });
  $effect(() => void (active && vaults.browser?.changes && api.vaultStats(active.id).then((s) => (stats = s), () => {})));

  async function load(id: string) {
    try {
      const [s, sl, st, root] = await Promise.all([api.vaultStats(id), api.slotsList(id), api.settingsGet(), api.fsProps(id, ROOT).catch(() => null)]);
      stats = s;
      slots = sl;
      settings = st;
      created = root?.created ? root.created / 1_000_000 : null;
      if (!api.isTauri && switches.dialog && dialog === null) openDialogFromSwitch(switches.dialog);
    } catch (e) {
      toasts.error(e);
    }
    void vaults.loadBackends();
  }

  function openDialogFromSwitch(d: string) {
    const duress = slots.find((s) => s.kind === 'duress');
    if (d === 'add-password') openDialog('add-password');
    else if (d === 'change-password') openDialog('change-password', slots[0]);
    else if (d === 'remove-slot') openDialog('remove-slot', slots.filter((s) => s.kind !== 'duress').at(-1) ?? null);
    else if (d === 'remove-duress' && duress) openDialog('remove-slot', duress);
    else if (d === 'add-duress' || d === 'add-duress-2') {
      openDialog('add-duress');
      if (d === 'add-duress-2') {
        duressUnderstood = true;
        duressStep = 2;
      }
    } else if (d === 'emblem') emblemOpen = true;
    else if (d === 'rotate-key') rotateOpen = true;
    else if (d === 'resize') resizeOpen = true;
  }

  function openDialog(kind: DialogKind, slot: SlotInfo | null = null) {
    dialog = kind;
    slotTarget = slot;
    pwOld = pwNew = pwConfirm = label = '';
    strength = null;
    dialogError = null;
    duressStep = 1;
    duressUnderstood = false;
    if (kind === 'add-password' || kind === 'change-password') preloadStrength();
  }

  function closeDialog() {
    if (busy) return;
    dialog = null;
    pwOld = pwNew = pwConfirm = '';
  }

  const mismatch = $derived(pwConfirm.length > 0 && pwConfirm !== pwNew);
  const canSubmitPw = $derived(!!strength?.acceptable && pwNew === pwConfirm && !busy && (dialog !== 'change-password' || pwOld.length > 0));
  const canSubmitDuress = $derived(duressStep === 2 && pwNew.length > 0 && pwNew === pwConfirm && !busy);

  async function submitDialog(e?: Event) {
    e?.preventDefault();
    if (!active || busy) return;
    if ((dialog === 'add-password' || dialog === 'change-password') && !canSubmitPw) return;
    if (dialog === 'add-duress' && duressStep === 1) {
      if (duressUnderstood) duressStep = 2;
      return;
    }
    if (dialog === 'add-duress' && !canSubmitDuress) return;
    busy = true;
    dialogError = null;
    try {
      if (dialog === 'add-password') {
        slots = await api.slotAddPassword(active.id, pwNew, label.trim() || undefined);
        toasts.push(m.det_password_added(), 'success');
      } else if (dialog === 'change-password' && slotTarget) {
        slots = await api.slotChangePassword(active.id, slotTarget.index, pwOld, pwNew);
        toasts.push(m.det_password_changed(), 'success');
      } else if (dialog === 'remove-slot' && slotTarget) {
        slots = await api.slotRemove(active.id, slotTarget.index);
        toasts.push(m.det_key_removed(), 'success');
      } else if (dialog === 'add-duress') {
        slots = await api.slotAddDuress(active.id, pwNew, label.trim() || undefined);
        toasts.push(m.duress_added(), 'success');
      }
      busy = false;
      closeDialog();
    } catch (err) {
      dialogError = errorCode(err) === 'duress_conflict' ? m.err_duress_conflict() : errorText(err);
    } finally {
      busy = false;
    }
  }

  async function setAutoLock(patch: Partial<Settings>) {
    if (!settings) return;
    const prev = settings;
    settings = { ...settings, ...patch };
    try {
      settings = await api.settingsSet(patch);
      if (patch.auto_lock_minutes !== undefined && !switches.autolock) idle.setMinutes(settings.auto_lock_minutes);
    } catch (e) {
      settings = prev;
      toasts.error(e);
    }
  }

  const KIND_LABEL: Record<SlotInfo['kind'], () => string> = {
    password: m.slot_password,
    recipient: m.slot_recipient,
    fido2: m.slot_fido2,
    duress: m.slot_duress,
  };
  const slotKindLabel = (s: SlotInfo) => KIND_LABEL[s.kind]();
  const kdfText = (s: SlotInfo) => (s.kdf && s.kind !== 'duress' ? `Argon2id · ${Math.round(s.kdf.m_kib / 1024)} MiB · t=${s.kdf.t} · p=${s.kdf.p}` : '');
  const openers = $derived(slots.filter((s) => s.kind !== 'duress'));
  const onlyOne = $derived(openers.length <= 1);
  const canRemove = (s: SlotInfo) => s.kind === 'duress' || !onlyOne;
  const minutes = $derived(settings?.auto_lock_minutes ?? 15);
  const mrz = $derived(
    active ? mrzInside(active, { created, files: stats?.file_count ?? null, changed: stats?.modified_ms || null, keys: slots.length || null }) : null,
  );
</script>

<main class="details sheet scroller" aria-labelledby="details-title">
  <div class="page head-page">
    <header class="details-head">
      {#if active}
        <span class="head-seal" aria-hidden="true">
          <Rosette size={88} kind="cover" frame={active.emblem_frame}>
            <Emblem emblem={active.emblem} name={active.name} size={32} decorative />
          </Rosette>
        </span>
      {/if}
      <div class="head-text">
        <p class="caps">{m.det_kicker()}</p>
        <h1 id="details-title" class="title display">{active?.name ?? m.volume()}</h1>
        <p class="path" title={active?.path}>{active?.path}</p>
        {#if mrz}<p class="mrz" aria-hidden="true" data-testid="details-mrz"><span>{mrz[0]}</span><span>{mrz[1]}</span></p>{/if}
      </div>
    </header>
  </div>
  <Band />
  <div class="page body-page">
    <div class="sections">
      <section class="card stats" aria-labelledby="stats-title">
        <h2 id="stats-title" class="card-title display">{m.det_volume()}</h2>
        <div class="stat-lead">
          <span class="lead-figure">{stats ? formatBytes(stats.used_bytes) : '—'}</span>
          <span class="lead-note">{m.det_content_in({ files: stats ? fileCount(stats.file_count) : '—' })}</span>
        </div>
        <dl class="stat-grid">
          <div><dt class="caps">{m.det_file_size()}</dt><dd class="num">{stats ? formatBytes(stats.file_size_bytes) : '—'}</dd></div>
          {#if stats?.free_bytes !== undefined}
            <div><dt class="caps">{m.det_free()}</dt><dd class="num">{formatBytes(stats.free_bytes)}</dd></div>
          {/if}
        </dl>
        <button type="button" class="btn sm resize-key" disabled={!active} onclick={() => (resizeOpen = true)} data-testid="resize-open">{m.rsz_open()}</button>
        <FormatUpgrade />
        <StickPrograms />
      </section>

      <section class="card emblem-card" aria-labelledby="emblem-title">
        <div class="card-head">
          <h2 id="emblem-title" class="card-title display">{m.det_emblem()}</h2>
          <button type="button" class="btn sm" disabled={!active} onclick={() => (emblemOpen = true)} aria-label={m.det_emblem_choose()} data-testid="emblem-open">{m.action_choose()}</button>
        </div>
        <div class="emblem-row">
          {#if active}
            <span class="emblem-seal"><Rosette size={64} kind="swatch" frame={active.emblem_frame}><Emblem emblem={active.emblem} name={active.name} size={24} /></Rosette></span>
          {/if}
          <p class="hint">{look === 'encrypted' ? m.det_emblem_hidden() : m.det_emblem_visible()}</p>
        </div>
      </section>

      <section class="card slots" aria-labelledby="slots-title" data-testid="slots">
        <div class="card-head">
          <h2 id="slots-title" class="card-title display">{m.det_keys()}</h2>
          <button type="button" class="btn sm" disabled={slots.length >= 8} onclick={() => openDialog('add-password')} data-testid="add-password">
            <Plus size={14} aria-hidden="true" /> {m.det_add_password()}
          </button>
        </div>
        <p class="hint">{m.det_keys_hint()}</p>
        <ul class="slot-list">
          {#each slots as s (s.index)}
            <li class="slot" data-kind={s.kind} data-in-use={s.in_use} data-testid="slot">
              <span class="slot-glyph" aria-hidden="true">{#if s.kind === 'duress'}<ShieldAlert size={15} />{:else}<KeyRound size={15} />{/if}</span>
              <div class="slot-main">
                <p class="slot-title">
                  {s.label || slotKindLabel(s)}
                  {#if s.in_use}<span class="badge">{m.det_in_use()}</span>{/if}
                </p>
                <p class="slot-meta">
                  {#if s.kind === 'duress'}{m.duress_kind_hint()}{:else}{slotKindLabel(s)}{/if} · {m.det_slot({ n: String(s.index + 1) })}{#if kdfText(s)}{' · '}<span class="mono">{kdfText(s)}</span>{/if}
                </p>
              </div>
              {#if s.kind === 'password'}
                <button type="button" class="btn sm" onclick={() => openDialog('change-password', s)}>{m.action_change()}</button>
              {/if}
              <button
                type="button"
                class="btn sm"
                disabled={!canRemove(s)}
                title={!canRemove(s) ? m.err_last_slot() : undefined}
                aria-describedby={!canRemove(s) ? 'last-slot-note' : undefined}
                onclick={() => openDialog('remove-slot', s)}
              >
                {m.action_remove()}
              </button>
            </li>
          {/each}
        </ul>
        {#if onlyOne}<p class="hint" id="last-slot-note">{m.det_only_key()}</p>{/if}
        <button type="button" class="btn sm ghost duress-add" disabled={slots.length >= 8} onclick={() => openDialog('add-duress')} data-testid="add-duress">
          <ShieldAlert size={14} aria-hidden="true" /> {m.duress_add()}
        </button>
        <div class="rotate">
          <div class="rotate-text">
            <p class="rotate-title">{m.det_rotate()}</p>
            <p class="hint">{m.det_rotate_line()}</p>
          </div>
          <button type="button" class="btn sm" disabled={!active || !slots.some((s) => s.kind === 'password')} onclick={() => (rotateOpen = true)} data-testid="rotate-open">
            <RotateCcwKey size={14} aria-hidden="true" /> {m.rot_confirm()}…
          </button>
        </div>
      </section>

      <section class="card autolock" aria-labelledby="autolock-title" data-testid="autolock">
        <h2 id="autolock-title" class="card-title display">{m.det_autolock()}</h2>
        <p class="hint">{minutes === 0 ? m.det_autolock_never_hint() : m.det_autolock_hint()}</p>
        <div
          class="segmented"
          role="radiogroup"
          aria-labelledby="autolock-title"
          tabindex="-1"
          data-testid="autolock-minutes"
          onkeydown={(e) => radioKeys(e, AUTO_LOCK_OPTIONS.map((o) => o.min), minutes, (v) => setAutoLock({ auto_lock_minutes: v }))}
        >
          {#each AUTO_LOCK_OPTIONS as o (o.min)}
            <button type="button" role="radio" aria-checked={minutes === o.min} tabindex={minutes === o.min ? 0 : -1} disabled={!settings} onclick={() => setAutoLock({ auto_lock_minutes: o.min })}>
              {o.label()}
            </button>
          {/each}
        </div>
        <div class="switch-row">
          <span class="switch-label" id="lock-sleep-label">{m.det_lock_sleep()}</span>
          <button type="button" class="switch" role="switch" aria-checked={settings?.lock_on_sleep ?? true} aria-labelledby="lock-sleep-label" disabled={!settings} data-testid="lock-on-sleep" onclick={() => setAutoLock({ lock_on_sleep: !settings?.lock_on_sleep })}>
            <span class="knob"></span>
          </button>
        </div>
        <div class="switch-row">
          <span class="switch-label" id="lock-screen-label">{m.det_lock_screen()}</span>
          <button type="button" class="switch" role="switch" aria-checked={settings?.lock_on_screen_lock ?? true} aria-labelledby="lock-screen-label" disabled={!settings} data-testid="lock-on-screen" onclick={() => setAutoLock({ lock_on_screen_lock: !settings?.lock_on_screen_lock })}>
            <span class="knob"></span>
          </button>
        </div>
        <h3 id="scope-title" class="sub-title caps">{m.det_scope()}</h3>
        <div
          class="segmented"
          role="radiogroup"
          aria-labelledby="scope-title"
          aria-describedby="scope-hint"
          tabindex="-1"
          data-testid="lock-scope"
          onkeydown={(e) => radioKeys(e, SCOPES.map((o) => o.id), settings?.lock_scope ?? 'volume', (v) => setAutoLock({ lock_scope: v }))}
        >
          {#each SCOPES as o (o.id)}
            {@const on = (settings?.lock_scope ?? 'volume') === o.id}
            <button type="button" role="radio" aria-checked={on} tabindex={on ? 0 : -1} disabled={!settings} onclick={() => setAutoLock({ lock_scope: o.id })}>
              {o.label()}
            </button>
          {/each}
        </div>
        <p class="hint" id="scope-hint">{m.det_scope_hint()}</p>
      </section>

      <section class="card backends" aria-labelledby="backends-title">
        <h2 id="backends-title" class="card-title display">{m.det_backends()}</h2>
        <p class="hint">{m.det_backends_hint()}</p>
        <ul class="backend-list">
          {#each vaults.backends as b (b.id)}
            <li class="backend" data-available={b.available}>
              <span class="backend-name">{backendLabel(b)}</span>
              <span class="backend-state">{b.available ? m.det_available() : (backendReason(b) ?? m.det_unavailable())}</span>
            </li>
          {/each}
        </ul>
      </section>

      <section class="card appearance" aria-labelledby="theme-title">
        <h2 id="theme-title" class="card-title display">{m.det_appearance()}</h2>
        <h3 id="theme-sub" class="sub-title caps first">{m.det_theme()}</h3>
        <div
          class="segmented themes"
          role="radiogroup"
          aria-labelledby="theme-title theme-sub"
          tabindex="-1"
          data-testid="volume-theme"
          onkeydown={(e) => radioKeys(e, THEMES.map((t) => t.id), prefs.theme, (v) => prefs.setTheme(v))}
        >
          {#each THEMES as t (t.id)}
            <button type="button" role="radio" data-theme-id={t.id} aria-checked={prefs.theme === t.id} tabindex={prefs.theme === t.id ? 0 : -1} onclick={() => prefs.setTheme(t.id)}>
              <span class="chip" data-chip={t.id} aria-hidden="true"></span>{t.label()}
            </button>
          {/each}
        </div>
        <h3 id="accent-title" class="sub-title caps">{m.det_ink()}</h3>
        <div
          class="swatches"
          role="radiogroup"
          aria-labelledby="accent-title"
          tabindex="-1"
          data-testid="accent-swatches"
          onkeydown={(e) => radioKeys(e, ACCENTS.map((a) => a.id), accent.current, (v: AccentId) => accent.set(v))}
        >
          {#each ACCENTS as a (a.id)}
            <button
              type="button"
              role="radio"
              class="swatch"
              data-accent={a.id}
              aria-checked={accent.current === a.id}
              aria-label={a.name()}
              title={a.name()}
              tabindex={accent.current === a.id ? 0 : -1}
              onclick={() => accent.set(a.id)}
            >
              <InkChip size={44} />
              <span class="swatch-name" aria-hidden="true">{a.name()}</span>
            </button>
          {/each}
        </div>
        <h3 id="look-title" class="sub-title caps">{m.look_locked()}</h3>
        <div
          class="segmented looks"
          role="radiogroup"
          aria-labelledby="look-title"
          aria-describedby="look-hint"
          aria-busy={lookBusy}
          tabindex="-1"
          data-testid="look-visibility"
          onkeydown={(e) => radioKeys(e, LOOKS.map((l) => l.id), look, (v) => setLook(v))}
        >
          {#each LOOKS as l (l.id)}
            <button type="button" role="radio" aria-checked={look === l.id} tabindex={look === l.id ? 0 : -1} disabled={!active} onclick={() => setLook(l.id)}>
              {l.label()}
            </button>
          {/each}
        </div>
        <p class="hint" id="look-hint">{LOOKS.find((l) => l.id === look)?.hint()}</p>
      </section>
    </div>
  </div>
</main>

<Dialog
  open={dialog === 'add-password' || dialog === 'change-password'}
  title={dialog === 'add-password' ? m.det_add_password() : m.det_change_password()}
  onclose={closeDialog}
>
  <form class="dialog-form" id="pw-form" onsubmit={submitDialog}>
    {#if dialog === 'change-password'}
      <PasswordField bind:value={pwOld} label={m.det_current_password()} autofocus disabled={busy} />
    {:else}
      <div class="field">
        <label for="slot-label">{m.det_slot_name()}</label>
        <input id="slot-label" class="input" bind:value={label} placeholder={m.det_slot_name_ph()} disabled={busy} />
      </div>
    {/if}
    <PasswordField
      bind:value={pwNew}
      label={dialog === 'change-password' ? m.det_new_password() : m.password()}
      autocomplete="new-password"
      autofocus={dialog === 'add-password'}
      describedby="slot-strength"
      disabled={busy}
    />
    <StrengthMeter id="slot-strength" password={pwNew} userInputs={[active?.name ?? '']} bind:strength />
    <PasswordField bind:value={pwConfirm} label={m.det_repeat()} autocomplete="new-password" invalid={mismatch} disabled={busy} />
    {#if mismatch}<p class="error-text">{m.passwords_mismatch()}</p>{/if}
    {#if dialogError}<p class="error-text" role="alert">{dialogError}</p>{/if}
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={closeDialog} disabled={busy}>{m.action_cancel()}</button>
    <button type="submit" form="pw-form" class="btn primary" disabled={!canSubmitPw}>
      {busy ? m.action_saving() : dialog === 'add-password' ? m.action_add() : m.action_change()}
    </button>
  {/snippet}
</Dialog>

<Dialog open={dialog === 'add-duress'} title={m.duress_title()} variant="danger" onclose={closeDialog} describedby="duress-what">
  <form class="dialog-form" id="duress-form" onsubmit={submitDialog} data-step={duressStep} data-testid="duress-form">
    {#if duressStep === 1}
      <p id="duress-what">{m.duress_what()}</p>
      <p class="duress-limit">{m.duress_limit()}</p>
      <label class="check">
        <input type="checkbox" class="danger" bind:checked={duressUnderstood} data-testid="duress-understood" />
        <span>{m.duress_confirm()}</span>
      </label>
    {:else}
      <p id="duress-what" class="hint">{m.duress_limit()}</p>
      <div class="field">
        <label for="duress-label">{m.det_slot_name()}</label>
        <input id="duress-label" class="input" bind:value={label} placeholder={m.duress_label_ph()} disabled={busy} />
      </div>
      <PasswordField bind:value={pwNew} label={m.password()} autocomplete="new-password" autofocus disabled={busy} invalid={!!dialogError} />
      <PasswordField bind:value={pwConfirm} label={m.det_repeat()} autocomplete="new-password" invalid={mismatch} disabled={busy} />
      {#if mismatch}<p class="error-text">{m.passwords_mismatch()}</p>{/if}
      {#if dialogError}<p class="error-text" role="alert" data-testid="duress-error">{dialogError}</p>{/if}
    {/if}
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={closeDialog} disabled={busy}>{m.action_cancel()}</button>
    {#if duressStep === 1}
      <button type="submit" form="duress-form" class="btn danger" disabled={!duressUnderstood}>{m.duress_continue()}</button>
    {:else}
      <button type="submit" form="duress-form" class="btn danger" disabled={!canSubmitDuress}>{busy ? m.action_saving() : m.duress_arm()}</button>
    {/if}
  {/snippet}
</Dialog>

{#if active}
  <RotateKey
    open={rotateOpen}
    vault={active.id}
    {slots}
    {stats}
    onclose={() => (rotateOpen = false)}
    onrotated={(sl) => {
      slots = sl;
      void api.vaultStats(active.id).then((s) => (stats = s), () => {});
    }}
  />
  <ResizeDialog open={resizeOpen} vault={active.id} onclose={() => (resizeOpen = false)} onresized={() => api.vaultStats(active.id).then((s) => (stats = s), () => {})} />
  <EmblemPicker
    open={emblemOpen}
    name={active.name}
    value={{ emblem: active.emblem, show_when_locked: active.show_when_locked, emblem_frame: active.emblem_frame }}
    onsave={(st) => vaults.setEmblem(active.id, st.emblem, st.show_when_locked, st.emblem_frame)}
    onclose={() => (emblemOpen = false)}
  />
{/if}

<Dialog open={dialog === 'remove-slot'} title={m.det_remove_title()} variant="danger" onclose={closeDialog} describedby="remove-desc">
  <p id="remove-desc">
    {#if slotTarget?.kind === 'duress'}
      {m.duress_remove_desc({ name: slotTarget.label || m.slot_duress() })}
    {:else}
      {m.det_remove_desc({ name: slotTarget?.label || (slotTarget ? slotKindLabel(slotTarget) : '') })}
      {#if slotTarget?.in_use}{m.det_remove_in_use()}{/if}
    {/if}
  </p>
  {#if dialogError}<p class="error-text" role="alert">{dialogError}</p>{/if}
  {#snippet actions()}
    <button type="button" class="btn" onclick={closeDialog} disabled={busy}>{m.action_cancel()}</button>
    <button type="button" class="btn danger" onclick={() => submitDialog()} disabled={busy}>{m.action_remove()}</button>
  {/snippet}
</Dialog>

<style>
  .details {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
  }
  .page {
    max-width: 1040px;
    margin: 0 auto;
    padding: 0 var(--space-6) var(--space-7);
  }
  .head-page {
    padding-top: var(--space-6);
    padding-bottom: 0;
  }
  .mrz {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 12px;
    font-family: var(--font-mono);
    font-size: min(11.5px, calc(100cqw / 33.8));
    font-weight: 500;
    line-height: 1.3;
    letter-spacing: 0.16em;
    color: var(--ink-3);
    white-space: pre;
  }
  @container (max-width: 270px) {
    .mrz {
      display: none;
    }
  }
  .details-head {
    display: flex;
    align-items: center;
    gap: var(--space-5);
    padding-bottom: var(--space-4);
  }
  .head-seal {
    display: grid;
    flex: none;
  }
  .head-text {
    flex: 1;
    min-width: 0;
    container-type: inline-size;
  }
  .title {
    margin-top: 6px;
    font-size: clamp(30px, 3.2vw, 42px);
    font-weight: 500;
    line-height: 1.05;
    color: var(--title-ink);
    overflow-wrap: break-word;
    text-wrap: balance;
  }
  .path {
    margin-top: 6px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }

  .sections {
    columns: 380px 2;
    column-gap: var(--space-7);
    margin-top: var(--space-2);
  }
  .card {
    break-inside: avoid;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-5) 0;
    border-bottom: 1px solid var(--paper-rule);
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .card-title {
    font-size: var(--display-s);
    font-weight: 500;
    color: var(--ink);
  }
  .card > .hint {
    max-width: 52ch;
  }

  .stat-lead {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 4px 10px;
    margin-top: 2px;
  }
  .lead-figure {
    font-family: var(--font-mono);
    font-size: 34px;
    font-weight: 400;
    letter-spacing: -0.02em;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    color: var(--title-ink);
  }
  .lead-note {
    font-size: var(--text-base);
    color: var(--ink-3);
  }
  .stat-grid {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    margin: var(--space-2) 0 0;
  }
  dd {
    margin: 5px 0 0;
    font-size: var(--text-base);
    font-weight: 500;
    color: var(--ink);
  }

  .emblem-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }
  .emblem-seal {
    display: grid;
    flex: none;
  }
  .emblem-row .hint {
    flex: 1;
    min-width: 0;
    overflow-wrap: break-word;
  }

  .slot-list,
  .backend-list {
    list-style: none;
    margin: var(--space-1) 0 0;
    padding: 0;
    border-top: 1px solid var(--paper-rule-2);
    border-bottom: 1px solid var(--paper-rule-2);
  }
  .slot {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 11px 2px;
  }
  .slot + .slot,
  .backend + .backend {
    border-top: 1px solid var(--paper-rule);
  }
  .slot-glyph {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .slot-main {
    flex: 1;
    min-width: 0;
  }
  .slot-title {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 2px var(--space-2);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .slot-meta {
    margin-top: 2px;
    font-size: var(--text-sm);
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .slot-meta .mono {
    font-size: 11px;
  }

  .segmented.themes [role='radio'],
  .segmented.looks [role='radio'] {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .chip {
    flex: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    box-shadow:
      inset 0 0 0 1px rgb(0 0 0 / 0.28),
      inset 1px 1px 1px rgb(255 255 255 / 0.18);
  }
  .chip[data-chip='dark'] {
    background: radial-gradient(circle at 30% 30%, #3a3836, #1c1b1a 72%);
  }
  .chip[data-chip='light'] {
    background: radial-gradient(circle at 30% 30%, #fffdf8, #e9e2d5 72%);
  }
  .chip[data-chip='system'] {
    background: linear-gradient(135deg, #f6f1e7 50%, #262523 50%);
  }

  .backend {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
    padding: 10px 2px;
  }
  .backend-name {
    flex: none;
    font-weight: 600;
  }
  .backend-state {
    min-width: 0;
    text-align: right;
    font-size: var(--text-sm);
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .backend[data-available='true'] .backend-state {
    color: var(--ink);
    font-weight: 500;
  }
  .backend[data-available='true'] .backend-state::before {
    content: '';
    display: inline-block;
    width: 8px;
    height: 4px;
    margin: 0 8px 3px 0;
    border-left: 1.6px solid var(--accent);
    border-bottom: 1.6px solid var(--accent);
    transform: rotate(-45deg);
  }

  .sub-title {
    margin-top: var(--space-4);
  }
  .sub-title.first {
    margin-top: var(--space-1);
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) 6px;
  }
  .swatch {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 76px;
    padding: 8px 2px 6px;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: var(--ink-3);
  }
  .swatch :global(.ink-chip) {
    transition: transform var(--dur-micro) var(--ease-out);
  }
  .swatch-name {
    font-size: var(--text-xs);
    line-height: 1.2;
    text-align: center;
    color: var(--ink-3);
  }
  .swatch:hover :global(.ink-chip) {
    transform: translateY(-1px);
  }
  .swatch:active :global(.ink-chip) {
    transform: translateY(1px);
  }
  .swatch[aria-checked='true'] :global(.ink-chip) {
    box-shadow:
      0 0 0 2px var(--paper),
      0 0 0 3.5px var(--accent),
      0 3px 6px -3px rgb(var(--cast) / 0.5);
  }
  .swatch[aria-checked='true'] .swatch-name {
    color: var(--accent);
    font-weight: 600;
  }
  .swatch:focus-visible {
    outline-color: var(--accent);
  }

  .dialog-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .slot[data-kind='duress'] .slot-glyph {
    color: var(--danger);
  }
  .duress-add {
    align-self: flex-start;
    margin-top: var(--space-1);
    color: var(--ink-3);
  }
  .resize-key {
    align-self: flex-start;
    margin-top: var(--space-3);
  }
  .rotate {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px dashed var(--paper-rule-2);
  }
  .rotate-text {
    min-width: 0;
  }
  .rotate-title {
    font-weight: 600;
    color: var(--ink);
  }
  .rotate .hint {
    margin-top: 2px;
    max-width: 46ch;
  }
  .rotate .btn {
    flex: none;
  }
  .duress-limit {
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--danger-wash);
    color: var(--ink);
    font-weight: 500;
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    color: var(--ink);
  }
  .check input {
    margin-top: 2px;
  }

  .autolock .switch-row:first-of-type {
    margin-top: var(--space-2);
  }
</style>
