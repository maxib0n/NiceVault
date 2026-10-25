// "Aggiorna i programmi sulla chiavetta" (app/src-tauri/src/device/stick.rs): when a volume lives
// on a NiceVault device, the programs on its NiceVault partition can be brought up to date.
//
//   device_sticks { path? }        -> Stick[]   connected NiceVault devices (path: the one holding it); read-only
//   device_stick_mount { id }      -> Stick     mounts the NiceVault partition (unprivileged)
//   device_stick_update { id }     -> JobId     replaces the programs (this app's own build + signed downloads)
// Events: job-progress { job, step, os, done_bytes, total_bytes } · job-done { job, error, outcome: StickOutcome }.

import { call } from './api';
import type { OsId } from './device';
import type { ApiError, JobId } from './types';

export type StickSource = 'running' | 'download';
export type StickChange = 'updated' | 'unchanged' | 'offline' | 'failed' | 'rewrite' | 'in_use';
export type StickStep = 'check' | 'download' | 'copy' | 'verify' | 'marker';
/** how a program is replaced: beside the old one (kept until verified) or in its place (tight space) */
export type StickMode = 'safe' | 'tight';

export interface StickOsSpace {
  os: OsId;
  /** bytes it takes now / the new one takes (before the download: an estimate) */
  now: number;
  next: number;
  /** null: it does not fit even in place of the old one */
  mode: StickMode | null;
}

/** The NiceVault partition's room (stick.rs Space): 128 MiB partitions are "tight". */
export interface StickSpace {
  size: number | null;
  free: number | null;
  /** freed first: leftovers of an update cut short, the old macOS zip */
  reclaim: number;
  /** in the order they are written (smallest first) */
  programs: StickOsSpace[];
  tight: boolean;
  fits: boolean;
}

export interface StickOs {
  os: OsId;
  file: string;
  present: boolean;
  /** version on the stick (null: missing or unknown) */
  version: string | null;
  /** what it would get: this app's version for its own OS, null = the latest (known online) */
  target: string | null;
  source: StickSource;
  current: boolean;
}

export interface Stick {
  disk_id: string;
  name: string;
  client_mount: string | null;
  mounted: boolean;
  complete: boolean;
  data_fs: string | null;
  running_version: string;
  programs: StickOs[];
  outdated: boolean;
  runs_from_stick: boolean;
  /** mounted and complete: the room and how the update would use it */
  space?: StickSpace | null;
}

export interface StickOsResult {
  os: OsId;
  from: string | null;
  to: string | null;
  result: StickChange;
  error: ApiError | null;
  mode?: StickMode | null;
}

export interface StickOutcome {
  results: StickOsResult[];
  stick: Stick;
  /** the room as the writing started (real sizes) */
  space?: StickSpace | null;
}

export interface StickProgress {
  job: JobId;
  /** 'space': the plan with the real sizes (carries `space`), not a step of its own */
  step: StickStep | 'space';
  space?: StickSpace;
  os: OsId | null;
  done_bytes: number;
  total_bytes: number;
}

export interface StickDone {
  job: JobId;
  error: ApiError | null;
  outcome: StickOutcome | null;
}

export const deviceSticks = (path?: string | null) => call<Stick[]>('device_sticks', { path: path ?? null });
export const deviceStickMount = (id: string) => call<Stick>('device_stick_mount', { id });
export const deviceStickUpdate = (id: string) => call<JobId>('device_stick_update', { id });

export const OS_NAMES: Record<OsId, string> = { linux: 'Linux', windows: 'Windows', macos: 'macOS' };

/** The programs that something would change for (missing or older), in display order. */
export const stale = (s: Stick): StickOs[] => s.programs.filter((p) => !p.current);

/** Whether a notice is worth showing: mounted, a finished device, something to update. */
export const needsUpdate = (s: Stick | null | undefined): s is Stick => !!s && s.mounted && s.complete && s.outdated;

/** Fraction 0..1 of a progress event (null: indeterminate). */
export const fraction = (p: Pick<StickProgress, 'done_bytes' | 'total_bytes'> | null): number | null =>
  p && p.total_bytes > 0 ? Math.min(1, p.done_bytes / p.total_bytes) : null;
