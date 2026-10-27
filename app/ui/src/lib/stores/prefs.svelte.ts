import * as api from '../api';
import type { SortSpec } from '../sort';

export type ThemePref = 'system' | 'light' | 'dark';
export type ViewMode = 'grid' | 'list';

function read<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(`nv.${key}`);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: unknown): void {
  try {
    localStorage.setItem(`nv.${key}`, JSON.stringify(value));
  } catch {
  }
}

class Prefs {
  theme = $state<ThemePref>('dark');
  view = $state<ViewMode>(read('view', 'grid'));
  sort = $state<SortSpec>(read('sort', { key: 'name', dir: 'asc' }));

  setTheme(t: ThemePref) {
    this.theme = t;
    void api.settingsSet({ theme: t }).catch(() => undefined);
  }
  setView(v: ViewMode) {
    this.view = v;
    write('view', v);
  }
  setSort(s: SortSpec) {
    this.sort = s;
    write('sort', s);
  }
}

export const prefs = new Prefs();
