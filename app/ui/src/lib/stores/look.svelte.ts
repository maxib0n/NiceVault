// Which accent / theme / auto-lock the app wears right now. Settings are per volume:
//   - open world (a volume on screen): that volume's settings (VaultInfo.settings, kept
//     fresh by api.onSettingsChanged);
//   - locked world: Sblocca wears the target volume's header accent when its look is
//     visible (RecentEntry.accent); everything else, and the theme, the app's own look
//     (Impostazioni > Aspetto dell'app; lib/worn.ts has the precedence).
// It drives the existing stores (accent.current, prefs.theme, idle), so App.svelte's
// effects apply it to <html data-accent / data-theme>. Started once by bootstrap.ts.
import { SvelteMap } from 'svelte/reactivity';
import * as api from '../api';
import { worn } from '../worn';
import { switches } from '../switches';
import type { Settings, VaultId } from '../types';
import { accent } from './accent.svelte';
import { appLook } from './applook.svelte';
import { app } from './app.svelte';
import { idle } from './idle.svelte';
import { prefs } from './prefs.svelte';
import { vaults } from './vaults.svelte';

class Look {
  /** latest known settings per open volume */
  settings = new SvelteMap<VaultId, Settings>();
  private started = false;

  /** Settings of an open volume (null when unknown). */
  of(id: VaultId | null | undefined): Settings | null {
    if (!id) return null;
    return this.settings.get(id) ?? vaults.open.find((v) => v.id === id)?.settings ?? null;
  }

  start(): void {
    if (this.started) return;
    this.started = true;
    api.onSettingsChanged((vault, s) => {
      const cur = this.of(vault);
      if (vault && cur) this.settings.set(vault, { ...cur, ...s });
      // one source of truth for the ink: Benvenuto's passport of this volume shows it at once
      // (the backend has rewritten the header; the open volume may be shown there right now)
      if (vault && typeof s.accent === 'string') {
        const path = vaults.open.find((v) => v.id === vault)?.path;
        const r = path ? app.recents.find((x) => x.path === path) : undefined;
        if (r && r.show_when_locked !== false) r.accent = s.accent;
        void app.loadRecents();
      }
      // each open volume keeps its own idle timeout
      if (vault && typeof s.auto_lock_minutes === 'number' && !switches.autolock) idle.setMinutesFor(vault, s.auto_lock_minutes);
    });
    $effect.root(() => {
      $effect(() => {
        const ids = new Set(vaults.open.map((v) => v.id));
        for (const id of [...this.settings.keys()]) if (!ids.has(id)) this.settings.delete(id);
      });
      $effect(() => {
        const active = app.world === 'open' ? vaults.active : null;
        const s = active ? this.of(active.id) : null;
        if (active && s) {
          accent.adopt(s.accent);
          prefs.theme = s.theme;
          if (!switches.autolock) idle.setMinutes(s.auto_lock_minutes);
        } else if (!active) {
          const w = worn(null, app.screen === 'unlock' ? app.unlockTarget : null, { theme: appLook.theme, accent: appLook.accent });
          accent.adopt(w.accent);
          prefs.theme = w.theme;
        }
      });
    });
  }
}

export const look = new Look();
