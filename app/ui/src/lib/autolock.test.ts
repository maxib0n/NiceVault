import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from './api';
import { MOCK_DURESS_PASSWORD, MOCK_PASSWORD, MockBackend, STICK } from './mock';
import { idle, lockAllIdle } from './stores/idle.svelte';
import type { VaultLocked } from './types';

const DOC = `${STICK}/Documenti.nvault`;
const ARC = `${STICK}/Archivio fiscale.nvault`;
const DIA = `${STICK}/Diario.nvault`;
const MIN = 60_000;

describe('auto-lock options (mock backend)', () => {
  let m: MockBackend;
  beforeEach(() => api.useMock((m = new MockBackend({ speed: 0 }))));

  it('settings carry the auto-lock options, per volume, unknown scope ignored', async () => {
    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(doc.settings).toMatchObject({ lock_on_sleep: true, lock_on_screen_lock: true, lock_scope: 'volume' });
    const s = await api.settingsSet({ auto_lock_minutes: 0, lock_on_sleep: false, lock_scope: 'all' }, doc.id);
    expect(s).toMatchObject({ auto_lock_minutes: 0, lock_on_sleep: false, lock_on_screen_lock: true, lock_scope: 'all' });
    expect((await api.settingsSet({ lock_scope: 'galassia' as never }, doc.id)).lock_scope).toBe('all');
    const arc = await api.vaultOpen(ARC, MOCK_PASSWORD);
    expect((await api.settingsGet(arc.id)).lock_scope).toBe('volume');
  });

  it('each volume has its own idle countdown; touching one restarts only its clock', async () => {
    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const arc = await api.vaultOpen(ARC, MOCK_PASSWORD);
    await api.settingsSet({ auto_lock_minutes: 1 }, doc.id);
    await api.settingsSet({ auto_lock_minutes: 0 }, arc.id);
    const t0 = Date.now();
    const st = m.autolockStatus(t0 + 30_000);
    expect(st.find((s) => s.vault === arc.id)?.remaining_ms).toBeNull();
    expect(st.find((s) => s.vault === doc.id)?.remaining_ms).toBeLessThanOrEqual(30_000);
    const locked: VaultLocked[] = [];
    await api.onVaultLocked((e) => locked.push(e));
    expect(m.idleTick(t0 + 2 * MIN)).toEqual([doc.id]);
    expect(locked).toEqual([{ vault: doc.id, reason: 'idle' }]);
    expect(m.idleTick(t0 + 24 * 60 * MIN)).toEqual([]);
    await api.vaultTouch(arc.id);
    await expect(api.vaultTouch(doc.id)).rejects.toMatchObject({ code: 'locked' });
  });

  it('sleep / screen triggers follow each volume; scope "all" takes every volume', async () => {
    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const arc = await api.vaultOpen(ARC, MOCK_PASSWORD);
    await api.settingsSet({ lock_on_screen_lock: false }, doc.id);
    expect(m.simulateLockEvent('screen')).toEqual([arc.id]);
    const dia = await api.vaultOpen(DIA, MOCK_PASSWORD);
    await api.settingsSet({ lock_on_sleep: false }, doc.id);
    await api.settingsSet({ lock_scope: 'all' }, dia.id);
    expect(m.simulateLockEvent('sleep').sort()).toEqual([doc.id, dia.id].sort());
    expect(await api.vaultListOpen()).toEqual([]);
  });
});

