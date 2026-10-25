// App-wide accent preset (Settings.accent, persisted next to the exe by the backend).
import * as api from '../api';
import { DEFAULT_ACCENT, isAccent, type AccentId } from '../accents';
import { toasts } from './toasts.svelte';

class Accent {
  current = $state<AccentId>(DEFAULT_ACCENT);

  /** From settings_get. */
  adopt(id: string | undefined) {
    this.current = isAccent(id) ? id : DEFAULT_ACCENT;
  }

  async set(id: AccentId) {
    const prev = this.current;
    this.current = id;
    try {
      this.adopt((await api.settingsSet({ accent: id })).accent);
    } catch (e) {
      this.current = prev;
      toasts.error(e);
    }
  }
}

export const accent = new Accent();
