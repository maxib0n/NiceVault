import * as api from '../api';
import type { LockReason, RecentEntry } from '../types';
import { toasts } from './toasts.svelte';
import { m } from '../../paraglide/messages.js';

export type Screen =
  | 'welcome' | 'create' | 'unlock' | 'vault' | 'details' | 'passwords' | 'settings' | 'device';

export const LOCKED_SCREENS: ReadonlySet<Screen> = new Set([
  'welcome', 'create', 'unlock', 'settings', 'device',
]);
const VAULT_SCREENS: ReadonlySet<Screen> = new Set(['vault', 'details', 'passwords']);

const LOCK_REASON: Record<LockReason, (() => string) | null> = {
  user: null,
  idle: m.lock_idle,
  removed: m.lock_removed,
  sleep: m.lock_sleep,
  screen: m.lock_screen,
};

export const lockReasonText = (r: LockReason) => LOCK_REASON[r]?.() ?? null;

class AppState {
  screen = $state<Screen>('welcome');
  unlockTarget = $state<RecentEntry | null>(null);
  lockReason = $state<LockReason | null>(null);
  recents = $state<RecentEntry[]>([]);
  recentsLoaded = $state(false);

  world = $derived(LOCKED_SCREENS.has(this.screen) ? 'locked' : 'open');

  settingsFrom = $state<Screen>('welcome');

  page = $derived(this.screen === 'settings' ? this.settingsFrom : this.screen);
  vaultMounted = $derived(VAULT_SCREENS.has(this.page));
  covered = $derived(this.screen === 'settings');

  goto(screen: Screen) {
    this.screen = screen;
  }

  openSettings() {
    if (this.screen === 'settings') return;
    this.settingsFrom =
      this.screen === 'unlock' || this.screen === 'create' || this.screen === 'device' ? 'welcome' : this.screen;
    this.screen = 'settings';
  }

  closeSettings() {
    if (this.screen === 'settings') this.screen = this.settingsFrom;
  }

  async loadRecents() {
    try {
      this.recents = await api.recentList();
    } catch (e) {
      toasts.error(e);
    } finally {
      this.recentsLoaded = true;
    }
  }

  async forgetRecent(path: string) {
    try {
      this.recents = await api.recentForget(path);
    } catch (e) {
      toasts.error(e);
    }
  }

  showUnlock(target: RecentEntry, reason: LockReason | null = null) {
    this.unlockTarget = target;
    this.lockReason = reason;
    this.screen = 'unlock';
  }

  async takeLaunchPath(openPaths: { id: string; path: string }[], activate: (id: string) => void) {
    const path = await api.launchPathTake().catch(() => null);
    if (!path) return;
    const open = openPaths.find((v) => v.path === path);
    if (open) activate(open.id);
    else this.showUnlockPath(path);
  }

  showUnlockPath(path: string) {
    const known = this.recents.find((r) => r.path === path);
    const name = path.split(/[\\/]/).pop()?.replace(/\.nvault$/i, '') ?? path;
    this.showUnlock(known ?? { path, name, size_bytes: 0, last_opened: 0, exists: true });
  }
}

export const app = new AppState();
