// Top-level navigation state: which screen, which volume is being unlocked,
// why the last volume was locked, the recent list.
import * as api from '../api';
import type { LockReason, RecentEntry } from '../types';
import { toasts } from './toasts.svelte';
import { m } from '../../paraglide/messages.js';

export type Screen =
  | 'welcome' | 'create' | 'unlock' | 'vault' | 'details' | 'passwords' | 'settings' | 'device';

/** Screens of the "locked world" (metal) vs the "open world" (paper).
 *  Impostazioni (global, not a volume's) is on the board too. */
export const LOCKED_SCREENS: ReadonlySet<Screen> = new Set([
  'welcome', 'create', 'unlock', 'settings', 'device',
]);
/** The open volume's pages (VaultScreen). */
const VAULT_SCREENS: ReadonlySet<Screen> = new Set(['vault', 'details', 'passwords']);

const LOCK_REASON: Record<LockReason, (() => string) | null> = {
  user: null,
  idle: m.lock_idle,
  removed: m.lock_removed,
  sleep: m.lock_sleep,
  screen: m.lock_screen,
};

/** Why Sblocca is shown, in the UI language (null for the person's own Blocca). */
export const lockReasonText = (r: LockReason) => LOCK_REASON[r]?.() ?? null;

class AppState {
  screen = $state<Screen>('welcome');
  /** volume shown on Sblocca */
  unlockTarget = $state<RecentEntry | null>(null);
  /** why we are on Sblocca (null = user's choice) */
  lockReason = $state<LockReason | null>(null);
  recents = $state<RecentEntry[]>([]);
  recentsLoaded = $state(false);

  world = $derived(LOCKED_SCREENS.has(this.screen) ? 'locked' : 'open');

  /** where Impostazioni goes back to */
  settingsFrom = $state<Screen>('welcome');

  /** the page under Impostazioni too: the open volume stays mounted (covered, inert) while
   *  Impostazioni is open over it, so Indietro shows it again without re-mounting it */
  page = $derived(this.screen === 'settings' ? this.settingsFrom : this.screen);
  vaultMounted = $derived(VAULT_SCREENS.has(this.page));
  /** Impostazioni is open over the volume: its own keys and mouse buttons stand down */
  covered = $derived(this.screen === 'settings');

  goto(screen: Screen) {
    this.screen = screen;
  }

  /** Impostazioni (Ctrl/Cmd+,): from any screen; Indietro returns there. */
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

  /** Go to Sblocca for a volume file. */
  showUnlock(target: RecentEntry, reason: LockReason | null = null) {
    this.unlockTarget = target;
    this.lockReason = reason;
    this.screen = 'unlock';
  }

  /** A volume file arrived from the command line / a second launch: its unlock screen, or the
   *  volume itself when it is already open. */
  async takeLaunchPath(openPaths: { id: string; path: string }[], activate: (id: string) => void) {
    const path = await api.launchPathTake().catch(() => null);
    if (!path) return;
    const open = openPaths.find((v) => v.path === path);
    if (open) activate(open.id);
    else this.showUnlockPath(path);
  }

  /** Unlock a path picked from a dialog (may not be in the recent list yet). */
  showUnlockPath(path: string) {
    const known = this.recents.find((r) => r.path === path);
    const name = path.split(/[\\/]/).pop()?.replace(/\.nvault$/i, '') ?? path;
    this.showUnlock(known ?? { path, name, size_bytes: 0, last_opened: 0, exists: true });
  }
}

export const app = new AppState();
