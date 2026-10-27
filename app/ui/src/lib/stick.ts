import { call } from './api';
import type { OsId } from './device';
import type { ApiError, JobId } from './types';

export type StickSource = 'running' | 'download';
export type StickChange = 'updated' | 'unchanged' | 'offline' | 'failed' | 'rewrite' | 'in_use';
export type StickStep = 'check' | 'download' | 'copy' | 'verify' | 'marker';
export type StickMode = 'safe' | 'tight';

export interface StickOsSpace {
  os: OsId;
  now: number;
  next: number;
  mode: StickMode | null;
}

export interface StickSpace {
  size: number | null;
  free: number | null;
  reclaim: number;
  programs: StickOsSpace[];
  tight: boolean;
  fits: boolean;
}

export interface StickOs {
  os: OsId;
  file: string;
  present: boolean;
  version: string | null;
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
  space?: StickSpace | null;
}

export interface StickProgress {
  job: JobId;
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

export const stale = (s: Stick): StickOs[] => s.programs.filter((p) => !p.current);

export const needsUpdate = (s: Stick | null | undefined): s is Stick => !!s && s.mounted && s.complete && s.outdated;

export const fraction = (p: Pick<StickProgress, 'done_bytes' | 'total_bytes'> | null): number | null =>
  p && p.total_bytes > 0 ? Math.min(1, p.done_bytes / p.total_bytes) : null;
