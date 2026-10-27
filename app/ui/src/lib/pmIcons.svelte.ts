import { isTauri, pmIconUrl } from './api';
import type { VaultId } from './types';

class PmIcons {
  private urls = $state<Record<string, string>>({});
  private pending = new Set<string>();

  src(vault: VaultId, icon: string): string | null | undefined {
    if (!isTauri) return pmIconUrl(vault, icon);
    const key = `${vault}|${icon}`;
    const u = this.urls[key];
    if (u !== undefined) return u || null;
    if (!this.pending.has(key)) {
      this.pending.add(key);
      queueMicrotask(() => void this.load(key, pmIconUrl(vault, icon)));
    }
    return undefined;
  }

  private async load(key: string, url: string) {
    let out = '';
    try {
      const r = await fetch(url);
      if (r.ok) out = URL.createObjectURL(await r.blob());
    } catch {
    }
    this.pending.delete(key);
    this.urls[key] = out;
  }

  drop(vault: VaultId) {
    const prefix = `${vault}|`;
    const next: Record<string, string> = {};
    for (const [k, u] of Object.entries(this.urls)) {
      if (!k.startsWith(prefix)) next[k] = u;
      else if (u) URL.revokeObjectURL(u);
    }
    this.urls = next;
  }
}

export const pmIcons = new PmIcons();
