import type { ResizeInfo } from './types';

export const KIB = 1024;
export const MIB = 1024 * KIB;
export const GIB = 1024 * MIB;
export const FIXED_MIN = 4 * MIB;
export const FIXED_MAX = 16 * 1024 * GIB;
export const FIXED_DB_OFFSET = 192 * KIB;
export const MARGIN_MIN = 8 * MIB;

export const down4k = (n: number) => n - (n % 4096);
const up4k = (n: number) => Math.ceil(n / 4096) * 4096;

export function limits(capacity: number, extent: number, used: number, free: number): { min: number; inPlace: number; max: number } {
  const margin = Math.max(Math.floor(used / 32), MARGIN_MIN);
  const min = Math.min(Math.max(up4k(FIXED_DB_OFFSET + used + margin), FIXED_MIN), capacity);
  const inPlace = Math.min(Math.max(up4k(FIXED_DB_OFFSET + extent), min), capacity);
  const max = Math.max(Math.min(down4k(capacity + free), FIXED_MAX), capacity);
  return { min, inPlace, max };
}

export type SizeUnit = 'MB' | 'GB';
export const UNIT_BYTES: Record<SizeUnit, number> = { MB: MIB, GB: GIB };

export function toBytes(value: number, unit: SizeUnit): number {
  return Number.isFinite(value) && value > 0 ? down4k(Math.round(value * UNIT_BYTES[unit])) : NaN;
}

export function fromBytes(n: number, unit: SizeUnit): number {
  return Math.round((n / UNIT_BYTES[unit]) * 100) / 100;
}

export const unitFor = (n: number): SizeUnit => (n >= GIB ? 'GB' : 'MB');

export type ResizePlan =
  | 'growable'
  | 'invalid'
  | 'same'
  | 'grow'
  | 'shrink'
  | 'compact'
  | 'too_small'
  | 'too_large'
  | 'no_space_compact';

export function plan(info: ResizeInfo, target: number): ResizePlan {
  if (info.capacity === null) return 'growable';
  if (!Number.isFinite(target)) return 'invalid';
  const t = down4k(target);
  if (t === info.capacity) return 'same';
  if (t > info.capacity) return t > info.max_capacity || t > FIXED_MAX ? 'too_large' : 'grow';
  if (t < info.min_capacity || t < FIXED_MIN) return 'too_small';
  if (t >= info.min_in_place) return 'shrink';
  return t > info.drive_free ? 'no_space_compact' : 'compact';
}
