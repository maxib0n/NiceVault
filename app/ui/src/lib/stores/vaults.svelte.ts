// Open volumes, the unlock flow, lock handling and mount state.
import { m } from '../../paraglide/messages.js';
import { SvelteMap } from 'svelte/reactivity';
import * as api from '../api';
import { play } from '../sound';
import { lockedImageOf } from '../emblem';
import { errorCode, errorText } from '../errors';
import type { Backend, Emblem, LockReason, RecentEntry, VaultId, VaultInfo, VaultLocked, Visibility } from '../types';
import { app } from './app.svelte';
import { FileBrowser } from './browser.svelte';
import { idle } from './idle.svelte';
import { photos } from './photos.svelte';
import { jobs } from './jobs.svelte';
import { toasts } from './toasts.svelte';

export type UnlockPhase = 'idle' | 'unlocking' | 'error' | 'success';

export interface UnlockState {
  phase: UnlockPhase;
  /** performance.now() when key derivation started */
  startedAt: number;
  /** expected duration of the key derivation in ms (from the last unlock of this volume) */
  expectedMs: number;
  /** measured duration of the last finished attempt */
  tookMs: number | null;
  error: string | null;
  /** increments on every failed attempt (key for a one-shot shake animation) */
  errorSeq: number;
}

const DEFAULT_UNLOCK_MS = 700;

class Vaults {
  open = $state<VaultInfo[]>([]);
  activeId = $state<VaultId | null>(null);
  browsers = new SvelteMap<VaultId, FileBrowser>();
  unlock = $state<UnlockState>({
    phase: 'idle',
    startedAt: 0,
    expectedMs: DEFAULT_UNLOCK_MS,
    tookMs: null,
    error: null,
    errorSeq: 0,
  });
  backends = $state<Backend[]>([]);
  /** performance.now() of the last successful unlock (the file view decodes inside its ceremony) */
  unlockedAt = -1e9;
  mountBusy = $state(false);

  active = $derived(this.open.find((v) => v.id === this.activeId) ?? null);
  browser = $derived(this.activeId ? (this.browsers.get(this.activeId) ?? null) : null);
  canMount = $derived(this.backends.some((b) => b.available));

  private adopt(info: VaultInfo) {
    const i = this.open.findIndex((v) => v.id === info.id);
    if (i >= 0) this.open[i] = info;
    else this.open.push(info);
    if (!this.browsers.has(info.id)) this.browsers.set(info.id, new FileBrowser(info));
    this.activeId = info.id;
    if (info.appearance_tampered) {
      toasts.push(m.toast_appearance_tampered({ name: info.name }), 'error', 0);
    }
    idle.startVault(info.id, info.settings?.auto_lock_minutes);
    void this.browsers.get(info.id)!.load();
    void this.loadBackends();
  }

  /** Unlock flow for Sblocca. Resolves true on success (screen switches to the vault). */
  async unlockVault(target: RecentEntry, password: string): Promise<boolean> {
    if (this.unlock.phase === 'unlocking') return false;
    const t0 = performance.now();
    this.unlock = {
      ...this.unlock,
      phase: 'unlocking',
      startedAt: t0,
      expectedMs: target.unlock_ms ?? DEFAULT_UNLOCK_MS,
      tookMs: null,
      error: null,
    };
    try {
      const info = await api.vaultOpen(target.path, password);
      this.unlock = { ...this.unlock, phase: 'success', tookMs: performance.now() - t0 };
      this.unlockedAt = performance.now();
      this.adopt(info);
      play('unlock');
      app.lockReason = null;
      app.goto('vault');
      void app.loadRecents();
      return true;
    } catch (e) {
      const code = errorCode(e);
      if (code === 'wrong_password') play('wrong');
      this.unlock = {
        ...this.unlock,
        phase: 'error',
        tookMs: performance.now() - t0,
        error: code === 'wrong_password' ? m.err_wrong_password() : errorText(e),
        errorSeq: this.unlock.errorSeq + 1,
      };
      return false;
    }
  }

  resetUnlock() {
    this.unlock = { ...this.unlock, phase: 'idle', error: null };
  }

  /** Create + open a new volume. */
  async createVault(path: string, password: string, fixedSize?: number | null): Promise<VaultInfo> {
    const info = await api.vaultCreate(path, password, 'strong', fixedSize);
    this.adopt(info);
    void app.loadRecents();
    return info;
  }

