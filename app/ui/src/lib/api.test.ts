import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';
import type { JobDone, JobProgress } from './types';
import { ROOT } from './types';

const DOC = `${STICK}/Documenti.nvault`;

async function collect(start: () => Promise<string>): Promise<{ progress: JobProgress[]; done: JobDone }> {
  const progress: JobProgress[] = [];
  const dones: JobDone[] = [];
  const offP = await api.onJobProgress((p) => progress.push(p));
  let resolveDone!: () => void;
  const finished = new Promise<void>((r) => (resolveDone = r));
  const offD = await api.onJobDone((d) => {
    dones.push(d);
    resolveDone();
  });
  const job = await start();
  await finished;
  offP();
  offD();
  return { progress: progress.filter((p) => p.job === job), done: dones.find((d) => d.job === job)! };
}

describe('api (mock backend)', () => {
  beforeEach(() => api.useMock(new MockBackend({ speed: 0 })));

  it('upgrades a volume to the single-file format (vault_upgrade_format), once', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(v.single_file).toBe(false);
    expect(v.can_upgrade_format).toBe(true);
    const u = await api.vaultUpgradeFormat(v.id);
    expect(u).toMatchObject({ id: v.id, single_file: true, can_upgrade_format: false });
    expect((await api.vaultListOpen()).find((x) => x.id === v.id)?.single_file).toBe(true);
    expect((await api.vaultUpgradeFormat(v.id)).single_file).toBe(true);
  });

  it('creates new volumes single-file, growable and fixed-size alike', async () => {
    const v = await api.vaultCreate(`${STICK}/Nuovo.nvault`, 'pw', 'fast');
    expect(v).toMatchObject({ single_file: true, can_upgrade_format: false });
    const f = await api.vaultCreate(`${STICK}/Fisso.nvault`, 'pw', 'fast', 64 << 20);
    expect(f).toMatchObject({ single_file: true, can_upgrade_format: false });
  });

  it('upgrades an older fixed-size volume; a nearly full one only once extended (volume_full)', async () => {
    api.useMock(new MockBackend({ speed: 0, fixed: true }));
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(v).toMatchObject({ single_file: false, can_upgrade_format: true });
    expect((await api.vaultUpgradeFormat(v.id)).single_file).toBe(true);
    api.useMock(new MockBackend({ speed: 0, fixed: true, fmtFull: true }));
    const full = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(full).toMatchObject({ single_file: false, can_upgrade_format: false });
    await expect(api.vaultUpgradeFormat(full.id)).rejects.toMatchObject({ code: 'volume_full' });
    const info = await api.vaultResizeInfo(full.id);
    await api.vaultResize(full.id, info.capacity! + 256 * 1024 * 1024);
    expect((await api.vaultListOpen()).find((x) => x.id === full.id)?.can_upgrade_format).toBe(true);
    expect((await api.vaultUpgradeFormat(full.id)).single_file).toBe(true);
  });

  it('is not running inside Tauri under vitest', () => {
    expect(api.isTauri).toBe(false);
  });

  it('lists recent volumes, newest first, including an unreachable one', async () => {
    const r = await api.recentList();
    expect(r.map((x) => x.name)).toEqual(['Documenti', 'Archivio fiscale', 'Diario', 'Vecchio']);
    expect(r[3].exists).toBe(false);
    const after = await api.recentForget(r[3].path);
    expect(after).toHaveLength(3);
  });

  it('rejects a wrong password with code wrong_password', async () => {
    await expect(api.vaultOpen(DOC, 'nope')).rejects.toMatchObject({ code: 'wrong_password' });
  });

  it('opens, lists a seeded tree (~40 files) and locks with an event', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(v.name).toBe('Documenti');
    const root = await api.fsList(v.id, ROOT);
    expect(root.map((n) => n.name)).toContain('Foto');
    const stats = await api.vaultStats(v.id);
    expect(stats.file_count).toBeGreaterThanOrEqual(35);

    const events: string[] = [];
    await api.onVaultLocked((e) => events.push(`${e.vault}:${e.reason}`));
    await api.vaultLock(v.id, 'idle');
    expect(events).toEqual([`${v.id}:idle`]);
    await expect(api.fsList(v.id, ROOT)).rejects.toMatchObject({ code: 'locked' });
  });

  it('mkdir / rename / delete with name validation and clashes', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const d = await api.fsMkdir(v.id, ROOT, 'Nuova');
    expect(d.kind).toBe('dir');
    await expect(api.fsMkdir(v.id, ROOT, 'Nuova')).rejects.toMatchObject({ code: 'exists' });
    await expect(api.fsMkdir(v.id, ROOT, 'a/b')).rejects.toMatchObject({ code: 'invalid_name' });
    await api.fsRename(v.id, d.id, ROOT, 'Rinominata');
    expect((await api.fsList(v.id, ROOT)).some((n) => n.name === 'Rinominata')).toBe(true);
    const { done } = await collect(() => api.fsDelete(v.id, [d.id]));
    expect(done.error ?? null).toBeNull();
    expect((await api.fsList(v.id, ROOT)).some((n) => n.id === d.id)).toBe(false);
  });

  it('delete runs as a job: items and freed bytes, fs-changed per batch, outcome', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const foto = (await api.fsList(v.id, ROOT)).find((n) => n.name === 'Foto')!;
    const changed: number[][] = [];
    const off = await api.onFsChanged((e) => changed.push(e.dirs));
    const { progress, done } = await collect(() => api.fsDelete(v.id, [foto.id]));
    off();
    expect(done.error ?? null).toBeNull();
    const out = done.outcome as { deleted: number; total: number; freed_bytes: number; errors: unknown[] };
    expect(out.deleted).toBe(out.total);
    expect(out.total).toBeGreaterThan(1);
    expect(out.errors).toEqual([]);
    expect(progress.at(-1)).toMatchObject({ done_items: out.total, total_items: out.total, done_bytes: out.freed_bytes });
    expect(changed.some((d) => d.includes(ROOT))).toBe(true);
    expect((await api.fsList(v.id, ROOT)).some((n) => n.id === foto.id)).toBe(false);
    await expect(api.fsDelete(v.id, [ROOT])).rejects.toMatchObject({ code: 'invalid_name' });
  });

  it('delete can be cancelled (the deleted stay deleted) and can fail with no_room_to_delete', async () => {
    const big = new MockBackend({ speed: 1, slowDelete: 30 });
    api.useMock(big);
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const dir = await api.fsMkdir(v.id, ROOT, 'Tanti');
    big.simulateExternalChange(dir.id, { vault: v.id, add: Array.from({ length: 400 }, (_, i) => `f${i}.txt`), emit: false });
    const { done } = await collect(async () => {
      const job = await api.fsDelete(v.id, [dir.id]);
      const off = await api.onJobProgress((p) => {
        if (p.job === job && (p.done_items ?? 0) > 0) void api.jobCancel(job);
      });
      setTimeout(off, 2000);
      return job;
    });
    expect(done.error).toMatchObject({ code: 'cancelled' });
    const left = await api.fsList(v.id, dir.id);
    expect(left.length).toBeGreaterThan(0);
    expect(left.length).toBeLessThan(400);

    api.useMock(new MockBackend({ speed: 0, deleteFails: true }));
    const w = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const foto = (await api.fsList(w.id, ROOT)).find((n) => n.name === 'Foto')!;
    const r = await collect(() => api.fsDelete(w.id, [foto.id]));
    expect(r.done.error).toMatchObject({ code: 'no_room_to_delete' });
    expect((r.done.outcome as { deleted: number }).deleted).toBeGreaterThan(0);
  });

  it('refuses moving a folder into itself', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const foto = (await api.fsList(v.id, ROOT)).find((n) => n.name === 'Foto')!;
    const sub = (await api.fsList(v.id, foto.id)).find((n) => n.kind === 'dir')!;
    await expect(api.fsRename(v.id, foto.id, sub.id, 'Foto')).rejects.toMatchObject({ code: 'invalid_name' });
  });

  it('import runs as a job with progress events and adds files', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const { progress, done } = await collect(() => api.fsImport(v.id, ROOT, ['/home/u/a.pdf', '/home/u/b.jpg']));
    expect(done.error ?? null).toBeNull();
    expect(progress.at(-1)?.done_bytes).toBe(progress.at(-1)?.total_bytes);
    const names = (await api.fsList(v.id, ROOT)).map((n) => n.name);
    expect(names).toEqual(expect.arrayContaining(['a.pdf', 'b.jpg']));
  });

  it('import can fail (error switch) and reports the error in job-done', async () => {
    api.useMock(new MockBackend({ speed: 0, importFails: true }));
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const { done } = await collect(() => api.fsImport(v.id, ROOT, ['/x/big.mkv']));
    expect(done.error).toMatchObject({ code: 'io' });
  });

  it('mount toggles when available and explains when not', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect((await api.mountAvailable()).some((b) => b.available)).toBe(true);
    const { mountpoint } = await api.mount(v.id);
    expect(mountpoint).toContain('Documenti');
    expect((await api.vaultListOpen())[0].mountpoint).toBe(mountpoint);
    await api.unmount(v.id);

    api.useMock(new MockBackend({ speed: 0, mountUnavailable: true }));
    const w = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const backends = await api.mountAvailable();
    expect(backends.every((b) => !b.available && b.reason)).toBe(true);
    await expect(api.mount(w.id)).rejects.toMatchObject({ code: 'mount_unavailable' });
  });

  it('keyslots: add, change (checks old password), refuse removing the last', async () => {
    api.useMock(new MockBackend({ speed: 0, singleSlot: true }));
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(await api.slotsList(v.id)).toHaveLength(1);
    await expect(api.slotRemove(v.id, 0)).rejects.toMatchObject({ code: 'last_slot' });
    const added = await api.slotAddPassword(v.id, 'cavallo batteria graffetta', 'Riserva');
    expect(added).toHaveLength(2);
    await expect(api.slotChangePassword(v.id, 0, 'sbagliata', 'x')).rejects.toMatchObject({ code: 'wrong_password' });
    await api.slotChangePassword(v.id, 0, MOCK_PASSWORD, 'nuova-password-lunga');
    expect(await api.slotRemove(v.id, 1)).toHaveLength(1);
  });

  it('create refuses an existing path and opens the new volume empty', async () => {
    await expect(api.vaultCreate(DOC, 'x')).rejects.toMatchObject({ code: 'exists' });
    const v = await api.vaultCreate(`${STICK}/Nuovo.nvault`, 'una frase lunga e sicura');
    expect(await api.fsList(v.id, ROOT)).toEqual([]);
    expect((await api.recentList())[0].name).toBe('Nuovo');
  });

  it('simulated drive removal locks every open volume with reason "removed"', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const reasons: string[] = [];
    await api.onVaultLocked((e) => reasons.push(e.reason));
    api.mock().simulateRemoval();
    expect(reasons).toEqual(['removed']);
    expect(await api.vaultListOpen()).toEqual([]);
    expect(v.id).toBeTruthy();
  });

  it('preview URLs map to mock assets', async () => {
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const root = await api.fsList(v.id, ROOT);
    const jpg = root.find((n) => n.name.endsWith('.jpg'))!;
    expect(api.previewUrl(v.id, jpg.id)).toMatch(/^\/mock\/photo-\d+\.jpg$/);
  });

  it('settings round-trip', async () => {
    expect((await api.settingsGet()).auto_lock_minutes).toBe(15);
    expect((await api.settingsSet({ auto_lock_minutes: 5 })).auto_lock_minutes).toBe(5);
  });
});
