// The free-space check before an import (dialog, drag & drop, phones' file picker). A short one
// raises `ask`, which SpaceDialog (mounted in App) shows; the import then does not start.
import * as api from '../api';
import { fits, type Fit } from '../space';
import type { VaultId } from '../types';

export interface SpaceAsk extends Fit {
  vault: VaultId;
}

/** bytes in all: host paths are measured by the backend, picked Files carry their own size */
export async function sourcesSize(sources: api.ImportSource[]): Promise<number> {
  const paths = sources.filter((s): s is string => typeof s === 'string');
  const files = sources.reduce((n, s) => n + (typeof s === 'string' ? 0 : s.size), 0);
  return files + (paths.length ? await api.importSize(paths) : 0);
}

class Space {
  ask = $state<SpaceAsk | null>(null);

  /** true when `sources` fit (or the check itself could not be made: the import then decides) */
  async check(vault: VaultId, sources: api.ImportSource[]): Promise<boolean> {
    try {
      const need = await sourcesSize(sources);
      if (need <= 0) return true;
      const f = fits(await api.vaultResizeInfo(vault), need);
      if (!f.ok) this.ask = { ...f, vault };
      return f.ok;
    } catch {
      return true;
    }
  }
}

export const space = new Space();
