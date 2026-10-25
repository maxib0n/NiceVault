// Idle auto-lock, one clock PER VOLUME (Settings.auto_lock_minutes of each, 0 = never).
// User input bumps the clock of the volume on screen and tells the backend (vault_touch,
// throttled). The countdown is real and can be shown in the UI (DESIGN: "the auto-lock
// countdown is real"): `remainingMs` is the active volume's, `remainingFor(id)` any volume's.
//
// Who locks:
//   - Tauri: the backend owns the timers (app/src-tauri/src/autolock.rs: it also counts jobs,
//     previews and the mounted drive as activity) and emits vault-locked "idle". This store only
//     mirrors them, re-synced from autolock_status every few seconds; it never locks by itself.
//   - mock / browser: there is no backend ticker, so an expired clock calls `onExpire`
//     (App.svelte: lockAllIdle), which locks exactly the expired volumes (all open ones when
//     one of them has lock_scope "all").
import { untrack } from 'svelte';
import { SvelteMap } from 'svelte/reactivity';
import * as api from '../api';
import type { VaultId } from '../types';

interface Clock {
  /** 0 = never */
  timeoutMs: number;
  lastActivity: number;
}

/** vault_touch at most this often per volume (the backend's resolution is 1 s anyway). */
export const TOUCH_EVERY_MS = 15_000;
/** Tauri: re-read the backend's countdowns this often. */
export const SYNC_EVERY_MS = 5_000;

class Idle {
  /** one clock per open volume */
  clocks = new SvelteMap<VaultId, Clock>();
  /** timeout for volumes opened from now on when their settings are not known (session default) */
  defaultMs = $state(15 * 60_000);
  /** ?autolock=N (seconds) for screenshots and flows: overrides every volume's minutes */
  forcedMs = $state<number | null>(null);
  now = $state(Date.now());
  active = $state(false);

  private activeId: () => VaultId | null = () => null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private onExpire: (() => void) | null = null;
  private lastBump = 0;
  private lastSync = 0;
  private lastTouch = new Map<VaultId, number>();
  private expired: VaultId[] = [];

  /** The vaults store tells which volume is on screen. */
  bindActive(get: () => VaultId | null): void {
    this.activeId = get;
  }

  /** Timeout of the volume on screen (no volume: the default). Setting it forces every volume (seconds switch). */
  get timeoutMs(): number {
    const id = this.activeId();
    const c = id ? this.clocks.get(id) : undefined;
    return c ? c.timeoutMs : (this.forcedMs ?? this.defaultMs);
  }
  set timeoutMs(ms: number) {
    this.force(ms);
  }

  /** Time left on the volume on screen (Infinity = never). */
  get remainingMs(): number {
    return this.remainingFor(this.activeId());
  }

  /** Time left before volume `id` locks for inactivity (Infinity = never / unknown). */
  remainingFor(id: VaultId | null | undefined): number {
    const c = id ? this.clocks.get(id) : undefined;
    if (!c) return Infinity;
    return c.timeoutMs > 0 ? Math.max(0, c.timeoutMs - (this.now - c.lastActivity)) : Infinity;
  }

  bump = () => {
    const t = Date.now();
    if (t - this.lastBump < 500) return; // throttle pointermove storms
    this.lastBump = t;
    this.now = t;
    const id = this.activeId();
    const c = id ? this.clocks.get(id) : undefined;
    if (!id || !c) return;
    this.clocks.set(id, { ...c, lastActivity: t });
    if (t - (this.lastTouch.get(id) ?? 0) >= TOUCH_EVERY_MS) {
      this.lastTouch.set(id, t);
      void api.vaultTouch(id).catch(() => undefined);
    }
  };

  /** Install activity listeners; returns a disposer. */
  install(onExpire: () => void): () => void {
    this.onExpire = onExpire;
    const evs = ['pointerdown', 'pointermove', 'keydown', 'wheel', 'touchstart', 'dragover'] as const;
    for (const e of evs) window.addEventListener(e, this.bump, { passive: true, capture: true });
    return () => {
      for (const e of evs) window.removeEventListener(e, this.bump, { capture: true });
      this.stop();
    };
  }

