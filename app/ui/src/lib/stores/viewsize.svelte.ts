import * as api from '../api';
import type { GridSize, ListDensity, VaultId } from '../types';
import { look } from './look.svelte';
import { isMobile } from '../platform';

export const GRID_SIZES: readonly GridSize[] = ['s', 'm', 'l', 'xl'];
export const DENSITIES: readonly ListDensity[] = ['compact', 'normal', 'comfortable'];
export const DEFAULT_GRID: GridSize = 'm';
export const DEFAULT_DENSITY: ListDensity = 'normal';

export const TILE_MIN_POINTER: Record<GridSize, number> = { s: 116, m: 164, l: 220, xl: 300 };
export const TILE_MIN_TOUCH: Record<GridSize, number> = { s: 74, m: 100, l: 140, xl: 240 };
export const TILE_MIN = isMobile ? TILE_MIN_TOUCH : TILE_MIN_POINTER;
export const ROW_POINTER: Record<ListDensity, { h: number; w: number; vis: number }> = {
  compact: { h: 28, w: 24, vis: 20 },
  normal: { h: 38, w: 30, vis: 26 },
  comfortable: { h: 54, w: 44, vis: 38 },
};
export const ROW_TOUCH: Record<ListDensity, { h: number; w: number; vis: number }> = {
  compact: { h: 44, w: 28, vis: 24 },
  normal: { h: 60, w: 44, vis: 38 },
  comfortable: { h: 84, w: 66, vis: 58 },
};
export const ROW = isMobile ? ROW_TOUCH : ROW_POINTER;

export function step<T>(list: readonly T[], cur: T, delta: number): T {
  const i = Math.max(0, list.indexOf(cur));
  return list[Math.min(list.length - 1, Math.max(0, i + delta))];
}

export function thumbPx(size: GridSize, dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1): number {
  return Math.ceil(TILE_MIN[size] * 1.5 * dpr);
}

class ViewSize {
  grid(vault: VaultId | null | undefined): GridSize {
    const g = look.of(vault)?.grid_size;
    return g && GRID_SIZES.includes(g) ? g : DEFAULT_GRID;
  }

  density(vault: VaultId | null | undefined): ListDensity {
    const d = look.of(vault)?.list_density;
    return d && DENSITIES.includes(d) ? d : DEFAULT_DENSITY;
  }

  private save(vault: VaultId, patch: { grid_size?: GridSize; list_density?: ListDensity }) {
    const cur = look.of(vault);
    if (cur) look.settings.set(vault, { ...cur, ...patch });
    void api.settingsSet(patch, vault).catch(() => undefined);
  }

  setGrid(vault: VaultId, g: GridSize) {
    if (g !== this.grid(vault)) this.save(vault, { grid_size: g });
  }

  setDensity(vault: VaultId, d: ListDensity) {
    if (d !== this.density(vault)) this.save(vault, { list_density: d });
  }

  zoom(vault: VaultId, layout: 'grid' | 'list', delta: number) {
    if (layout === 'grid') this.setGrid(vault, delta === 0 ? DEFAULT_GRID : step(GRID_SIZES, this.grid(vault), delta));
    else this.setDensity(vault, delta === 0 ? DEFAULT_DENSITY : step(DENSITIES, this.density(vault), delta));
  }
}

export const viewSize = new ViewSize();
