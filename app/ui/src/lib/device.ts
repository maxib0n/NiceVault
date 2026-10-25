// "Crea dispositivo NiceVault": types and wrappers of the
// device_* commands (app/src-tauri/src/device/cmd.rs) and the pure helpers the wizard uses.
//
//   device_candidates                      -> DiskCandidate[]   external drives only, never a system disk
//   device_options { id }                  -> DeviceOptions     file systems (creatable, readable on), sources
//   device_plan { opts: PlanOpts }         -> DevicePlan        layout, steps, the exact commands
//   device_create { plan_id, typed }       -> JobId             typed = the drive name (or CANCELLA / ERASE)
//   device_resume { id }                   -> JobId             "Completa" (no reformat) / outcome of a complete one
//   device_eject { id } · device_log { job } -> string · device_outcome { job } -> DeviceOutcome
//   device_open { job }                    -> ()                opens VaultData in the file manager
//   device_fetch_missing { job }           -> DeviceOutcome     "Scarica ora": downloads the missing programs
// Events: job-progress { job, step, done_bytes, total_bytes } · job-done { job, error, outcome }.

import { call } from './api';
import { formatNumber } from './format';
import type { ApiError, JobId } from './types';

export type OsId = 'linux' | 'windows' | 'macos';
export type DataFsId = 'exfat' | 'ntfs' | 'ext4' | 'btrfs' | 'apfs';
export type TableId = 'mbr' | 'gpt';
export type Access = 'rw' | 'ro' | 'no';
export type NvState = 'none' | 'incomplete' | 'complete';
export type VolumeMode = 'whole' | 'piece';

export interface DiskPartition {
  path: string;
  label: string;
  fs: string;
  size: number;
  mountpoints: string[];
}

/** A whole disk as the backend models it (device/mod.rs Disk). */
export interface Disk {
  id: string;
  os_path: string;
  vendor: string;
  model: string;
  serial: string;
  bus: string;
  size: number;
  removable: boolean;
  partitions: DiskPartition[];
  nicevault: NvState;
}

export interface DiskCandidate {
  id: string;
  os_path: string;
  vendor: string;
  model: string;
  serial: string;
  bus: string;
  size: number;
  removable: boolean;
  partitions: DiskPartition[];
  nicevault: NvState;
  /** `Vendor Model` (what is typed to confirm; may be empty) */
  name: string;
  serial_tail: string;
}

export interface FsOption {
  id: DataFsId;
  creatable: boolean;
  missing_tool: string | null;
  package: string | null;
  readable_on: [OsId, Access][];
}

export interface DeviceOptions {
  os: OsId;
  fs: FsOption[];
  sources: { os: OsId; local: boolean }[];
  gpt_required: boolean;
  default_table: TableId;
  data_size: number;
  update_host: string;
}

export interface VolumeOpts {
  mode: VolumeMode;
  /** "Un pezzo": bytes, preallocated; null = grows as needed */
  size: number | null;
  name: string;
}

export interface PlanOpts {
  id: string;
  table: TableId;
  data_fs: DataFsId;
  first_volume: VolumeOpts | null;
  download: boolean;
}

export interface PartSpec {
  label: string;
  fs: string;
  offset: number;
  size: number;
}

/** A NiceVault volume on the drive the plan erases (read-only scan of its mounted partitions). */
export interface FoundVolume {
  name: string;
  path: string;
  /** open in this app right now */
  open: boolean;
}

export interface DevicePlan {
  id: string;
  os: OsId;
  /** the backend sends a bare Disk; devicePlan() adds name / serial_tail like a candidate's */
  disk: DiskCandidate;
  table: TableId;
  table_forced: boolean;
  data_fs: DataFsId;
  client: PartSpec;
  data: PartSpec;
  data_size: number;
  first_volume: VolumeOpts | null;
  download: boolean;
  steps: DeviceStep[];
  commands_preview: string[];
  confirm_name: string;
  /** NiceVault volumes the erase would destroy (older backends: missing = none found) */
  volumes?: FoundVolume[];
}

export type DeviceStep =
  | 'check'
  | 'download'
  | 'unmount'
  | 'partition'
  | 'format_client'
  | 'format_data'
  | 'mount'
  | 'copy'
  | 'verify'
  | 'marker'
  | 'volume';

export interface DeviceOutcome {
  disk_id: string;
  client_mount: string;
  data_mount: string;
  data_fs: DataFsId | null;
  data_free: number;
  whole_volume_size: number;
  missing: OsId[];
  first_volume: VolumeOpts | null;
  log: string;
}

export interface DeviceProgress {
  job: JobId;
  step: DeviceStep;
  done_bytes: number;
  total_bytes: number;
}

export interface DeviceDone {
  job: JobId;
  error: ApiError | null;
  outcome: DeviceOutcome | null;
  log?: string;
}

// ---------------------------------------------------------------------------
// commands

export const deviceCandidates = () => call<DiskCandidate[]>('device_candidates');
export const deviceOptions = (id: string) => call<DeviceOptions>('device_options', { id });
export const devicePlan = async (opts: PlanOpts) => normalizePlan(await call<DevicePlan | (Omit<DevicePlan, 'disk'> & { disk: Disk })>('device_plan', { opts }));
export const deviceCreate = (planId: string, typed: string) => call<JobId>('device_create', { plan_id: planId, typed });
export const deviceResume = (id: string) => call<JobId>('device_resume', { id });
export const deviceEject = (id: string) => call<void>('device_eject', { id });
export const deviceLog = (job: JobId) => call<string>('device_log', { job });
export const deviceOutcome = (job: JobId) => call<DeviceOutcome>('device_outcome', { job });
export const deviceOpen = (job: JobId) => call<void>('device_open', { job });
/** "Scarica ora": adds the programs a finished device lacks (made offline); the updated outcome. */
export const deviceFetchMissing = (job: JobId) => call<DeviceOutcome>('device_fetch_missing', { job });

