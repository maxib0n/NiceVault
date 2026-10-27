import { describe, expect, it } from 'vitest';
import { fits, JOURNAL_MARGIN } from './space';
import { FIXED_DB_OFFSET, GIB, MIB } from './resize';
import type { ResizeInfo } from './types';

const info = (o: Partial<ResizeInfo>): ResizeInfo => ({ capacity: 4 * GIB, file_bytes: 4 * GIB, used_bytes: GIB, min_capacity: 0, min_in_place: 0, max_capacity: 100 * GIB, drive_free: 10 * GIB, ...o }) as ResizeInfo;

describe('import space check', () => {
  it('fixed: capacity minus header, data and the journal margin', () => {
    const f = fits(info({}), 100 * MIB);
    expect(f).toMatchObject({ ok: true, fixed: true, free: 4 * GIB - FIXED_DB_OFFSET - GIB - JOURNAL_MARGIN });
    expect(f.suggest).toBeNull();
  });
  it('fixed, too big: says what to extend to, in 4 KiB steps', () => {
    const i = info({});
    const f = fits(i, 5 * GIB);
    expect(f.ok).toBe(false);
    expect(f.suggest! % 4096).toBe(0);
    expect(fits({ ...i, capacity: f.suggest! }, 5 * GIB).ok).toBe(true);
  });
  it('growable: the drive decides, nothing to extend', () => {
    expect(fits(info({ capacity: null }), 11 * GIB)).toMatchObject({ ok: false, fixed: false, free: 10 * GIB, suggest: null });
    expect(fits(info({ capacity: null }), 10 * GIB).ok).toBe(true);
  });
  it('a full fixed volume has no negative room', () => {
    expect(fits(info({ used_bytes: 4 * GIB }), 1).free).toBe(0);
  });
});
