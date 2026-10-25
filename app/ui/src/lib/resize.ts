// Estendi / Riduci: the size rules of nv-core resize.rs, for the dialog (what a size will do, before
// asking) and the mock backend (the same answers as the real one).
import type { ResizeInfo } from './types';

export const KIB = 1024;
export const MIB = 1024 * KIB;
export const GIB = 1024 * MIB;
/** Fixed::MIN / Fixed::MAX (header.rs) */
export const FIXED_MIN = 4 * MIB;
export const FIXED_MAX = 16 * 1024 * GIB;
/** header copies + footer region before the store (vfs.rs FIXED_DB_OFFSET) */
export const FIXED_DB_OFFSET = 192 * KIB;
/** resize.rs RESIZE_MARGIN_MIN: a shrink leaves max(this, used / 32) above the data */
export const MARGIN_MIN = 8 * MIB;

export const down4k = (n: number) => n - (n % 4096);
const up4k = (n: number) => Math.ceil(n / 4096) * 4096;

/** resize.rs `limits`: smallest size, smallest without compaction, largest the drive allows. */
export function limits(capacity: number, extent: number, used: number, free: number): { min: number; inPlace: number; max: number } {
  const margin = Math.max(Math.floor(used / 32), MARGIN_MIN);
  const min = Math.min(Math.max(up4k(FIXED_DB_OFFSET + used + margin), FIXED_MIN), capacity);
  const inPlace = Math.min(Math.max(up4k(FIXED_DB_OFFSET + extent), min), capacity);
  const max = Math.max(Math.min(down4k(capacity + free), FIXED_MAX), capacity);
  return { min, inPlace, max };
}

export type SizeUnit = 'MB' | 'GB';
export const UNIT_BYTES: Record<SizeUnit, number> = { MB: MIB, GB: GIB };

/** The size typed in the dialog, in bytes as the backend will take it (4 KiB steps); NaN if not a number. */
export function toBytes(value: number, unit: SizeUnit): number {
  return Number.isFinite(value) && value > 0 ? down4k(Math.round(value * UNIT_BYTES[unit])) : NaN;
}

/** Bytes shown in `unit`, two decimals at most. */
export function fromBytes(n: number, unit: SizeUnit): number {
  return Math.round((n / UNIT_BYTES[unit]) * 100) / 100;
}

/** The unit a size reads best in. */
export const unitFor = (n: number): SizeUnit => (n >= GIB ? 'GB' : 'MB');

/** What a resize to `target` bytes will do (or why it would be refused), like vault_resize. */
export type ResizePlan =
  | 'growable' // capacity null: nothing to resize
  | 'invalid' // not a size
  | 'same'
  | 'grow' // extend the file: needs target - capacity free on the drive
  | 'shrink' // in place: no free space needed
  | 'compact' // pack the data into a copy of the new size: needs target free on the drive
  | 'too_small' // below the data in use + margin
  | 'too_large' // more than the drive has room for (or the format allows)
  | 'no_space_compact'; // would need compaction, but the drive has not got the new size free

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
