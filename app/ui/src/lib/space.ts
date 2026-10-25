// "Non c'è abbastanza spazio": the room a volume has for an import, checked before it starts.
// Fixed-size: capacity minus the header region, the data and the margin the single-file journal
// needs. Growable: the free space of its drive. (vault_resize_info has the facts.)
import type { ResizeInfo } from './types';
import { FIXED_DB_OFFSET, MARGIN_MIN } from './resize';

/** kept free inside a fixed-size volume for the journal region */
export const JOURNAL_MARGIN = MARGIN_MIN;

export interface Space {
  free: number;
  fixed: boolean;
}

export function spaceOf(i: ResizeInfo): Space {
  if (i.capacity === null) return { free: i.drive_free, fixed: false };
  return { free: Math.max(0, i.capacity - FIXED_DB_OFFSET - i.used_bytes - JOURNAL_MARGIN), fixed: true };
}

export interface Fit {
  ok: boolean;
  need: number;
  free: number;
  fixed: boolean;
  /** fixed volumes: a capacity (4 KiB steps) the import fits in; null otherwise */
  suggest: number | null;
}

export function fits(i: ResizeInfo, need: number): Fit {
  const { free, fixed } = spaceOf(i);
  const ok = need <= free;
  const suggest = fixed && !ok ? Math.ceil(((i.capacity as number) + need - free) / 4096) * 4096 : null;
  return { ok, need, free, fixed, suggest };
}
