import * as api from '../api';
import type { Drive } from '../types';
import { app } from './app.svelte';

class Drives {
  list = $state<Drive[]>([]);
  loaded = $state(false);
  changes = $state(0);
  private unlisten: (() => void) | null = null;

  async refresh(): Promise<Drive[]> {
    try {
      this.list = await api.drivesList();
    } catch {
    } finally {
      this.loaded = true;
    }
    return this.list;
  }

  async listen(): Promise<() => void> {
    if (!this.unlisten) {
      this.unlisten = await api.onDrivesChanged((e) => {
        this.list = e.drives;
        this.loaded = true;
        this.changes++;
        void app.loadRecents();
      });
    }
    return () => {
      this.unlisten?.();
      this.unlisten = null;
    };
  }
}

export const drives = new Drives();

export function driveOf(list: Drive[], path: string): Drive | null {
  const norm = (p: string) => p.replace(/[\\/]+$/, '') || p;
  const p = norm(path);
  let best: Drive | null = null;
  for (const d of list) {
    const root = norm(d.mount_path);
    const sep = root.endsWith('/') || root.endsWith('\\') ? '' : p.includes('\\') && !p.includes('/') ? '\\' : '/';
    const inside = p === root || p.startsWith(root + sep) || (root === '/' && p.startsWith('/'));
    if (inside && (!best || root.length > norm(best.mount_path).length)) best = d;
  }
  return best;
}
