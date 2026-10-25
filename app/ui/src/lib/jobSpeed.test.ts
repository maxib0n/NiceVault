import { describe, expect, it } from 'vitest';
import { etaParts, etaSeconds, feedRate, startRate } from './jobSpeed';

describe('job speed', () => {
  it('a steady 10 MB/s reads as 10 MB/s, and the ETA waits for 2 s', () => {
    const r = startRate(0);
    expect(r.rate).toBeNull();
    for (let t = 500; t <= 1500; t += 500) feedRate(r, t, (t / 1000) * 10e6);
    expect(r.rate).toBeCloseTo(10e6, -3);
    expect(etaSeconds(r, 100e6)).toBeNull(); // 1.5 s in
    feedRate(r, 2000, 20e6);
    expect(etaSeconds(r, 100e6)).toBeCloseTo(8, 0);
  });

  it('smooths a change of speed over a few seconds', () => {
    const r = startRate(0);
    for (let t = 500; t <= 5000; t += 500) feedRate(r, t, t * 10e3);
    const before = r.rate!;
    feedRate(r, 5500, r.done + 1e6); // 2 MB/s for half a second
    expect(r.rate!).toBeLessThan(before);
    expect(r.rate!).toBeGreaterThan(2e6 * 3);
    for (let i = 1; i <= 40; i++) feedRate(r, 5500 + i * 500, r.done + 1e6);
    expect(r.rate!).toBeCloseTo(2e6, -5);
  });

  it('unknown when stalled, finished or without samples', () => {
    const r = startRate(0);
    expect(etaSeconds(r, 10)).toBeNull();
    feedRate(r, 1000, 0);
    feedRate(r, 3000, 0);
    expect(etaSeconds(r, 100)).toBeNull(); // no speed
    feedRate(r, 4000, 100);
    expect(etaSeconds(r, 100)).toBeNull(); // done
    feedRate(r, 4000, 50); // same instant: ignored
    expect(r.done).toBe(100);
  });

  it('formats seconds, minutes, hours', () => {
    expect(etaParts(3)).toEqual({ unit: 's', n: 5 });
    expect(etaParts(42)).toEqual({ unit: 's', n: 40 });
    expect(etaParts(61)).toEqual({ unit: 'min', n: 2 });
    expect(etaParts(120)).toEqual({ unit: 'min', n: 2 });
    expect(etaParts(3900)).toEqual({ unit: 'min', n: 65 });
    expect(etaParts(7300)).toEqual({ unit: 'h', n: 2, min: 2 });
  });
});
