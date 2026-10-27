import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';
import { FIXED_DB_OFFSET, GIB, MARGIN_MIN, MIB, fromBytes, limits, plan, toBytes, unitFor } from './resize';
import type { ResizeInfo, ResizeProgress } from './types';

const up4k = (n: number) => Math.ceil(n / 4096) * 4096;

describe('resize rules (same cases as nv-core)', () => {
  it('limits', () => {
    expect(limits(64 * MIB, 0, 0, 0)).toEqual({ min: up4k(FIXED_DB_OFFSET + MARGIN_MIN), inPlace: up4k(FIXED_DB_OFFSET + MARGIN_MIN), max: 64 * MIB });
    const full = 64 * MIB - FIXED_DB_OFFSET;
    expect(limits(64 * MIB, full, full, 10 * MIB + 5)).toEqual({ min: 64 * MIB, inPlace: 64 * MIB, max: 74 * MIB });
    const used = 3200 * MIB;
    expect(limits(8000 * MIB, used, used, 0).min).toBe(up4k(FIXED_DB_OFFSET + used + 100 * MIB));
  });

  it('plans', () => {
    const info: ResizeInfo = {
      capacity: 4 * GIB,
      file_bytes: 4 * GIB,
      used_bytes: GIB,
      min_capacity: 1.1 * GIB,
      min_in_place: 2 * GIB,
      max_capacity: 10 * GIB,
      drive_free: 1.5 * GIB,
    };
    expect(plan(info, 4 * GIB + 100)).toBe('same');
    expect(plan(info, 5 * GIB)).toBe('grow');
    expect(plan(info, 11 * GIB)).toBe('too_large');
    expect(plan(info, 3 * GIB)).toBe('shrink');
    expect(plan(info, 2 * GIB)).toBe('shrink');
    expect(plan(info, 1.2 * GIB)).toBe('compact');
    expect(plan(info, 1.6 * GIB)).toBe('no_space_compact');
    expect(plan(info, GIB)).toBe('too_small');
    expect(plan(info, NaN)).toBe('invalid');
    expect(plan({ ...info, capacity: null }, 5 * GIB)).toBe('growable');
  });

  it('units', () => {
    expect(toBytes(1.5, 'GB')).toBe(1.5 * GIB);
    expect(toBytes(100, 'MB') % 4096).toBe(0);
    expect(toBytes(0, 'GB')).toBeNaN();
    expect(fromBytes(1536 * MIB, 'GB')).toBe(1.5);
    expect(unitFor(GIB)).toBe('GB');
    expect(unitFor(GIB - 1)).toBe('MB');
  });
});

describe('mock vault_resize*', () => {
  let mock: MockBackend;
  beforeEach(() => {
    mock = new MockBackend({ speed: 0, fixed: true });
    api.useMock(mock);
  });

  async function openDoc() {
    return (await api.vaultOpen(`${STICK}/Documenti.nvault`, MOCK_PASSWORD)).id;
  }

  it('grows, shrinks in place, compacts, refuses like resize.rs', async () => {
    const id = await openDoc();
    const events: ResizeProgress[] = [];
    const off = await api.onVaultResize((p) => events.push(p));
    const info = await api.vaultResizeInfo(id);
    expect(info.capacity).toBe(4 * GIB);
    expect(info.min_capacity).toBeLessThan(info.min_in_place);
    expect((await api.vaultStats(id)).file_size_bytes).toBe(4 * GIB);

    const grown = await api.vaultResize(id, 6 * GIB + 7);
    expect([grown.capacity, grown.file_bytes, grown.drive_free]).toEqual([6 * GIB, 6 * GIB, info.drive_free - 2 * GIB]);
    expect(events.map((e) => e.phase)).toEqual(['extend', 'commit']);

    events.length = 0;
    const inPlace = await api.vaultResize(id, grown.min_in_place);
    expect(inPlace.capacity).toBe(grown.min_in_place);
    expect(events.some((e) => e.phase === 'compact')).toBe(false);

    events.length = 0;
    const packed = await api.vaultResize(id, inPlace.min_capacity);
    expect(packed.capacity).toBe(inPlace.min_capacity);
    expect(events.filter((e) => e.phase === 'compact').at(-1)).toMatchObject({ done: inPlace.used_bytes, total: inPlace.used_bytes, vault: id });
    expect(packed.min_in_place).toBe(packed.min_capacity);

    await expect(api.vaultResize(id, packed.min_capacity - 4096 * 10)).rejects.toMatchObject({ code: 'too_small' });
    await expect(api.vaultResize(id, 1024)).rejects.toMatchObject({ code: 'out_of_range' });
    await expect(api.vaultResize(id, 1000 * GIB)).rejects.toMatchObject({ code: 'no_space' });
    off();
  });

  it('growable volumes have nothing to resize; created fixed ones do', async () => {
    api.useMock((mock = new MockBackend({ speed: 0 })));
    const id = await openDoc();
    const info = await api.vaultResizeInfo(id);
    expect(info.capacity).toBeNull();
    await expect(api.vaultResize(id, 8 * GIB)).rejects.toMatchObject({ code: 'not_fixed' });
    const made = await api.vaultCreate(`${STICK}/Fisso.nvault`, 'pw', 'fast', 64 * MIB + 5);
    expect((await api.vaultResizeInfo(made.id)).capacity).toBe(64 * MIB);
  });

  it('locked volume', async () => {
    await expect(api.vaultResizeInfo('nope')).rejects.toMatchObject({ code: 'locked' });
  });
});
