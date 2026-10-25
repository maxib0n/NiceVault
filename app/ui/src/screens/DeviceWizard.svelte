<!--
  Crea dispositivo NiceVault, a full-window sheet: 1 Unità (external drives only, the
  partitions that will be erased) → 2 Opzioni (partition table, VaultData's file system with where
  it is readable, programs, first volume) → 3 Conferma (summary, "Mostra comandi", then the drive
  name typed) → 4 Creazione (resumable steps, bytes for download / copy) → 5 Fatto (the first
  volume, Apri VaultData, Espelli). "Completa" resumes a device whose creation stopped midway.
  Every call it waits on is shown (the pressed key turns its lathes and says what it does, the
  footer's status line counts the seconds, reassures after 2 s, offers Annulla / Riprova after
  30 s); Indietro and Esc always work before the erase (a stale answer is dropped by its token).
  The privileged steps say that the system's password window may appear. A drive holding
  NiceVault volumes names them and asks for one more explicit consent.
  Mock: ?screen=device&device_step=disk|options|confirm|type|progress|done|error,
  device_slow=candidates|options|plan|create|auth, device_volumes, device_stuck_ms (switches.ts).
-->
<script lang="ts">
  import { ArrowLeft, Check, ChevronDown, ChevronRight, CircleAlert, HardDrive, LoaderCircle, RefreshCw, ShieldCheck, TriangleAlert, Usb, X } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { m } from '../paraglide/messages.js';
  import * as api from '../lib/api';
  import * as dev from '../lib/device';
  import type { DataFsId, DeviceDone, DeviceOptions, DeviceOutcome, DevicePlan, DeviceProgress, DeviceStep, DiskCandidate, OsId, TableId, VolumeMode } from '../lib/device';
  import { errorCode, errorText } from '../lib/errors';
  import { STUCK_MS } from '../lib/device';
  import { formatBytes } from '../lib/format';
  import { preloadStrength, type Strength } from '../lib/strength';
  import { switches } from '../lib/switches';
  import { app } from '../lib/stores/app.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import type { ApiError, VaultInfo } from '../lib/types';
  import PasswordField from '../components/PasswordField.svelte';
  import StrengthMeter from '../components/StrengthMeter.svelte';
  import Band from '../ornament/Band.svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import Stitch from '../ornament/Stitch.svelte';
  import AuthCard from '../components/device/AuthCard.svelte';
  import Lathes from '../components/device/Lathes.svelte';
  import VolumesWarning from '../components/device/VolumesWarning.svelte';
  import Waiting from '../components/device/Waiting.svelte';
  import MissingPrograms from '../components/device/MissingPrograms.svelte';

  type Stage = 'disk' | 'options' | 'confirm' | 'type' | 'progress' | 'done' | 'error';
  const STATIONS = [m.dev_step_disk, m.dev_step_options, m.dev_step_confirm, m.dev_step_progress, m.dev_step_done] as const;
  const STATION: Record<Stage, number> = { disk: 1, options: 2, confirm: 3, type: 3, progress: 4, error: 4, done: 5 };
  const OS_NAME: Record<OsId, string> = { linux: 'Linux', windows: 'Windows', macos: 'macOS' };
  const FS_NAME: Record<DataFsId, string> = { exfat: 'exFAT', ntfs: 'NTFS', ext4: 'ext4', btrfs: 'btrfs', apfs: 'APFS' };
  const FS_DESC: Record<DataFsId, () => string> = { exfat: m.dev_fs_exfat_d, ntfs: m.dev_fs_ntfs_d, ext4: m.dev_fs_ext4_d, btrfs: m.dev_fs_btrfs_d, apfs: m.dev_fs_apfs_d };
  const STEP_LABEL: Record<DeviceStep, () => string> = {
    check: m.dev_s_check,
    download: m.dev_s_download,
    unmount: m.dev_s_unmount,
    partition: m.dev_s_partition,
    format_client: m.dev_s_format_client,
    format_data: m.dev_s_format_data,
    mount: m.dev_s_mount,
    copy: m.dev_s_copy,
    verify: m.dev_s_verify,
    marker: m.dev_s_marker,
    volume: m.dev_s_volume,
  };
  const BUSY: Record<dev.Pending, () => string> = {
    candidates: m.dev_busy_candidates,
    options: m.dev_busy_options,
    plan: m.dev_busy_plan,
    create: m.dev_busy_create,
    resume: m.dev_busy_resume,
    eject: m.dev_busy_eject,
  };
  const REFUSED: Record<string, () => string> = {
    system: m.dev_refused_system,
    internal: m.dev_refused_internal,
    virtual: m.dev_refused_virtual,
    too_small: m.dev_refused_too_small,
    hosts_app: m.dev_refused_hosts_app,
    open_volume: m.dev_refused_open_volume,
    read_only: m.dev_refused_read_only,
  };

  let stage = $state<Stage>('disk');
  let disks = $state<DiskCandidate[]>([]);
  let loaded = $state(false);
  let selectedId = $state<string | null>(null);
  const selected = $derived(disks.find((d) => d.id === selectedId) ?? null);
  let options = $state<DeviceOptions | null>(null);
  let table = $state<TableId>('mbr');
  let fs = $state<DataFsId>('exfat');
  let download = $state(true);
  let firstVolume = $state(false);
  let mode = $state<VolumeMode>('whole');
  let volName = $state(m.create_default_name());
  let sizeGb = $state('');
  let fixed = $state(true);
  let plan = $state<DevicePlan | null>(null);
  let showCommands = $state(false);
  let typed = $state('');
  let job = $state<string | null>(null);
  let steps = $state<DeviceStep[]>([]);
  let current = $state<DeviceStep | null>(null);
  let bytes = $state<{ done: number; total: number }>({ done: 0, total: 0 });
  let failure = $state<ApiError | null>(null);
  let outcome = $state<DeviceOutcome | null>(null);
  let error = $state<string | null>(null);
  // what the wizard is waiting on: one call at a time; `token` drops an abandoned call's answer
  let pending = $state<dev.Pending | null>(null);
  let token = 0;
  let since = $state(0);
  let now = $state(Date.now());
  let retry: (() => void) | null = $state(null);
  let listing = $state(false);
  let listSince = 0;
  let stepSince = $state(0);
  let saving = $state(false);
  const busy = $derived(pending !== null || saving);
  const stuckMs = switches.device_stuck_ms ?? STUCK_MS;
  /** the erase also needs this when the drive holds NiceVault volumes */
  let ack = $state(false);
  let ejected = $state(false);
  // the first volume, on Fatto
  let password = $state('');
  let confirm = $state('');
  let strength = $state<Strength | null>(null);
  let created = $state<VaultInfo | null>(null);

  const unnamed = $derived(m.dev_unnamed());
  const fsOpt = $derived(options?.fs.find((f) => f.id === fs) ?? null);
  const gptForced = $derived(!!options?.gpt_required || fs === 'apfs');
  const dataSize = $derived(options?.data_size ?? 0);
  const pieceBytes = $derived(dev.gbToBytes(sizeGb));
  const pieceOk = $derived(mode === 'whole' || !fixed || (pieceBytes !== null && pieceBytes >= 2 ** 30 && pieceBytes <= dataSize - 64 * 2 ** 20));
  const nameOk = $derived(volName.trim() !== '' && !/[\\/:*?"<>|\0]/.test(volName.trim()));
  const optionsOk = $derived(!!fsOpt?.creatable && (!firstVolume || (nameOk && pieceOk)));
  const typedOk = $derived(!!plan && dev.confirmMatches(plan.confirm_name, typed));
  const volumes = $derived(plan?.volumes ?? []);
  const eraseOk = $derived(typedOk && (volumes.length === 0 || ack));
  const authWait = $derived(stage === 'progress' && dev.waitingFor(current));
  /** the footer's status line: what is being waited on, and since when */
  const wait = $derived.by((): { label: string; since: number; abandon: boolean } | null => {
    if (pending) return { label: BUSY[pending](), since, abandon: pending === 'options' || pending === 'plan' || pending === 'candidates' };
    if (stage === 'progress' && authWait) return { label: m.dev_auth_wait(), since: stepSince, abandon: false };
    if (stage === 'progress' && current) return { label: `${STEP_LABEL[current]()}…`, since: stepSince, abandon: false };
    if (listing) return { label: m.dev_busy_candidates(), since: listSince, abandon: false };
    return null;
  });
  $effect(() => {
    if (!wait) return;
    now = Date.now();
    const i = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(i);
  });
  /** the drive the ledger describes */
  const ledgerDisk = $derived(plan?.disk ?? selected);
  const confirmWord = $derived(m.dev_type_word());
  const finished = $derived(stage === 'done' ? 'ok' : stage === 'error' ? 'error' : null);
  const states = $derived(dev.stepStates(steps, current, finished));
  const pct = $derived(bytes.total > 0 ? Math.min(100, Math.round((bytes.done / bytes.total) * 100)) : null);
  const canCancel = $derived(stage === 'progress' && dev.cancellable(current));
  const volSize = $derived(outcome && plan?.first_volume ? dev.volumeSize(plan.first_volume, outcome) : null);
  const volOk = $derived(!!strength?.acceptable && password === confirm && !busy);
  const mismatch = $derived(confirm.length > 0 && confirm !== password);

  const listOf = (os: OsId[]) => (os.length === 2 ? m.dev_list_two({ a: OS_NAME[os[0]], b: OS_NAME[os[1]] }) : os.map((o) => OS_NAME[o]).join(', '));

  function begin_(kind: dev.Pending, again: (() => void) | null): number {
    error = null;
    pending = kind;
    since = now = Date.now();
    retry = again;
    return ++token;
  }
  function end_(t: number) {
    if (t !== token) return;
    pending = null;
    retry = null;
  }
  /** Drop what is being waited on (its answer, if it ever comes, is ignored). */
  function abandon() {
    token++;
    pending = null;
    retry = null;
  }

  async function loadDisks(show = false) {
    if (show || !loaded) {
      listing = true;
      listSince = Date.now();
    }
    try {
      disks = await dev.deviceCandidates();
    } catch (e) {
      error = errorText(e);
    } finally {
      loaded = true;
      listing = false;
    }
    if (selectedId && !disks.some((d) => d.id === selectedId)) selectedId = null;
  }

  onMount(() => {
    preloadStrength();
    let off: (() => void)[] = [];
    void (async () => {
      off.push(await api.onDrivesChanged(() => stage === 'disk' && void loadDisks()));
      off.push(await api.onJobProgress((p) => progressEvent(p as unknown as DeviceProgress)));
      off.push(await api.onJobDone((d) => doneEvent(d as unknown as DeviceDone)));
      await loadDisks();
      if (!api.isTauri && switches.device_step) await jumpTo(switches.device_step);
      if (!api.isTauri && switches.device_slow === 'options' && !switches.device_step && disks[0]) {
        selectedId = disks[0].id;
        void toOptions();
      } else if (!api.isTauri && switches.device_slow === 'plan' && switches.device_step === 'options') void toConfirm();
      else if (!api.isTauri && switches.device_slow === 'create' && switches.device_step === 'type' && plan) {
        typed = plan.confirm_name || confirmWord;
        ack = true;
        void start();
      }
    })();
    return () => off.forEach((f) => f());
  });

  /** Mock: show any step directly (screenshots, e2e). */
  async function jumpTo(s: NonNullable<typeof switches.device_step>) {
    if (s === 'disk' || !disks[0]) return;
    selectedId = disks[0].id;
    await toOptions();
    if (s === 'options') return;
    firstVolume = true;
    await toConfirm();
    if (s === 'confirm') return;
    stage = 'type';
    if (s === 'type') return;
    if (s === 'progress') {
      typed = plan?.confirm_name || confirmWord;
      ack = true;
      await start();
    } else if (s === 'error') {
      steps = plan!.steps;
      current = 'partition';
      failure = { code: 'elevation_denied', message: '' };
      stage = 'error';
    } else if (plan) {
      steps = plan.steps;
      current = 'marker';
      outcome = {
        disk_id: plan.disk.id,
        client_mount: `/run/media/${switches.demo ? 'alex' : 'utente'}/NiceVault`,
        data_mount: `/run/media/${switches.demo ? 'alex' : 'utente'}/VaultData`,
        data_fs: plan.data_fs,
        data_free: plan.data_size,
        whole_volume_size: Math.floor((plan.data_size - 160 * 2 ** 20) / 2 ** 20) * 2 ** 20,
        missing: switches.device_offline ? ['windows', 'macos'] : [],
        first_volume: plan.first_volume,
        log: '',
      };
      stage = 'done';
    }
  }

  async function toOptions() {
    if (!selected || pending) return;
    const t = begin_('options', () => void toOptions());
    try {
      const o = await dev.deviceOptions(selected.id);
      if (t !== token) return;
      options = o;
      table = o.default_table;
      const first = o.fs.find((f) => f.id === 'exfat' && f.creatable) ?? o.fs.find((f) => f.creatable);
      fs = first?.id ?? 'exfat';
      stage = 'options';
    } catch (e) {
      if (t === token) error = refusalText(e);
    } finally {
      end_(t);
    }
  }

  async function toConfirm() {
    if (!selected || !options || pending) return;
    const t = begin_('plan', () => void toConfirm());
    try {
      const p = await dev.devicePlan({
        id: selected.id,
        table: gptForced ? 'gpt' : table,
        data_fs: fs,
        download,
        first_volume: firstVolume ? { mode, size: mode === 'piece' && fixed ? pieceBytes : null, name: volName.trim() } : null,
      });
      if (t !== token) return;
      plan = p;
      typed = '';
      ack = false;
      showCommands = false;
      stage = 'confirm';
    } catch (e) {
      if (t === token) error = refusalText(e);
    } finally {
      end_(t);
    }
  }

  function refusalText(e: unknown): string {
    const er = e as ApiError;
    if (errorCode(e) === 'device_refused' && REFUSED[er.message]) return REFUSED[er.message]();
    if (errorCode(e) === 'fs_unsupported') {
      const [, tool] = String(er.message).split(':');
      return tool ? m.dev_missing_tool({ tool, pkg: options?.fs.find((f) => f.missing_tool === tool)?.package ?? tool }) : m.dev_not_here();
    }
    return errorText(e);
  }

  // events can arrive before the command that started the job returns its id: kept, then replayed
  const early = new Map<string, (DeviceProgress | DeviceDone)[]>();

  /** The progress view at once (before the command answers): the system may ask for a password. */
  function showProgress(list: DeviceStep[]) {
    job = null;
    steps = list;
    current = null;
    stepSince = Date.now();
    bytes = { done: 0, total: 0 };
    failure = null;
    outcome = null;
    stage = 'progress';
  }

  function begin(p: DevicePlan | null, id: string, preset?: DeviceStep[]) {
    job = id;
    if (!steps.length || preset) steps = preset ?? p?.steps ?? [];
    stage = 'progress';
    for (const e of early.get(id) ?? []) ('step' in e ? progressEvent(e) : doneEvent(e as DeviceDone));
    early.clear();
  }

  function keep(e: DeviceProgress | DeviceDone) {
    const l = early.get(e.job) ?? [];
    l.push(e);
    early.set(e.job, l);
  }

  // the job starts as device_create / device_resume answer: never abandoned (no stale token)
  async function start() {
    if (!plan || !eraseOk || pending) return;
    const p = plan;
    const t = begin_('create', null);
    showProgress(p.steps);
    try {
      begin(p, await dev.deviceCreate(p.id, typed));
    } catch (e) {
      error = errorText(e);
      stage = 'type';
      // the drive changed or went: back to the list, nothing was touched
      if (['device_changed', 'device_not_found', 'device_refused', 'not_found'].includes(errorCode(e) ?? '')) {
        stage = 'disk';
        await loadDisks();
      }
    } finally {
      end_(t);
    }
  }

  /** "Completa": a device whose creation stopped midway (no reformat). */
  async function resume(d: DiskCandidate) {
    if (pending) return;
    const t = begin_('resume', null);
    selectedId = d.id;
    const list: DeviceStep[] = d.nicevault === 'complete' ? ['check', 'mount'] : ['check', 'download', 'mount', 'copy', 'verify', 'marker'];
    plan = null;
    showProgress(list);
    try {
      begin(null, await dev.deviceResume(d.id), list);
    } catch (e) {
      error = refusalText(e);
      stage = 'disk';
    } finally {
      end_(t);
    }
  }

  function progressEvent(p: DeviceProgress) {
    if (!p.step) return;
    if (p.job !== job) return keep(p);
    if (!steps.includes(p.step)) steps = [...steps, p.step];
    if (current !== p.step) stepSince = Date.now();
    current = p.step;
    bytes = { done: p.done_bytes, total: p.total_bytes };
  }

  function doneEvent(d: DeviceDone) {
    if (d.job !== job) return keep(d);
    if (d.error) {
      failure = typeof d.error === 'string' ? { code: 'error', message: d.error } : d.error;
      stage = 'error';
      void loadDisks();
      return;
    }
    outcome = d.outcome;
    current = steps.includes('marker') ? 'marker' : current;
    stage = 'done';
  }

  async function cancel() {
    if (job && canCancel) await api.call('job_cancel', { job });
  }

  async function copyLog() {
    if (!job) return;
    try {
      const text = await dev.deviceLog(job);
      await navigator.clipboard.writeText(text);
      toasts.push(m.dev_log_copied());
    } catch (e) {
      toasts.error(e);
    }
  }

  async function createVolume(e?: Event) {
    e?.preventDefault();
    if (!outcome || !plan?.first_volume || !volOk) return;
    saving = true;
    error = null;
    try {
      created = await vaults.createVault(dev.volumePath(outcome.data_mount, plan.first_volume.name), password, volSize);
      password = confirm = '';
    } catch (err) {
      error = errorText(err);
    } finally {
      saving = false;
    }
  }

  async function eject() {
    if ((!outcome && !selectedId) || pending) return;
    const t = begin_('eject', null);
    try {
      await dev.deviceEject(outcome?.disk_id ?? selectedId!);
      ejected = true;
    } catch (e) {
      error = errorText(e);
    } finally {
      end_(t);
    }
  }

  /** Indietro / Annulla / Esc: always usable before the erase; a pending call is dropped. */
  function back() {
    abandon();
    error = null;
    if (stage === 'options') stage = 'disk';
    else if (stage === 'confirm') stage = 'options';
    else if (stage === 'type') stage = 'confirm';
    else if (stage === 'error') {
      stage = 'disk';
      void loadDisks();
    } else app.goto(vaults.open.length ? 'vault' : 'welcome');
  }

  function finish() {
    if (created && vaults.open.some((v) => v.id === created!.id)) {
      vaults.activate(created.id);
      app.goto('vault');
    } else app.goto('welcome');
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !saving && stage !== 'progress' && stage !== 'done') {
      e.preventDefault();
      back();
    }
  }

  const diskIcon = (d: DiskCandidate) => (d.nicevault !== 'none' ? ShieldCheck : d.bus === 'usb' ? Usb : HardDrive);
</script>

<svelte:window onkeydown={keydown} />

<main class="wizard ground" data-stage={stage} data-pending={pending ?? undefined} data-confirm-name={plan ? plan.confirm_name || confirmWord : undefined} aria-labelledby="device-title" data-testid="device-wizard">
  <article class="form-sheet sheet">
    <Stitch inset={6} />
    <Band height={9} />
    <header class="head">
      <h1 id="device-title" class="title display">{m.dev_title()}</h1>
      <ol class="steps" aria-label={m.create_steps()}>
        {#each STATIONS as label, i (i)}
          {@const n = STATION[stage]}
          <li class="step" data-state={i + 1 < n ? 'done' : i + 1 === n ? 'current' : 'todo'} aria-current={i + 1 === n ? 'step' : undefined}>
            <span class="step-n"><span class="step-digit">{i + 1}</span><Check size={12} strokeWidth={2.6} aria-hidden="true" class="step-check" /></span>
            <span class="step-label">{label()}</span>
          </li>
        {/each}
      </ol>
    </header>

    <div class="frame scroller">
      <div class="columns" data-ledger={stage !== 'done'}>
        {#if stage !== 'done'}
          <aside class="ledger" aria-label={m.dev_ledger()} data-testid="device-ledger">
            {#if stage === 'disk' || !ledgerDisk}
              <p class="caps">{m.dev_entry()}</p>
              <p class="lead">{m.dev_entry_hint()}</p>
              <ul class="parts new">
                <li><span class="p-label">NiceVault</span><span class="mono">exfat</span><span class="num">128 MB</span></li>
                <li><span class="p-label">VaultData</span><span class="mono">…</span><span class="num">{m.dev_vol_mode_whole().toLowerCase()}</span></li>
              </ul>
            {:else}
              {@const Icon = diskIcon(ledgerDisk)}
              <p class="caps">{m.dev_ledger_drive()}</p>
              <div class="l-drive">
                <span class="glyph" aria-hidden="true"><Icon size={18} strokeWidth={1.6} /></span>
                <span class="l-name">{dev.diskName(ledgerDisk, unnamed)}</span>
                <span class="l-line mono">{dev.diskLine(ledgerDisk, unnamed).split(' · ').slice(1).join(' · ')}</span>
              </div>
              <ul class="parts">
                {#each ledgerDisk.partitions as p (p.path)}
                  <li><span class="p-label">{p.label || '—'}</span><span class="mono">{p.fs || '?'}</span><span class="num">{dev.formatDiskSize(p.size)}</span></li>
                {:else}
                  <li class="none">{m.dev_no_partitions()}</li>
                {/each}
              </ul>
              {#if options || plan}
                <p class="caps">{m.dev_ledger()}</p>
                <dl class="l-choices">
                  <dt>VaultData</dt>
                  <dd class="mono">{FS_NAME[plan?.data_fs ?? fs]}</dd>
                  <dt>{m.dev_table()}</dt>
                  <dd class="mono">{(plan?.table ?? (gptForced ? 'gpt' : table)).toUpperCase()}</dd>
                  <dt>{m.dev_ledger_programs()}</dt>
                  <dd>{(plan?.download ?? download) ? m.dev_ledger_programs_download() : m.dev_ledger_programs_local()}</dd>
                  <dt>{m.dev_s_volume()}</dt>
                  <dd>{plan ? (plan.first_volume?.name ?? m.dev_ledger_volume_none()) : firstVolume ? volName : m.dev_ledger_volume_none()}</dd>
                </dl>
              {/if}
            {/if}
          </aside>
        {/if}

        <div class="pane">
          <svelte:boundary>
            <div class="body">
              {#if stage === 'disk'}
                <div class="field">
                  <div class="row-head">
                    <span class="field-label" id="dev-disks-label">{m.dev_choose()}</span>
                    <button type="button" class="btn ghost sm" onclick={() => loadDisks(true)} aria-label={m.dev_refresh()} title={m.dev_refresh()}><RefreshCw size={14} aria-hidden="true" /></button>
                  </div>
                  {#if loaded && disks.length === 0}
                    <p class="empty" data-testid="device-empty"><Usb size={18} aria-hidden="true" /> {m.dev_empty()}</p>
                  {:else}
                    <div class="disks" role="radiogroup" aria-labelledby="dev-disks-label" aria-busy={!loaded} data-testid="device-disks">
                      {#each disks as d (d.id)}
                        {@const Icon = diskIcon(d)}
                        <div class="disk-wrap">
                          <button type="button" role="radio" class="disk" aria-checked={selectedId === d.id} data-testid="device-disk" data-disk-id={d.id} onclick={() => (selectedId = d.id)}>
                            <span class="glyph" aria-hidden="true"><Icon size={18} strokeWidth={1.6} /></span>
                            <span class="d-name">{dev.diskName(d, unnamed)}</span>
                            <span class="d-line mono">{dev.diskLine(d, unnamed).split(' · ').slice(1).join(' · ')}</span>
                            <span class="tags">
                              {#if d.nicevault === 'complete'}<span class="badge">{m.dev_badge_complete()}</span>{/if}
                              {#if d.nicevault === 'incomplete'}<span class="badge" data-tone="danger">{m.dev_badge_incomplete()}</span>{/if}
                            </span>
                            <ul class="parts" aria-label={m.dev_partitions()}>
                              {#each d.partitions as p (p.path)}
                                <li><span class="p-label">{p.label || '—'}</span><span class="mono">{p.fs || '?'}</span><span class="num">{dev.formatDiskSize(p.size)}</span></li>
                              {:else}
                                <li class="none">{m.dev_no_partitions()}</li>
                              {/each}
                            </ul>
                          </button>
                          {#if d.nicevault === 'incomplete'}
                            <div class="resume">
                              <p class="hint">{m.dev_complete_hint()}</p>
                              <button type="button" class="btn sm" disabled={busy} onclick={() => resume(d)} data-testid="device-complete">{m.dev_complete()}</button>
                            </div>
                          {/if}
                        </div>
                      {/each}
                    </div>
                  {/if}
                  <p class="hint">{m.dev_only_external()}</p>
                </div>
              {:else if stage === 'options' && options && selected}
                <p class="chosen mono">{dev.diskLine(selected, unnamed)}</p>
                <div class="field">
                  <span class="field-label" id="dev-fs-label">{m.dev_fs()}</span>
                  <p class="hint">{m.dev_fs_hint()}</p>
                  <div class="fs-list" role="radiogroup" aria-labelledby="dev-fs-label" data-testid="device-fs">
                    {#each options.fs as f (f.id)}
                      {@const no = dev.unreadableOn(f)}
                      <button type="button" role="radio" class="fs" aria-checked={fs === f.id} disabled={!f.creatable} data-fs={f.id} onclick={() => (fs = f.id)}>
                        <span class="fs-name">{FS_NAME[f.id]}</span>
                        <span class="fs-desc">{FS_DESC[f.id]()}</span>
                        <span class="readable" aria-label={m.dev_readable_on()}>
                          {#each f.readable_on as [os, a] (os)}
                            <span class="os" data-access={a} title={a === 'rw' ? m.dev_access_rw() : a === 'ro' ? m.dev_access_ro() : m.dev_access_no()}>{OS_NAME[os]}{#if a === 'ro'}<span class="ro"> · {m.dev_access_ro_short()}</span>{/if}</span>
                          {/each}
                        </span>
                        {#if !f.creatable}
                          <span class="hint fs-why">{f.missing_tool ? m.dev_missing_tool({ tool: f.missing_tool, pkg: f.package ?? f.missing_tool }) : m.dev_not_here()}</span>
                        {:else if no.length}
                          <span class="badge" data-tone="danger"><TriangleAlert size={11} aria-hidden="true" />{m.dev_unreadable({ list: listOf(no) })}</span>
                        {/if}
                      </button>
                    {/each}
                  </div>
                </div>

                <div class="opt-grid">
                  <div class="field">
                    <span class="field-label" id="dev-table-label">{m.dev_table()}</span>
                    <div class="segmented" role="radiogroup" aria-labelledby="dev-table-label" data-testid="device-table">
                      {#each ['mbr', 'gpt'] as const as t (t)}
                        <button type="button" role="radio" aria-checked={(gptForced ? 'gpt' : table) === t} disabled={gptForced && t === 'mbr'} onclick={() => (table = t)}>{t.toUpperCase()}</button>
                      {/each}
                    </div>
                    <p class="hint">{gptForced ? (fs === 'apfs' ? m.dev_gpt_apfs() : m.dev_gpt_required()) : table === 'mbr' ? m.dev_table_mbr_hint() : m.dev_table_gpt_hint()}</p>
                  </div>

                  <div class="field">
                    <span class="field-label">{m.dev_sources()}</span>
                    <div class="switch-row">
                      <span class="switch-label" id="dev-download">{m.dev_download()}</span>
                      <button type="button" class="switch" role="switch" aria-checked={download} aria-labelledby="dev-download" data-testid="device-download" onclick={() => (download = !download)}><span class="knob"></span></button>
                    </div>
                    <p class="hint">{download ? m.dev_sources_hint({ host: options.update_host }) : m.dev_sources_local()}</p>
                    <p class="sources mono" data-testid="device-sources">
                      {options.sources.map((s) => (download ? m.dev_src_download({ os: OS_NAME[s.os] }) : s.local ? m.dev_src_local({ os: OS_NAME[s.os] }) : m.dev_src_missing({ os: OS_NAME[s.os] }))).join(' · ')}
                    </p>
                  </div>
                </div>

                <div class="field">
                  <div class="switch-row">
                    <span class="switch-label" id="dev-first">{m.dev_first_volume()}</span>
                    <button type="button" class="switch" role="switch" aria-checked={firstVolume} aria-labelledby="dev-first" data-testid="device-first-volume" onclick={() => (firstVolume = !firstVolume)}><span class="knob"></span></button>
                  </div>
                  {#if firstVolume}
                    <div class="segmented" role="radiogroup" aria-label={m.dev_first_volume()} data-testid="device-volume-mode">
                      <button type="button" role="radio" aria-checked={mode === 'whole'} onclick={() => (mode = 'whole')}>{m.dev_vol_mode_whole()}</button>
                      <button type="button" role="radio" aria-checked={mode === 'piece'} onclick={() => (mode = 'piece')}>{m.dev_vol_mode_piece()}</button>
                    </div>
                    <p class="hint">{mode === 'whole' ? m.dev_vol_mode_whole_d({ size: formatBytes(dataSize) }) : m.dev_vol_mode_piece_d()}</p>
                    <div class="vol-grid">
                      <div class="field">
                        <label for="dev-vol-name">{m.dev_vol_name()}</label>
                        <input id="dev-vol-name" class="input" bind:value={volName} autocomplete="off" spellcheck="false" aria-invalid={!nameOk} />
                      </div>
                      {#if mode === 'piece'}
                        <div class="field">
                          <label for="dev-vol-size">{m.dev_vol_size()}</label>
                          <input id="dev-vol-size" class="input num" inputmode="decimal" bind:value={sizeGb} disabled={!fixed} placeholder="8" aria-invalid={!pieceOk} data-testid="device-volume-size" />
                        </div>
                      {/if}
                    </div>
                    {#if mode === 'piece'}
                      <div class="switch-row">
                        <span class="switch-label" id="dev-fixed">{m.dev_vol_fixed()}</span>
                        <button type="button" class="switch" role="switch" aria-checked={fixed} aria-labelledby="dev-fixed" onclick={() => (fixed = !fixed)}><span class="knob"></span></button>
                      </div>
                      <p class={pieceOk ? 'hint' : 'error-text'}>{!pieceOk ? m.dev_vol_size_bad({ max: dev.formatDiskSize(dataSize) }) : fixed ? m.dev_vol_fixed_hint_on() : m.dev_vol_fixed_hint()}</p>
                    {/if}
                  {/if}
                </div>
              {:else if (stage === 'confirm' || stage === 'type') && plan}
                {#if volumes.length && stage === 'confirm'}<VolumesWarning {volumes} bind:ack />{/if}
                <section class="summary" aria-labelledby="dev-sum" data-testid="device-summary">
                  <p id="dev-sum" class="erase">
                    <CircleAlert size={18} aria-hidden="true" />
                    <span>{m.dev_erase_a()} <strong>{dev.diskName(plan.disk, unnamed)} ({dev.formatDiskSize(plan.disk.size)}, {dev.busLabel(plan.disk.bus)})</strong> {m.dev_erase_b()}</span>
                  </p>
                  <div class="layouts">
                    <div>
                      <p class="caps">{m.dev_now()}</p>
                      <ul class="parts">
                        {#each plan.disk.partitions as p (p.path)}
                          <li><span class="p-label">{p.label || '—'}</span><span class="mono">{p.fs || '?'}</span><span class="num">{dev.formatDiskSize(p.size)}</span></li>
                        {:else}
                          <li class="none">{m.dev_no_partitions()}</li>
                        {/each}
                      </ul>
                      {#if plan.disk.serial_tail}<p class="hint mono">S/N …{plan.disk.serial_tail}</p>{/if}
                    </div>
                    <ChevronRight size={18} aria-hidden="true" class="arrow" />
                    <div>
                      <p class="caps">{m.dev_after()} · {plan.table.toUpperCase()}</p>
                      <ul class="parts new">
                        <li><span class="p-label">NiceVault</span><span class="mono">exfat</span><span class="num">128 MB</span></li>
                        <li><span class="p-label">VaultData</span><span class="mono">{plan.data_fs}</span><span class="num">{dev.formatDiskSize(plan.data_size)}</span></li>
                      </ul>
                      {#if plan.first_volume}<p class="hint">{m.dev_after_volume({ name: plan.first_volume.name })}</p>{/if}
                    </div>
                  </div>
                  <button type="button" class="btn ghost sm disclose" aria-expanded={showCommands} onclick={() => (showCommands = !showCommands)} data-testid="device-show-commands">
                    {#if showCommands}<ChevronDown size={14} aria-hidden="true" />{:else}<ChevronRight size={14} aria-hidden="true" />{/if}
                    {m.dev_show_commands()}
                  </button>
                  {#if showCommands}
                    <pre class="commands mono" data-testid="device-commands">{plan.commands_preview.join('\n')}</pre>
                  {/if}
                </section>
                {#if stage === 'type'}
                  <!-- next to the typed name: the erase also waits on this consent -->
                  {#if volumes.length}<VolumesWarning {volumes} bind:ack />{/if}
                  <form class="field typed" onsubmit={(e) => (e.preventDefault(), start())}>
                    <label for="dev-typed">{m.dev_type_prompt({ name: plan.confirm_name || confirmWord })}</label>
                    <!-- svelte-ignore a11y_autofocus -->
                    <input id="dev-typed" class="input" bind:value={typed} autocomplete="off" spellcheck="false" autofocus data-testid="device-typed" />
                    <p class="hint">{m.dev_permission_note()}</p>
                    <p class="hint auth-ahead"><ShieldCheck size={13} aria-hidden="true" /> {m.dev_auth_ahead()}</p>
                  </form>
                {/if}
              {:else if stage === 'progress' || stage === 'error' || stage === 'done'}
                {#if authWait}<AuthCard />{/if}
                {#if stage !== 'done' || !plan?.first_volume}
                  <ol class="progress-steps" aria-label={m.dev_step_progress()} data-testid="device-steps">
                    {#each steps as s, i (s)}
                      <li data-state={states[i]} data-step={s}>
                        <span class="dot" aria-hidden="true">
                          {#if states[i] === 'done'}<Check size={12} strokeWidth={2.6} />{:else if states[i] === 'current'}<LoaderCircle size={13} class="spin" />{:else if states[i] === 'failed'}<X size={12} strokeWidth={2.6} />{/if}
                        </span>
                        <span class="s-label">{STEP_LABEL[s]()}</span>
                        {#if states[i] === 'current' && (s === 'download' || s === 'copy') && bytes.total > 0}
                          <span class="num s-bytes">{formatBytes(bytes.done)} / {formatBytes(bytes.total)}</span>
                        {/if}
                      </li>
                    {/each}
                  </ol>
                  {#if stage === 'progress'}
                    <progress max="100" value={pct ?? undefined} aria-label={m.dev_step_progress()}></progress>
                    {#if !canCancel}<p class="hint">{m.dev_cancel_late()}</p>{/if}
                  {/if}
                {/if}
                {#if stage === 'error' && failure}
                  <div class="failed" role="alert" data-testid="device-error" data-code={failure.code}>
                    <p class="f-title">{m.dev_failed_title()}</p>
                    <p>{failure.code === 'device_refused' && REFUSED[failure.message] ? REFUSED[failure.message]() : errorText(failure)}</p>
                    <button type="button" class="btn sm" onclick={copyLog} data-testid="device-copy-log">{m.dev_copy_log()}</button>
                  </div>
                {/if}
                {#if stage === 'done' && outcome}
                  <div class="done" role="status" data-testid="device-done">
                    <span class="done-seal" aria-hidden="true"><Rosette size={92} kind="seal"><ShieldCheck size={24} strokeWidth={1.4} /></Rosette></span>
                    <p class="done-title display">{m.dev_done_title()}</p>
                    <p class="hint">{m.dev_done_hint()}</p>
                    <MissingPrograms {job} bind:outcome list={listOf(outcome.missing)} />
                    {#if ejected}<p class="hint" data-testid="device-ejected">{m.dev_ejected()}</p>{/if}
                  </div>
                  {#if plan?.first_volume && !ejected}
                    {#if created}
                      <p class="hint path" data-testid="device-volume-made">{m.dev_volume_made()} {created.path}</p>
                    {:else}
                      <form class="volume" onsubmit={createVolume} data-testid="device-volume">
                        <p class="caps">{m.dev_s_volume()}</p>
                        <p class="hint path">{dev.volumePath(outcome.data_mount, plan.first_volume.name)} · {volSize ? formatBytes(volSize) : m.dev_volume_grows()}</p>
                        <PasswordField bind:value={password} label={m.password()} autocomplete="new-password" describedby="dev-pw-strength" disabled={busy} />
                        <StrengthMeter id="dev-pw-strength" {password} userInputs={[plan.first_volume.name, 'nicevault']} bind:strength />
                        <PasswordField bind:value={confirm} label={m.create_repeat()} autocomplete="new-password" invalid={mismatch} disabled={busy} />
                        {#if mismatch}<p class="error-text">{m.passwords_mismatch()}</p>{/if}
                        <p class="warn">{m.create_warn()}</p>
                        <button type="submit" class="btn primary" disabled={!volOk} data-testid="device-volume-create">{saving ? m.create_busy() : m.dev_volume_create()}</button>
                      </form>
                    {/if}
                  {/if}
                {/if}
              {/if}
            </div>
            {#snippet failed(e, reset)}
              <div class="failed" role="alert" data-testid="device-render-error">
                <p class="f-title">{m.dev_render_failed()}</p>
                <p class="mono">{errorText(e)}</p>
                <button type="button" class="btn sm" onclick={() => (back(), reset())}><ArrowLeft size={14} aria-hidden="true" /> {m.action_back()}</button>
              </div>
            {/snippet}
          </svelte:boundary>
        </div>
      </div>
    </div>

    <footer class="nav">
      {#if stage === 'disk' || stage === 'options' || stage === 'confirm' || stage === 'type'}
        <button type="button" class="btn ghost" onclick={back} disabled={saving} data-testid="device-back">
          <ArrowLeft size={16} aria-hidden="true" />
          {stage === 'disk' ? m.action_cancel() : m.action_back()}
        </button>
      {:else if stage === 'progress'}
        <button type="button" class="btn ghost" onclick={cancel} disabled={!canCancel || !job} data-testid="device-cancel">{m.action_cancel()}</button>
      {:else if stage === 'error'}
        <button type="button" class="btn ghost" onclick={back} data-testid="device-back-to-disks"><ArrowLeft size={16} aria-hidden="true" /> {m.dev_back_to_disks()}</button>
      {/if}
      <div class="status">
        {#if error}
          <p class="error-text" role="alert" data-testid="device-form-error"><CircleAlert size={15} aria-hidden="true" /> {error}</p>
        {:else if stage === 'type' && typedOk && !eraseOk}
          <button type="button" class="need-ack hint" onclick={() => document.querySelector<HTMLElement>('[data-testid=device-volumes-ack]')?.scrollIntoView({ block: 'center' })} data-testid="device-need-ack">{m.dev_volumes_need_ack()}</button>
        {:else if wait}
          <Waiting
            label={wait.label}
            elapsed={Math.max(0, now - wait.since)}
            {stuckMs}
            oncancel={wait.abandon ? abandon : undefined}
            onretry={wait.abandon && retry ? () => { const again = retry; abandon(); again?.(); } : undefined}
          />
        {/if}
      </div>
      {#if stage === 'disk'}
        <button type="button" class="btn primary" disabled={!selected || busy} aria-busy={pending === 'options'} onclick={toOptions} data-testid="device-next">
          {#if pending === 'options'}<Lathes size={16} /> {m.dev_busy_options()}{:else}{m.create_next()}{/if}
        </button>
      {:else if stage === 'options'}
        <button type="button" class="btn primary" disabled={!optionsOk || busy} aria-busy={pending === 'plan'} onclick={toConfirm} data-testid="device-next">
          {#if pending === 'plan'}<Lathes size={16} /> {m.dev_busy_plan()}{:else}{m.create_next()}{/if}
        </button>
      {:else if stage === 'confirm'}
        <button type="button" class="btn primary" onclick={() => (stage = 'type')} data-testid="device-continue">{m.dev_continue()}</button>
      {:else if stage === 'type'}
        <button type="button" class="btn danger" disabled={!eraseOk || busy} onclick={start} data-testid="device-erase">{m.dev_erase_create()}</button>
      {:else if stage === 'done' && outcome}
        {#if !ejected}
          <button type="button" class="btn" onclick={() => job && dev.deviceOpen(job)} disabled={!job} data-testid="device-open-data">{m.dev_open_data()}</button>
          <button type="button" class="btn" onclick={eject} disabled={busy} aria-busy={pending === 'eject'} data-testid="device-eject">
            {#if pending === 'eject'}<Lathes size={16} /> {m.dev_busy_eject()}{:else}{m.dev_eject()}{/if}
          </button>
        {/if}
        <button type="button" class="btn primary" onclick={finish} data-testid="device-finish">{created ? m.dev_volume_open() : m.dev_finish()}</button>
      {/if}
    </footer>
  </article>
</main>

<style>
  /* a full-window sheet on the board: head, the step (scrolls), the footer with the status line
     (always on screen). Wide: the ledger on the left, the step on the right. */
  .wizard {
    position: relative;
    z-index: 2;
    height: 100%;
    display: flex;
    padding: clamp(8px, 1.6vw, 22px);
    overflow: hidden;
  }
  .form-sheet {
    --pad: clamp(16px, 3.2vw, 44px);
    --bleed-start: var(--pad);
    --bleed-end: var(--pad);
    container: sheet / inline-size;
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr) auto;
    padding: 16px var(--pad) 0;
    border-radius: var(--radius-stitched);
    box-shadow: var(--shadow-sheet);
  }
  /* the first step is a short list: the sheet closes round it (centred on the board) instead of
     leaving a blank page under two cards; it grows again, up to the window, with a long list */
  .wizard[data-stage='disk'] .form-sheet {
    flex: 0 1 auto;
    align-self: center;
    width: 100%;
    max-height: 100%;
    padding-bottom: 0;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    column-gap: var(--space-6);
    row-gap: var(--space-2);
    margin-top: var(--space-3);
    padding-bottom: var(--space-4);
    border-bottom: 1px solid var(--paper-rule);
  }
  .title {
    font-size: clamp(24px, 3.4cqi, 38px);
    font-weight: 500;
    line-height: 1.15;
    color: var(--title-ink);
  }
  .frame {
    min-height: 0;
    overflow: auto;
    margin-inline: calc(-1 * var(--pad));
    padding: var(--space-5) var(--pad) var(--space-6);
    scrollbar-gutter: stable;
  }
  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    max-width: 920px;
    margin: 0 auto;
  }
  .ledger {
    display: none;
  }
  .pane {
    min-width: 0;
  }
  @container sheet (min-width: 1000px) {
    .columns[data-ledger='true'] {
      grid-template-columns: minmax(250px, 300px) minmax(0, 860px);
      justify-content: center;
      column-gap: clamp(32px, 4cqi, 64px);
      max-width: none;
    }
    .columns[data-ledger='true'] .ledger {
      position: sticky;
      top: 0;
      align-self: start;
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      padding: 16px 18px 18px;
      border-radius: var(--radius-md);
      background: var(--paper-sunk);
      box-shadow:
        inset 0 1px 2px var(--field-shade),
        0 1px 0 var(--paper-hi);
    }
    .columns[data-ledger='true'] .chosen {
      display: none;
    }
    .disks {
      display: grid !important;
      grid-template-columns: repeat(auto-fill, minmax(330px, 1fr));
      align-items: start;
    }
  }
  @container sheet (max-width: 640px) {
    .step:not([data-state='current']) .step-label {
      display: none;
    }
    .step + .step::before {
      width: 12px !important;
    }
    .opt-grid,
    .layouts {
      grid-template-columns: minmax(0, 1fr) !important;
    }
    .layouts :global(.arrow) {
      display: none;
    }
    .vol-grid {
      grid-template-columns: 1fr !important;
    }
    .nav .status {
      order: -1;
      flex-basis: 100%;
    }
  }
  .ledger .caps:not(:first-child) {
    margin-top: var(--space-3);
  }
  .lead {
    font-size: var(--text-md);
    color: var(--ink-2);
  }
  .l-drive {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-areas:
      'glyph name'
      'glyph line';
    column-gap: var(--space-2);
    row-gap: 2px;
    align-items: center;
  }
  .l-name {
    grid-area: name;
    font-weight: 600;
  }
  .l-line {
    grid-area: line;
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .ledger .parts {
    margin: 0;
  }
  .l-choices {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: var(--space-3);
    row-gap: 6px;
    margin: 0;
    font-size: var(--text-sm);
  }
  .l-choices dt {
    color: var(--ink-3);
  }
  .l-choices dd {
    margin: 0;
    text-align: right;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .opt-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-5);
    align-items: start;
  }
  .auth-ahead {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .auth-ahead :global(svg) {
    flex: none;
    color: var(--accent);
  }
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

  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }
  .body :global(.field) {
    gap: var(--space-2);
  }
  .body :global(.input) {
    height: 38px;
    font-size: var(--text-base);
  }
  .body :global(.hint),
  .body :global(.error-text) {
    overflow-wrap: anywhere;
  }
  .row-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }
  .empty {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-5);
    border-radius: var(--radius-md);
    color: var(--ink-3);
    background: var(--paper-sunk);
    box-shadow: inset 0 1px 2px var(--field-shade);
  }

  /* ---- drives: cards pressed into the paper, like the drive picker ------------- */
  .disks {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .disk-wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .disk {
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-areas:
      'glyph name tags'
      'glyph line line'
      'parts parts parts';
    column-gap: var(--space-2);
    row-gap: 4px;
    align-items: center;
    width: 100%;
    padding: 10px 12px;
    border: 0;
    border-radius: var(--radius-md);
    text-align: left;
    color: var(--ink);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
    transition:
      box-shadow var(--dur-micro) var(--ease-out),
      background var(--dur-micro) var(--ease-out);
  }
  .disk:hover {
    background: color-mix(in oklab, var(--paper-sunk) 85%, var(--ink) 4%);
  }
  .disk[aria-checked='true'] {
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--accent-line),
      0 0 0 3px var(--accent-soft),
      var(--lift-key);
  }
  .glyph {
    grid-area: glyph;
    align-self: start;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    color: var(--ink-3);
    background: var(--paper);
    box-shadow: 0 0 0 1px var(--paper-rule-2);
  }
  .disk[aria-checked='true'] .glyph {
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: 0 0 0 1px var(--accent-line);
  }
  .d-name {
    grid-area: name;
    font-size: var(--text-base);
    font-weight: 600;
  }
  .d-line {
    grid-area: line;
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .tags {
    grid-area: tags;
    display: flex;
    gap: 4px;
  }
  .parts {
    grid-area: parts;
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-xs);
    color: var(--ink-2);
  }
  .parts li {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: var(--space-3);
    padding: 3px 8px;
    border-radius: var(--radius-sm);
    background: color-mix(in oklab, var(--paper) 70%, transparent);
  }
  .parts li.none {
    display: block;
    color: var(--ink-3);
  }
  .parts .p-label {
    font-weight: 500;
  }
  .parts.new li {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .resume {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: 6px 12px;
  }
  .chosen {
    font-size: var(--text-sm);
    color: var(--ink-2);
  }

  /* ---- file systems: one card each, where it can be read --------------------- */
  .fs-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-2);
  }
  .fs {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 10px 12px;
    border: 0;
    border-radius: var(--radius-md);
    text-align: left;
    color: var(--ink);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .fs[aria-checked='true'] {
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--accent-line),
      0 0 0 3px var(--accent-soft),
      var(--lift-key);
  }
  .fs:disabled {
    color: var(--ink-3);
  }
  .fs-name {
    font-weight: 600;
  }
  .fs-desc {
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .readable {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .os {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.04em;
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    box-shadow: inset 0 0 0 1px var(--paper-rule-2);
  }
  .os[data-access='rw'] {
    color: var(--ink);
  }
  .os[data-access='ro'] {
    color: var(--ink-2);
  }
  .os[data-access='no'] {
    color: var(--ink-4);
    text-decoration: line-through;
  }
  .fs-why {
    font-size: var(--text-xs);
  }
  .sources {
    font-size: var(--text-xs);
    color: var(--ink-2);
  }
  .vol-grid {
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: var(--space-3);
  }

  /* ---- confirm ----------------------------------------------------------------- */
  .summary {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .erase {
    display: flex;
    gap: var(--space-2);
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
    color: var(--ink);
    background: var(--danger-wash);
    box-shadow: inset 2px 0 0 var(--danger);
  }
  .erase :global(svg) {
    flex: none;
    color: var(--danger);
    margin-top: 1px;
  }
  .layouts {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: start;
    gap: var(--space-3);
  }
  .layouts :global(.arrow) {
    margin-top: 28px;
    color: var(--ink-3);
  }
  .disclose {
    align-self: flex-start;
  }
  .commands {
    max-height: 220px;
    overflow: auto;
    margin: 0;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--ink-2);
    background: var(--paper-sunk);
    box-shadow: inset 0 1px 2px var(--field-shade);
  }

  /* ---- progress ---------------------------------------------------------------- */
  .progress-steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .progress-steps li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-md);
    color: var(--ink-3);
  }
  .progress-steps li[data-state='current'],
  .progress-steps li[data-state='done'] {
    color: var(--ink);
  }
  .progress-steps li[data-state='failed'] {
    color: var(--danger);
  }
  .dot {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--paper-sunk);
    box-shadow: inset 0 1px 2px var(--field-shade);
  }
  li[data-state='done'] .dot {
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  li[data-state='current'] .dot {
    color: var(--accent);
  }
  li[data-state='failed'] .dot {
    color: var(--danger);
    background: var(--danger-wash);
  }
  .dot :global(.spin) {
    animation: spin 1.1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .s-bytes {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .failed {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: 12px 14px;
    border-radius: var(--radius-sm);
    background: var(--danger-wash);
    box-shadow: inset 2px 0 0 var(--danger);
  }
  .f-title {
    font-weight: 600;
  }

  /* ---- done -------------------------------------------------------------------- */
  [data-stage='done'] .columns {
    max-width: 640px;
  }
  .done {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    text-align: center;
  }
  /* the same engraved seal as every finished act (rotate-key, create) */
  .done-seal {
    display: grid;
    color: var(--accent);
  }
  .done-title {
    font-size: 30px;
    color: var(--title-ink);
  }
  .volume {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding-top: var(--space-4);
    border-top: 1px solid var(--paper-rule);
  }
  .volume .btn {
    align-self: flex-end;
  }
  .warn {
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
    color: var(--ink);
    background: var(--accent-soft);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .nav {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) 0 var(--space-4);
    border-top: 1px solid var(--paper-rule);
  }
  .status {
    flex: 1;
    min-width: 0;
    display: flex;
    justify-content: flex-end;
  }
  .status .error-text {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 0;
  }
  .status .error-text :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .need-ack {
    border: 0;
    padding: 0;
    background: none;
    text-align: right;
    color: var(--danger);
    cursor: pointer;
  }
  .nav .btn :global(.lathes) {
    margin-right: 2px;
  }
</style>
