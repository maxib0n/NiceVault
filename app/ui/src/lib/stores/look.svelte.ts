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
  settings = new SvelteMap<VaultId, Settings>();
  private started = false;

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
      if (vault && typeof s.accent === 'string') {
        const path = vaults.open.find((v) => v.id === vault)?.path;
        const r = path ? app.recents.find((x) => x.path === path) : undefined;
        if (r && r.show_when_locked !== false) r.accent = s.accent;
        void app.loadRecents();
      }
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
