import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import * as dev from './device';
import type { DeviceDone, DeviceProgress } from './device';
import { MockBackend } from './mock';

async function run(start: () => Promise<string>): Promise<{ steps: string[]; done: DeviceDone }> {
  const progress: DeviceProgress[] = [];
  const dones: DeviceDone[] = [];
  const offP = await api.onJobProgress((p) => progress.push(p as unknown as DeviceProgress));
  const offD = await api.onJobDone((d) => dones.push(d as unknown as DeviceDone));
  const job = await start();
  for (let i = 0; i < 500 && !dones.some((d) => d.job === job); i++) await new Promise((r) => setTimeout(r, 2));
  offP();
  offD();
  const steps: string[] = [];
  for (const p of progress.filter((x) => x.job === job)) if (steps.at(-1) !== p.step) steps.push(p.step);
  return { steps, done: dones.find((d) => d.job === job)! };
}

describe('device helpers', () => {
  it('formats drives like the box says them', () => {
    expect(dev.formatDiskSize(30_752_636_928)).toMatch(/^30[.,]8 GB$/);
    expect(dev.formatDiskSize(2_000_398_934_016)).toMatch(/^2 TB$/);
    expect(dev.busLabel('usb')).toBe('USB');
    const d = { name: 'SanDisk Ultra', size: 30_752_636_928, bus: 'usb', serial_tail: '1103235' } as dev.DiskCandidate;
    expect(dev.diskLine(d, 'x')).toMatch(/^SanDisk Ultra · 30[.,]8 GB · USB · S\/N …1103235$/);
    expect(dev.diskLine({ ...d, name: ' ', serial_tail: '' }, 'Unità senza nome')).toMatch(/^Unità senza nome · 30[.,]8 GB · USB$/);
  });

  it('matches the confirmation like the backend', () => {
    expect(dev.confirmMatches('SanDisk Ultra', '  sandisk   ULTRA ')).toBe(true);
    expect(dev.confirmMatches('SanDisk Ultra', 'SanDisk')).toBe(false);
    expect(dev.confirmMatches('SanDisk Ultra', '')).toBe(false);
    expect(dev.confirmMatches('', 'cancella')).toBe(true);
    expect(dev.confirmMatches('', 'ERASE')).toBe(true);
    expect(dev.confirmMatches('', '')).toBe(false);
  });

  it('step states, cancel window, sizes, paths', () => {
    const steps: dev.DeviceStep[] = ['check', 'download', 'partition', 'copy'];
    expect(dev.stepStates(steps, 'partition', null)).toEqual(['done', 'done', 'current', 'todo']);
    expect(dev.stepStates(steps, 'partition', 'error')).toEqual(['done', 'done', 'failed', 'todo']);
    expect(dev.stepStates(steps, 'copy', 'ok')).toEqual(['done', 'done', 'done', 'done']);
    expect(dev.cancellable('download')).toBe(true);
    expect(dev.cancellable('partition')).toBe(false);
    expect(dev.cancellable('copy')).toBe(false);
    expect(dev.gbToBytes('8')).toBe(Math.floor(8e9 / 1048576) * 1048576);
    expect(dev.gbToBytes('1,5')).toBe(Math.floor(1.5e9 / 1048576) * 1048576);
    expect(dev.gbToBytes('x')).toBeNull();
    expect(dev.gbToBytes('0')).toBeNull();
    expect(dev.volumePath('/run/media/u/VaultData/', 'Dati.nvault')).toBe('/run/media/u/VaultData/Dati.nvault');
    expect(dev.volumePath('E:\\', 'Dati')).toBe('E:\\Dati.nvault');
    expect(dev.volumeSize({ mode: 'whole', size: null, name: 'x' }, { whole_volume_size: 99 })).toBe(99);
    expect(dev.volumeSize({ mode: 'piece', size: 5, name: 'x' }, { whole_volume_size: 99 })).toBe(5);
  });

  it('readability chips', () => {
    const f: dev.FsOption = { id: 'ext4', creatable: true, missing_tool: null, package: null, readable_on: [['windows', 'no'], ['macos', 'no'], ['linux', 'rw']] };
    expect(dev.unreadableOn(f)).toEqual(['windows', 'macos']);
    expect(dev.readOnlyOn({ ...f, readable_on: [['macos', 'ro']] })).toEqual(['macos']);
  });
});