  activate(id: VaultId) {
    if (this.open.some((v) => v.id === id)) {
      this.activeId = id;
      if (app.screen !== 'vault' && app.screen !== 'details' && app.screen !== 'passwords') app.goto('vault');
    }
  }

  async lock(id: VaultId | null = this.activeId, reason: LockReason = 'user') {
    if (!id) return;
    try {
      await api.vaultLock(id, reason);
    } catch (e) {
      toasts.error(e);
    }
  }

  /** vault-locked event (user, idle, removed, sleep, screen). */
  handleLocked(evt: VaultLocked) {
    const info = this.open.find((v) => v.id === evt.vault);
    if (evt.reason === 'user') play('lock');
    this.open = this.open.filter((v) => v.id !== evt.vault);
    this.browsers.get(evt.vault)?.dispose();
    this.browsers.delete(evt.vault);
    jobs.dropVault(evt.vault);
    photos.dropVault(evt.vault);
    idle.stopVault(evt.vault);
    const wasActive = this.activeId === evt.vault;
    if (this.open.length === 0) {
      idle.stop();
      // nothing is open any more: toasts like "Importato: <file name>." must not
      // linger on the lock screen (a removed/idle lock happens unattended)
      toasts.clear();
    }
    if (!wasActive) {
      if (info && evt.reason !== 'user') toasts.push(m.toast_locked({ name: info.name }), 'info');
      return;
    }
    if (evt.reason === 'user' && this.open.length > 0) {
      this.activeId = this.open[0].id;
      return;
    }
    this.activeId = this.open[0]?.id ?? null;
    if (info) {
      const recent = app.recents.find((r) => r.path === info.path);
      this.resetUnlock();
      app.showUnlock(
        recent ?? { path: info.path, name: info.name, size_bytes: 0, last_opened: Date.now(), exists: true },
        evt.reason === 'user' ? null : evt.reason,
      );
    } else {
      app.goto('welcome');
    }
    void app.loadRecents();
  }

  /**
   * Store the emblem inside the volume; showWhenLocked = visibility "visible" (the header gets
   * accent + a small copy of the emblem, made here for images). Refreshes the open info and Benvenuto.
   */
  async setEmblem(id: VaultId, emblem: Emblem, showWhenLocked: boolean, frame?: string): Promise<boolean> {
    try {
      const locked = emblem.kind === 'image' ? await lockedImageOf(emblem.data_base64) : null;
      const st = await api.vaultEmblemSet(id, emblem, showWhenLocked, locked, frame);
      const v = this.open.find((x) => x.id === id);
      if (v) {
        v.emblem = st.emblem;
        v.show_when_locked = st.show_when_locked;
        v.visibility = st.show_when_locked ? 'visible' : 'encrypted';
        if (st.emblem_frame) v.emblem_frame = st.emblem_frame;
      }
      void app.loadRecents();
      return true;
    } catch (e) {
      toasts.error(e);
      return false;
    }
  }

  /** "Aspetto visibile da chiuso" (visible) / "Aspetto cifrato" (encrypted) for a volume. */
  async setVisibility(id: VaultId, visibility: Visibility): Promise<boolean> {
    try {
      const st = await api.vaultAppearanceSet(id, { visibility });
      const v = this.open.find((x) => x.id === id);
      if (v) {
        v.visibility = st.visibility;
        v.show_when_locked = st.visibility === 'visible';
      }
      void app.loadRecents();
      return true;
    } catch (e) {
      toasts.error(e);
      return false;
    }
  }

  async loadBackends() {
    try {
      this.backends = await api.mountAvailable();
    } catch (e) {
      this.backends = [];
      toasts.error(e);
    }
  }

  async toggleMount(id: VaultId | null = this.activeId) {
    const v = this.open.find((x) => x.id === id);
    if (!v || this.mountBusy) return;
    this.mountBusy = true;
    try {
      if (v.mountpoint) {
        await api.unmount(v.id);
        v.mountpoint = null;
      } else {
        const r = await api.mount(v.id);
        v.mountpoint = r.mountpoint;
      }
    } catch (e) {
      toasts.error(e);
    } finally {
      this.mountBusy = false;
    }
  }
}

export const vaults = new Vaults();

// settingsGet()/settingsSet() act on the volume on screen.
api.bindActiveVault(() => vaults.activeId);
// the idle clock that user input bumps is the one of the volume on screen
idle.bindActive(() => vaults.activeId);