// ---------------------------------------------------------------------------
// pure helpers

/** Drive sizes as the box says them: decimal GB / TB, one decimal ("30,8 GB"). */
export function formatDiskSize(bytes: number): string {
  if (bytes >= 1e12) return `${formatNumber(bytes / 1e12, 1)} TB`;
  if (bytes >= 1e9) return `${formatNumber(bytes / 1e9, 1)} GB`;
  return `${formatNumber(bytes / 1e6, 0)} MB`;
}

const BUS: Record<string, string> = { usb: 'USB', sd: 'SD', mmc: 'MMC', sdio: 'SDIO', ieee1394: 'FireWire', virtual: 'VHD', sata: 'SATA', nvme: 'NVMe' };
export const busLabel = (bus: string) => BUS[bus] ?? bus.toUpperCase();

/** The drive's name, or `fallback` (an unnamed drive) */
export const diskName = (d: { name?: string | null }, fallback: string) => (d.name ?? '').trim() || fallback;

/** Last 7 characters of the serial (the backend's Disk::serial_tail). */
export const serialTail = (serial: string) => [...serial.trim()].slice(-7).join('');

/** The plan as the wizard reads it: the backend's `disk` is a bare Disk (no `name`, no
 *  `serial_tail`; reading `name.trim()` off it froze the confirm step), `volumes` may be absent. */
export function normalizePlan(p: Omit<DevicePlan, 'disk'> & { disk: Disk | DiskCandidate }): DevicePlan {
  const d = p.disk as Partial<DiskCandidate> & Disk;
  return {
    ...p,
    disk: { ...d, name: d.name ?? p.confirm_name ?? '', serial_tail: d.serial_tail ?? serialTail(d.serial ?? '') },
    volumes: p.volumes ?? [],
  };
}

// ---------------------------------------------------------------------------
// waiting (every call the wizard waits on shows it; see DeviceWizard.svelte)

export type Pending = 'candidates' | 'options' | 'plan' | 'create' | 'resume' | 'eject';
/** After this long a step says it is still working (reassurance). */
export const HINT_MS = 2000;
/** After this long a step offers Annulla / Riprova. */
export const STUCK_MS = 30000;
/** Privileged steps: the system may be asking for a password (polkit / UAC / macOS). */
export const AUTH_STEPS: DeviceStep[] = ['unmount', 'partition', 'format_client', 'format_data'];
export const waitingFor = (step: DeviceStep | null) => step !== null && AUTH_STEPS.includes(step);

/** `Vendor Model · 30,8 GB · USB · S/N …1103235` (spec 2). */
export function diskLine(d: DiskCandidate, fallback: string): string {
  const parts = [diskName(d, fallback), formatDiskSize(d.size), busLabel(d.bus)];
  if (d.serial_tail) parts.push(`S/N …${d.serial_tail}`);
  return parts.join(' · ');
}

/** The same rule as the backend (confirmation_ok): trimmed, case-insensitive, spaces collapsed;
 *  an unnamed drive takes CANCELLA or ERASE. */
export function confirmMatches(confirmName: string, typed: string): boolean {
  const norm = (s: string) => s.trim().split(/\s+/).filter(Boolean).join(' ').toLowerCase();
  const t = norm(typed);
  if (!confirmName.trim()) return t === 'cancella' || t === 'erase';
  return t !== '' && t === norm(confirmName);
}

/** The OSes that cannot read a file system ("Non leggibile su Windows e macOS"). */
export const unreadableOn = (f: FsOption): OsId[] => f.readable_on.filter(([, a]) => a === 'no').map(([o]) => o);
export const readOnlyOn = (f: FsOption): OsId[] => f.readable_on.filter(([, a]) => a === 'ro').map(([o]) => o);

export type StepState = 'todo' | 'current' | 'done' | 'failed';

/** Each step's state from the current one (and whether the job ended, with an error). */
export function stepStates(steps: DeviceStep[], current: DeviceStep | null, finished: 'ok' | 'error' | null): StepState[] {
  const i = current ? steps.indexOf(current) : -1;
  return steps.map((_, k) => {
    if (finished === 'ok') return 'done';
    if (k < i) return 'done';
    if (k === i) return finished === 'error' ? 'failed' : 'current';
    return 'todo';
  });
}

/** Cancelling is possible only before the disk is touched (spec 6.5). */
export const cancellable = (step: DeviceStep | null) => step === null || step === 'check' || step === 'download';

/** The `.nvault` path of the first volume in VaultData. */
export function volumePath(dataMount: string, name: string): string {
  const sep = dataMount.includes('\\') && !dataMount.includes('/') ? '\\' : '/';
  return `${dataMount.replace(/[\\/]+$/, '')}${sep}${name.trim().replace(/\.nvault$/i, '')}.nvault`;
}

/** Size of the first volume: the whole partition (preallocated), a fixed piece, or growing. */
export function volumeSize(v: VolumeOpts, o: Pick<DeviceOutcome, 'whole_volume_size'>): number | null {
  return v.mode === 'whole' ? o.whole_volume_size : v.size;
}

/** GB typed in the options -> bytes (whole MiB); NaN / <= 0 -> null. */
export function gbToBytes(gb: string): number | null {
  const n = Number(gb.replace(',', '.'));
  if (!Number.isFinite(n) || n <= 0) return null;
  const b = Math.floor((n * 1e9) / 1048576) * 1048576;
  return b > 0 ? b : null;
}
