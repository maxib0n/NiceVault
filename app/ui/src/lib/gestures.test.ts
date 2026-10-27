import { describe, expect, it } from 'vitest';
import { classify, FLICK_MIN, FLICK_SPEED, pinchScale, rubber, SWIPE_MIN, ZOOM_MAX } from './gestures';

describe('gestures', () => {
  it('classifies strokes', () => {
    expect(classify({ dx: 3, dy: -4 })).toBe('tap');
    expect(classify({ dx: -SWIPE_MIN - 1, dy: 10 })).toBe('left');
    expect(classify({ dx: SWIPE_MIN + 1, dy: -10 })).toBe('right');
    expect(classify({ dx: 10, dy: SWIPE_MIN * 2 })).toBe('down');
    expect(classify({ dx: 0, dy: -SWIPE_MIN * 2 })).toBe(null);
    expect(classify({ dx: SWIPE_MIN, dy: SWIPE_MIN })).toBe(null);
    expect(classify({ dx: 30, dy: 0 })).toBe(null);
  });
  it('a quick flick counts as a swipe even when short', () => {
    expect(classify({ dx: -FLICK_MIN - 2, dy: 3, vx: -FLICK_SPEED - 0.1 })).toBe('left');
    expect(classify({ dx: FLICK_MIN + 2, dy: 3, vx: FLICK_SPEED + 0.1 })).toBe('right');
    expect(classify({ dx: FLICK_MIN + 2, dy: 3, vx: 0.1 })).toBe(null);
    expect(classify({ dx: FLICK_MIN + 2, dy: 3, vx: -1 })).toBe(null);
    expect(classify({ dx: 4, dy: FLICK_MIN * 2 + 2, vy: FLICK_SPEED + 0.2 })).toBe('down');
  });
  it('rubber band stiffens and never passes its limit', () => {
    expect(rubber(0, 40)).toBe(0);
    expect(Math.abs(rubber(-400, 40))).toBeLessThan(40);
    expect(rubber(-400, 40)).toBeLessThan(0);
    expect(rubber(20, 40)).toBeLessThan(20);
  });
  it('pinch scale is clamped', () => {
    expect(pinchScale(1, 100, 200)).toBe(2);
    expect(pinchScale(2, 100, 25)).toBe(1);
    expect(pinchScale(4, 100, 1000)).toBe(ZOOM_MAX);
    expect(pinchScale(1.5, 0, 100)).toBe(1.5);
  });
});
