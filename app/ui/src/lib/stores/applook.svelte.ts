import * as api from '../api';
import type { AppSettings, AppSettingsPatch } from '../api';
import { DEFAULT_ACCENT, isAccent, type AccentId } from '../accents';
import type { ThemeId } from '../types';
import { setSoundsEnabled } from '../sound';
import { toasts } from './toasts.svelte';
import { isMobile } from '../platform';

class AppLook {
  theme = $state<ThemeId>('dark');
  accent = $state<AccentId>(DEFAULT_ACCENT);
  scale = $state(100);
  newLikeApp = $state(true);
  sounds = $state(true);
  videoArrows = $state<api.VideoArrows>('seek');

  adopt(s: Pick<AppSettings, 'app_theme' | 'app_accent' | 'ui_scale' | 'new_like_app'> & { sounds?: boolean; video_arrows?: api.VideoArrows }) {
    this.theme = s.app_theme ?? 'dark';
    this.accent = isAccent(s.app_accent) ? s.app_accent : DEFAULT_ACCENT;
    this.scale = !isMobile && (api.UI_SCALES as readonly number[]).includes(s.ui_scale) ? s.ui_scale : 100;
    this.newLikeApp = s.new_like_app ?? true;
    this.sounds = s.sounds ?? true;
    this.videoArrows = s.video_arrows === 'step' ? 'step' : 'seek';
    setSoundsEnabled(this.sounds);
    this.applyScale();
  }

  private applyScale() {
    if (api.isTauri || typeof document === 'undefined') return;
    const z = this.scale / 100;
    if (z === 1) document.documentElement.style.removeProperty('zoom');
    else document.documentElement.style.setProperty('zoom', String(z));
  }

  async set(patch: Pick<AppSettingsPatch, 'app_theme' | 'app_accent' | 'ui_scale' | 'new_like_app' | 'sounds' | 'video_arrows'>): Promise<AppSettings | null> {
    const prev = { app_theme: this.theme, app_accent: this.accent, ui_scale: this.scale, new_like_app: this.newLikeApp, sounds: this.sounds, video_arrows: this.videoArrows };
    this.adopt({ ...prev, ...patch });
    try {
      const s = await api.appSettingsSet(patch);
      this.adopt(s);
      return s;
    } catch (e) {
      this.adopt(prev);
      toasts.error(e);
      return null;
    }
  }

  step(dir: 1 | -1 | 0) {
    if (isMobile) return;
    const all = api.UI_SCALES as readonly number[];
    const i = all.indexOf(this.scale);
    const next = dir === 0 ? 100 : all[Math.min(all.length - 1, Math.max(0, (i < 0 ? all.indexOf(100) : i) + dir))];
    if (next !== this.scale) void this.set({ ui_scale: next });
  }
}

export const appLook = new AppLook();
