import { untrack } from 'svelte';
import { SvelteMap } from 'svelte/reactivity';
import * as api from '../api';
import type { VaultId } from '../types';

interface Clock {
  timeoutMs: number;
  lastActivity: number;
}

export const TOUCH_EVERY_MS = 15_000;
export const SYNC_EVERY_MS = 5_000;

class Idle {
  clocks = new SvelteMap<VaultId, Clock>();
  defaultMs = $state(15 * 60_000);
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

  bindActive(get: () => VaultId | null): void {
    this.activeId = get;
  }

  get timeoutMs(): number {
    const id = this.activeId();
    const c = id ? this.clocks.get(id) : undefined;
    return c ? c.timeoutMs : (this.forcedMs ?? this.defaultMs);
  }
  set timeoutMs(ms: number) {
    this.force(ms);
  }

  get remainingMs(): number {
    return this.remainingFor(this.activeId());
  }

  remainingFor(id: VaultId | null | undefined): number {
    const c = id ? this.clocks.get(id) : undefined;
    if (!c) return Infinity;
    return c.timeoutMs > 0 ? Math.max(0, c.timeoutMs - (this.now - c.lastActivity)) : Infinity;
  }

  bump = () => {
    const t = Date.now();
    if (t - this.lastBump < 500) return;
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

  install(onExpire: () => void): () => void {
    this.onExpire = onExpire;
    const evs = ['pointerdown', 'pointermove', 'keydown', 'wheel', 'touchstart', 'dragover'] as const;
    for (const e of evs) window.addEventListener(e, this.bump, { passive: true, capture: true });
    return () => {
      for (const e of evs) window.removeEventListener(e, this.bump, { capture: true });
      this.stop();
    };
  }

  startVault(id: VaultId, minutes?: number | null): void {
    untrack(() => {
      const t = Date.now();
      const known = typeof minutes === 'number' ? Math.max(0, minutes) * 60_000 : (this.clocks.get(id)?.timeoutMs ?? this.defaultMs);
      this.clocks.set(id, { timeoutMs: this.forcedMs ?? known, lastActivity: t });
      this.lastTouch.set(id, t);
      this.start();
    });
  }

  stopVault(id: VaultId): void {
    untrack(() => {
      this.clocks.delete(id);
      this.lastTouch.delete(id);
      this.expired = this.expired.filter((x) => x !== id);
      if (this.clocks.size === 0) this.stop();
    });
  }

  start() {
    this.active = true;
    this.now = Date.now();
    this.timer ??= setInterval(() => this.tick(Date.now()), 1000);
  }

  tick(now: number): void {
    this.now = now;
    if (api.isTauri) {
      if (now - this.lastSync >= SYNC_EVERY_MS) {
        this.lastSync = now;
        void this.sync();
      }
      return;
    }
    let fired = false;
    for (const [id, c] of this.clocks) {
      if (c.timeoutMs > 0 && now - c.lastActivity >= c.timeoutMs) {
        this.clocks.set(id, { ...c, lastActivity: now });
        if (!this.expired.includes(id)) this.expired.push(id);
        fired = true;
      }
    }
    if (fired) this.onExpire?.();
  }

  async sync(): Promise<void> {
    const list = await api.autolockStatus().catch(() => null);
    if (!list) return;
    const now = Date.now();
    for (const s of list) {
      const c = this.clocks.get(s.vault);
      if (!c) continue;
      const timeoutMs = this.forcedMs ?? s.auto_lock_minutes * 60_000;
      const backendLast = s.remaining_ms === null ? now : now - (timeoutMs - s.remaining_ms);
      this.clocks.set(s.vault, { timeoutMs, lastActivity: Math.max(c.lastActivity, backendLast) });
    }
  }

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

  setMinutes(min: number) {
    untrack(() => {
      const id = this.activeId();
      if (id && this.clocks.has(id)) this.setMinutesFor(id, min);
      else this.defaultMs = Math.max(0, min) * 60_000;
    });
  }

  setMinutesFor(id: VaultId, min: number) {
    untrack(() => {
      const c = this.clocks.get(id);
      const timeoutMs = Math.max(0, min) * 60_000;
      if (!c || this.forcedMs !== null || c.timeoutMs === timeoutMs) return;
      this.clocks.set(id, { ...c, timeoutMs });
    });
  }

  force(ms: number) {
    untrack(() => {
      const f = (this.forcedMs = Math.max(0, ms));
      this.defaultMs = f;
      for (const [id, c] of this.clocks) this.clocks.set(id, { ...c, timeoutMs: f });
    });
  }
}

export const idle = new Idle();

export async function lockAllIdle() {
  const open = await api.vaultListOpen().catch(() => []);
  const expired = new Set(idle.takeExpired());
  const due = expired.size ? open.filter((v) => expired.has(v.id)) : open;
  const all = due.some((v) => v.settings?.lock_scope === 'all');
  const targets = all || !expired.size ? open : due;
  await Promise.all(targets.map((v) => api.vaultLock(v.id, 'idle').catch(() => undefined)));
}
