import { describe, expect, it } from 'vitest';
import { DENSITIES, GRID_SIZES, ROW, TILE_MIN, step, thumbPx } from './stores/viewsize.svelte';

describe('view sizes', () => {
  it('steps and clamps', () => {
    expect(step(GRID_SIZES, 'm', 1)).toBe('l');
    expect(step(GRID_SIZES, 'xl', 1)).toBe('xl');
    expect(step(GRID_SIZES, 's', -1)).toBe('s');
    expect(step(DENSITIES, 'normal', -1)).toBe('compact');
  });

  it('sizes grow monotonically', () => {
    const tiles = GRID_SIZES.map((g) => TILE_MIN[g]);
    expect([...tiles].sort((a, b) => a - b)).toEqual(tiles);
    const rows = DENSITIES.map((d) => ROW[d].h);
    expect([...rows].sort((a, b) => a - b)).toEqual(rows);
  });

  it('asks for thumbnails at the shown size times the pixel ratio', () => {
    expect(thumbPx('m', 1)).toBe(246);
    expect(thumbPx('m', 2)).toBe(492);
    expect(thumbPx('xl', 2)).toBeGreaterThan(thumbPx('s', 2) * 2);
  });
});
