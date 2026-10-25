// The mounted drives (drives.rs), live. "Nuovo volume" offers them as places; when one is
// plugged in or pulled out (`drives-changed`) Benvenuto re-scans its volumes by itself.
import * as api from '../api';
import type { Drive } from '../types';
import { app } from './app.svelte';

class Drives {
  list = $state<Drive[]>([]);
  loaded = $state(false);
  /** bumps at every drives-changed (tests, screens that want to react) */
  changes = $state(0);
  private unlisten: (() => void) | null = null;

  /** Fresh list (free space up to date). */
  async refresh(): Promise<Drive[]> {
    try {
      this.list = await api.drivesList();
    } catch {
      /* keep the last list */
    } finally {
      this.loaded = true;
    }
    return this.list;
  }

  /** Subscribes once (App.svelte). Returns the unlisten function. */
  async listen(): Promise<() => void> {
    if (!this.unlisten) {
      this.unlisten = await api.onDrivesChanged((e) => {
        this.list = e.drives;
        this.loaded = true;
        this.changes++;
        // volumes on the drive appear / show as unreachable without a click
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

/** The drive holding `path` (longest mount_path that is a prefix of it). */
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