  /** A volume was opened: start its clock (`minutes` = its Settings.auto_lock_minutes). */
  startVault(id: VaultId, minutes?: number | null): void {
    untrack(() => {
      const t = Date.now();
      const known = typeof minutes === 'number' ? Math.max(0, minutes) * 60_000 : (this.clocks.get(id)?.timeoutMs ?? this.defaultMs);
      this.clocks.set(id, { timeoutMs: this.forcedMs ?? known, lastActivity: t });
      this.lastTouch.set(id, t);
      this.start();
    });
  }

  /** A volume was locked: drop its clock (and stop ticking when none is left). */
  stopVault(id: VaultId): void {
    untrack(() => {
      this.clocks.delete(id);
      this.lastTouch.delete(id);
      this.expired = this.expired.filter((x) => x !== id);
      if (this.clocks.size === 0) this.stop();
    });
  }

  /** Start ticking (a volume is open). */
  start() {
    this.active = true;
    this.now = Date.now();
    this.timer ??= setInterval(() => this.tick(Date.now()), 1000);
  }

  /** One second of the clock: exposed for tests. */
  tick(now: number): void {
    this.now = now;
    if (api.isTauri) {
      if (now - this.lastSync >= SYNC_EVERY_MS) {
        this.lastSync = now;
        void this.sync();
      }
      return; // the backend locks
    }
    let fired = false;
    for (const [id, c] of this.clocks) {
      if (c.timeoutMs > 0 && now - c.lastActivity >= c.timeoutMs) {
        this.clocks.set(id, { ...c, lastActivity: now }); // fire once
        if (!this.expired.includes(id)) this.expired.push(id);
        fired = true;
      }
    }
    if (fired) this.onExpire?.();
  }

  /** Align the clocks with the backend's countdowns (it sees jobs, previews, the mounted drive). */
  async sync(): Promise<void> {
    const list = await api.autolockStatus().catch(() => null);
    if (!list) return;
    const now = Date.now();
    for (const s of list) {
      const c = this.clocks.get(s.vault);
      if (!c) continue;
      const timeoutMs = this.forcedMs ?? s.auto_lock_minutes * 60_000;
      // null while a job runs (never idle) or minutes 0
      const backendLast = s.remaining_ms === null ? now : now - (timeoutMs - s.remaining_ms);
      this.clocks.set(s.vault, { timeoutMs, lastActivity: Math.max(c.lastActivity, backendLast) });
    }
  }

  /** Volumes whose clock ran out since the last call (then forgotten). */
  takeExpired(): VaultId[] {
    const e = this.expired;
    this.expired = [];
    return e;
  }

  stop() {
    this.active = false;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  /** Minutes of the volume on screen (none: the default for the next volume). */
  // Actions run untracked: look.svelte.ts calls them from an $effect, which must not come to
  // depend on the clocks it writes.
  setMinutes(min: number) {
    untrack(() => {
      const id = this.activeId();
      if (id && this.clocks.has(id)) this.setMinutesFor(id, min);
      else this.defaultMs = Math.max(0, min) * 60_000;
    });
  }

  /** Minutes of volume `id` (its settings changed). Ignored while forced (?autolock=). */
  setMinutesFor(id: VaultId, min: number) {
    untrack(() => {
      const c = this.clocks.get(id);
      const timeoutMs = Math.max(0, min) * 60_000;
      if (!c || this.forcedMs !== null || c.timeoutMs === timeoutMs) return;
      this.clocks.set(id, { ...c, timeoutMs });
    });
  }

  /** Same timeout for every volume, now and later (the ?autolock=N switch). */
  force(ms: number) {
    untrack(() => {
      const f = (this.forcedMs = Math.max(0, ms));
      this.defaultMs = f;
      for (const [id, c] of this.clocks) this.clocks.set(id, { ...c, timeoutMs: f });
    });
  }
}

export const idle = new Idle();

/**
 * The idle store's onExpire (mock / browser; in Tauri the backend locks): lock the volumes whose
 * clock ran out with reason "idle" — every open one when one of them has lock_scope "all".
 * Called with nothing expired, it locks every open volume (previous behaviour).
 */
export async function lockAllIdle() {
  const open = await api.vaultListOpen().catch(() => []);
  const expired = new Set(idle.takeExpired());
  const due = expired.size ? open.filter((v) => expired.has(v.id)) : open;
  const all = due.some((v) => v.settings?.lock_scope === 'all');
  const targets = all || !expired.size ? open : due;
  await Promise.all(targets.map((v) => api.vaultLock(v.id, 'idle').catch(() => undefined)));
}
