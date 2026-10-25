import { describe, expect, it } from 'vitest';
import { fraction, needsUpdate, stale, type Stick } from './stick';
import { StickMock } from './stickMock';
import type { EventMap, EventName } from './types';

const host = (events: [string, unknown][]) => ({
  wait: () => Promise.resolve(),
  emit: <E extends EventName>(e: E, p: EventMap[E]) => void events.push([e, p]),
  speed: () => 0,
});
const opts = { scenario: 'old' as const, offline: false, fail: null, dataMount: '/media/V', running: '0.1.0', latest: '0.2.0' };

describe('stick helpers', () => {
  it('needsUpdate / stale / fraction', () => {
    const s = { mounted: true, complete: true, outdated: true, programs: [{ os: 'linux', current: false }, { os: 'windows', current: true }] } as unknown as Stick;
    expect(needsUpdate(s)).toBe(true);
    expect(needsUpdate({ ...s, mounted: false })).toBe(false);
    expect(needsUpdate(null)).toBe(false);
    expect(stale(s).map((p) => p.os)).toEqual(['linux']);
    expect(fraction({ done_bytes: 5, total_bytes: 10 })).toBe(0.5);
    expect(fraction({ done_bytes: 5, total_bytes: 0 })).toBeNull();
  });
});

describe('StickMock (parity with device/stick.rs)', () => {
  it('finds the device by the volume path and updates it', async () => {
    const ev: [string, unknown][] = [];
    const mk = new StickMock(host(ev), opts);
    expect(await mk.invoke('device_sticks', { path: '/elsewhere/x.nvault' })).toEqual([]);
    const [s] = (await mk.invoke('device_sticks', { path: '/media/V/x.nvault' })) as Stick[];
    expect(s.outdated).toBe(true);
    expect(s.programs.find((p) => p.os === 'linux')?.target).toBe('0.1.0');
    const job = await mk.invoke('device_stick_update', { id: '/dev/sdd' });
    await expect(mk.invoke('device_stick_update', { id: '/dev/sdd' })).rejects.toMatchObject({ code: 'busy' });
    await new Promise((r) => setTimeout(r, 20));
    const done = ev.find(([e, p]) => e === 'job-done' && (p as { job: string }).job === job)?.[1] as { outcome: { stick: Stick } };
    expect(done.outcome.stick.outdated).toBe(false);
  });

  it('refuses an update while the NiceVault partition is not mounted', async () => {
    const mk = new StickMock(host([]), { ...opts, scenario: 'unmounted' });
    await expect(mk.invoke('device_stick_update', { id: '/dev/sdd' })).rejects.toMatchObject({ code: 'stick_not_mounted' });
    expect(((await mk.invoke('device_stick_mount', { id: '/dev/sdd' })) as Stick).mounted).toBe(true);
  });
});