describe('device mock (parity with device/cmd.rs)', () => {
  beforeEach(() => api.useMock(new MockBackend({ speed: 0 })));

  it('lists external drives, plans, refuses a wrong name, creates', async () => {
    const disks = await dev.deviceCandidates();
    expect(disks.map((d) => d.id)).toEqual(['/dev/sdd', '/dev/mmcblk0']);
    const o = await dev.deviceOptions('/dev/sdd');
    expect(o.fs.find((f) => f.id === 'btrfs')).toMatchObject({ creatable: false, missing_tool: 'mkfs.btrfs', package: 'btrfs-progs' });
    await expect(dev.devicePlan({ id: '/dev/sdd', table: 'mbr', data_fs: 'btrfs', first_volume: null, download: true })).rejects.toMatchObject({ code: 'fs_unsupported' });
    const p = await dev.devicePlan({ id: '/dev/sdd', table: 'mbr', data_fs: 'exfat', first_volume: { mode: 'whole', size: null, name: 'Dati' }, download: true });
    expect(p.confirm_name).toBe('SanDisk Ultra');
    expect(p.steps.at(-1)).toBe('volume');
    expect(p.commands_preview.some((c) => c.includes('Block.Format'))).toBe(true);
    await expect(dev.deviceCreate(p.id, 'SanDisk')).rejects.toMatchObject({ code: 'confirmation_mismatch' });
    const { steps, done } = await run(() => dev.deviceCreate(p.id, 'sandisk ultra'));
    expect(done.error).toBeNull();
    expect(steps).toEqual(['check', 'download', 'unmount', 'partition', 'format_client', 'format_data', 'mount', 'copy', 'verify', 'marker']);
    expect(done.outcome?.whole_volume_size).toBeGreaterThan(29e9);
    expect((await dev.deviceCandidates()).find((d) => d.id === '/dev/sdd')?.nicevault).toBe('complete');
    await dev.deviceEject('/dev/sdd');
    expect((await dev.deviceCandidates()).map((d) => d.id)).toEqual(['/dev/mmcblk0']);
  });

  it('an unplug midway leaves an incomplete device; Completa does not reformat', async () => {
    api.useMock(new MockBackend({ speed: 0, deviceFail: 'device_gone@copy' }));
    const p = await dev.devicePlan({ id: '/dev/mmcblk0', table: 'mbr', data_fs: 'exfat', first_volume: null, download: true });
    expect(p.confirm_name).toBe('');
    const { done } = await run(() => dev.deviceCreate(p.id, 'CANCELLA'));
    expect(done.error).toMatchObject({ code: 'device_gone' });
    const d = (await dev.deviceCandidates()).find((x) => x.id === '/dev/mmcblk0')!;
    expect(d.nicevault).toBe('incomplete');
    api.useMock(new MockBackend({ speed: 0, device: 'incomplete' }));
    const r = await run(() => dev.deviceResume('/dev/mmcblk0'));
    expect(r.done.error).toBeNull();
    expect(r.steps).toEqual(['check', 'download', 'mount', 'copy', 'verify', 'marker']);
  });

  it('a refused permission prompt is an error; no drives = empty list', async () => {
    api.useMock(new MockBackend({ speed: 0, deviceFail: 'elevation_denied' }));
    const p = await dev.devicePlan({ id: '/dev/sdd', table: 'gpt', data_fs: 'ntfs', first_volume: null, download: false });
    expect(p.steps).not.toContain('download');
    const { done } = await run(() => dev.deviceCreate(p.id, 'SanDisk Ultra'));
    expect(done.error).toMatchObject({ code: 'elevation_denied' });
    api.useMock(new MockBackend({ speed: 0, device: 'empty' }));
    expect(await dev.deviceCandidates()).toEqual([]);
  });
});

describe('normalizePlan', () => {
  it('gives the backend\'s bare Disk a name and serial tail (the confirm step read name.trim())', () => {
    const disk = { id: '/dev/sdd', os_path: '/dev/sdd', vendor: 'SanDisk', model: 'Ultra', serial: '4C530001071103235', bus: 'usb', size: 1, removable: true, partitions: [], nicevault: 'none' as const };
    const p = dev.normalizePlan({ confirm_name: 'SanDisk Ultra', disk } as unknown as Parameters<typeof dev.normalizePlan>[0]);
    expect(p.disk.name).toBe('SanDisk Ultra');
    expect(p.disk.serial_tail).toBe('1103235');
    expect(p.volumes).toEqual([]);
    expect(dev.diskName({}, 'x')).toBe('x');
  });
});