describe('duress password (mock backend)', () => {
  beforeEach(() => api.useMock(new MockBackend({ speed: 0 })));

  it('Documenti is seeded with "panico": entering it destroys the volume like a wrong password', async () => {
    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const slots = await api.slotsList(doc.id);
    expect(slots.map((s) => s.kind)).toEqual(['password', 'password', 'duress']);
    expect(slots[2]).toMatchObject({ in_use: false, kdf: slots[0].kdf });
    await api.vaultLock(doc.id);
    const wrong = await api.vaultOpen(DOC, 'sbagliata').catch((e) => e);
    const duress = await api.vaultOpen(DOC, MOCK_DURESS_PASSWORD).catch((e) => e);
    expect(duress).toEqual(wrong);
    expect(duress).toMatchObject({ code: 'wrong_password' });
    await expect(api.vaultOpen(DOC, MOCK_PASSWORD)).rejects.toMatchObject({ code: 'wrong_password' });
    expect((await api.vaultOpen(ARC, MOCK_PASSWORD)).name).toBe('Archivio fiscale');
  });

  it('add / remove: never the only key, never equal to another password, not changeable', async () => {
    const arc = await api.vaultOpen(ARC, MOCK_PASSWORD);
    await expect(api.slotAddDuress(arc.id, MOCK_PASSWORD)).rejects.toMatchObject({ code: 'duress_conflict' });
    await expect(api.slotAddDuress(arc.id, '')).rejects.toMatchObject({ code: 'invalid_password' });
    const slots = await api.slotAddDuress(arc.id, 'rapina', 'Rapina');
    const d = slots.find((s) => s.kind === 'duress')!;
    expect(d.label).toBe('Rapina');
    await expect(api.slotAddPassword(arc.id, 'rapina')).rejects.toMatchObject({ code: 'duress_conflict' });
    await expect(api.slotChangePassword(arc.id, d.index, MOCK_PASSWORD, 'x')).rejects.toMatchObject({ code: 'invalid_slot' });
    await api.slotRemove(arc.id, 1);
    await expect(api.slotRemove(arc.id, 0)).rejects.toMatchObject({ code: 'last_slot' });
    expect((await api.slotRemove(arc.id, d.index)).map((s) => s.kind)).toEqual(['password']);
    await api.vaultLock(arc.id);
    await expect(api.vaultOpen(ARC, 'rapina')).rejects.toMatchObject({ code: 'wrong_password' });
    expect((await api.vaultOpen(ARC, MOCK_PASSWORD)).name).toBe('Archivio fiscale');
  });
});

describe('idle store: one clock per volume', () => {
  let active: string | null = null;
  let expiredCalls = 0;
  beforeEach(() => {
    vi.useFakeTimers();
    api.useMock(new MockBackend({ speed: 0 }));
    (globalThis as { window?: unknown }).window ??= { addEventListener() {}, removeEventListener() {} };
    idle.bindActive(() => active);
    idle.install(() => {
      expiredCalls++;
      void lockAllIdle();
    });
    expiredCalls = 0;
  });
  afterEach(() => {
    for (const id of [...idle.clocks.keys()]) idle.stopVault(id);
    vi.useRealTimers();
  });

  it('expires each volume on its own minutes, locking only that one', async () => {
    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const arc = await api.vaultOpen(ARC, MOCK_PASSWORD);
    idle.startVault(doc.id, 1);
    idle.startVault(arc.id, 5);
    active = doc.id;
    expect(idle.timeoutMs).toBe(MIN);
    const t0 = Date.now();
    idle.tick(t0 + 30_000);
    expect(idle.remainingMs).toBe(30_000);
    expect(idle.remainingFor(arc.id)).toBe(4.5 * MIN);
    vi.setSystemTime(t0 + 50_000);
    idle.bump();
    idle.tick(t0 + 61_000);
    expect(expiredCalls).toBe(0);
    idle.tick(t0 + 111_000);
    expect(expiredCalls).toBe(1);
    await vi.waitFor(async () => expect((await api.vaultListOpen()).map((v) => v.id)).toEqual([arc.id]));
    idle.stopVault(doc.id);
    idle.tick(t0 + 5 * MIN + 1);
    await vi.waitFor(async () => expect(await api.vaultListOpen()).toEqual([]));
  });

  it('minutes 0 = never; settings changes reach the right volume; the ?autolock switch forces all', async () => {
    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    idle.startVault(doc.id, 0);
    active = doc.id;
    expect(idle.remainingMs).toBe(Infinity);
    idle.tick(Date.now() + 48 * 60 * MIN);
    expect(expiredCalls).toBe(0);
    idle.setMinutesFor(doc.id, 2);
    expect(idle.timeoutMs).toBe(2 * MIN);
    idle.force(3000);
    expect(idle.timeoutMs).toBe(3000);
    idle.setMinutesFor(doc.id, 10);
    expect(idle.timeoutMs).toBe(3000);
    idle.forcedMs = null;
  });

  it('a scope "all" volume takes every open one with it', async () => {
    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const arc = await api.vaultOpen(ARC, MOCK_PASSWORD);
    await api.settingsSet({ lock_scope: 'all' }, doc.id);
    idle.startVault(doc.id, 1);
    idle.startVault(arc.id, 60);
    idle.tick(Date.now() + 2 * MIN);
    await vi.waitFor(async () => expect(await api.vaultListOpen()).toEqual([]));
  });
});
