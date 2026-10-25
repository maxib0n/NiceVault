// Site icons of password-manager rows. In the app each stored icon (a <= 64 px PNG inside the
// volume) is fetched ONCE from nvault://<vault>/pm-icon/<host>/<version> and kept as a blob:
// URL, so a fast scroll that creates and drops rows never goes back through the custom
// protocol (one IPC + database read per <img> on WebKitGTK). The mock's icons are data: URLs
// already in memory and are used as they are. Icons that fail stay on the monogram.
import { isTauri, pmIconUrl } from './api';
import type { VaultId } from './types';

class PmIcons {
  /** `${vault}|${icon}` -> blob: URL, or '' = failed */
  private urls = $state<Record<string, string>>({});
  private pending = new Set<string>();

  /** The <img> src for `icon`, undefined while it loads (start it), null when it failed. */
  src(vault: VaultId, icon: string): string | null | undefined {
    if (!isTauri) return pmIconUrl(vault, icon);
    const key = `${vault}|${icon}`;
    const u = this.urls[key];
    if (u !== undefined) return u || null;
    if (!this.pending.has(key)) {
      this.pending.add(key);
      // not during the render that asked
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
      /* locked meanwhile, or gone: the monogram stays */
    }
    this.pending.delete(key);
    this.urls[key] = out;
  }

  /** Forget a volume's icons (it was locked / the screen closed). */
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
