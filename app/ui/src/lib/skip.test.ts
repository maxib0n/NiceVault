import { describe, expect, it } from 'vitest';
import { skipper } from './skip';

describe('skipper', () => {
  it('grows with quick presses and starts over after a pause', () => {
    const s = skipper();
    const quick = Array.from({ length: 10 }, (_, i) => s(i * 200));
    expect(quick).toEqual([10, 10, 10, 20, 20, 30, 30, 60, 60, 60]);
    expect(s(10 * 200 + 2000)).toBe(10);
    s(5000);
    expect(s(5100)).toBe(10);
    expect(s(5200, -1)).toBe(10);
  });
});
