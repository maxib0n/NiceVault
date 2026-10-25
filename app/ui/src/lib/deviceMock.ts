// In-browser mock of the device_* commands ("Crea dispositivo"), plugged into MockBackend.invoke.
// Same names, argument shapes, errors and events as app/src-tauri/src/device/cmd.rs (whose fake
// backend, NICEVAULT_FAKE_DISKS, the Rust e2e uses). Nothing is erased anywhere.
//
// URL switches (switches.ts): device=empty (no drive) · device=incomplete (the SD card is an
// interrupted device: "Completa") · device_fail=<code>[@step] (the job fails, e.g.
// elevation_denied, device_gone@copy) · device_offline (the builds cannot be downloaded; the first
// "Scarica ora" fails too, the second finds the network back) ·
// device_slow=candidates|options|plan|create (that command never answers: the busy states) or
// =auth (the job waits at "unmount" as if the system's password window were open) ·
// device_volumes (the SanDisk holds NiceVault volumes: the erase warning).

import type {
  DataFsId,
  DeviceOptions,
  DeviceOutcome,
  DevicePlan,
  DeviceStep,
  DiskCandidate,
  FsOption,
  OsId,
  PlanOpts,
} from './device';
import type { ApiError, EventMap, EventName, JobId } from './types';
import { switches } from './switches';

export interface DeviceMockHost {
  wait(ms: number): Promise<void>;
  emit<E extends EventName>(event: E, payload: EventMap[E]): void;
  speed(): number;
}

export interface DeviceMockOptions {
  scenario: 'empty' | 'incomplete' | null;
  /** error code, optionally `code@step` */
  fail: string | null;
  offline: boolean;
  slow?: 'candidates' | 'options' | 'plan' | 'create' | 'auth' | null;
  volumes?: boolean;
}

/** The SanDisk's volumes with ?device_volumes (as the backend's volumes_on finds them). */
export const MOCK_VOLUMES = [
  { name: 'Archivio.nvault', path: '/run/media/utente/CHIAVETTA/Lavoro/Archivio.nvault', open: false },
  { name: 'SH2RE.nvault', path: '/run/media/utente/CHIAVETTA/SH2RE.nvault', open: false },
];

/** the mock computer's user name (README screenshots: ?demo=1) */
const USER = switches.demo ? 'alex' : 'utente';

const err = (code: string, message: string): ApiError => ({ code, message });
const MIB = 1048576;
const STEPS: DeviceStep[] = ['check', 'download', 'unmount', 'partition', 'format_client', 'format_data', 'mount', 'copy', 'verify', 'marker', 'volume'];

export const MOCK_DISKS: DiskCandidate[] = [
  {
    id: '/dev/sdd',
    os_path: '/dev/sdd',
    vendor: 'SanDisk',
    model: 'Ultra',
    serial: '4C530001071103235',
    bus: 'usb',
    size: 30_752_636_928,
    removable: true,
    partitions: [{ path: '/dev/sdd1', label: switches.demo ? 'SANDISK' : 'CHIAVETTA', fs: 'vfat', size: 30_751_588_352, mountpoints: [switches.demo ? '/run/media/alex/SANDISK' : '/run/media/utente/CHIAVETTA'] }],
    nicevault: 'none',
    name: 'SanDisk Ultra',
    serial_tail: '1103235',
  },
  {
    id: '/dev/mmcblk0',
    os_path: '/dev/mmcblk0',
    vendor: '',
    model: '',
    serial: '0x1b5e9a21',
    bus: 'sd',
    size: 15_931_539_456,
    removable: true,
    partitions: [{ path: '/dev/mmcblk0p1', label: switches.demo ? 'CAMERA' : 'FOTO', fs: 'exfat', size: 15_930_490_880, mountpoints: [] }],
    nicevault: 'none',
    name: '',
    serial_tail: 'b5e9a21',
  },
];

const READ: Record<DataFsId, [OsId, 'rw' | 'ro' | 'no'][]> = {
  exfat: [['windows', 'rw'], ['macos', 'rw'], ['linux', 'rw']],
  ntfs: [['windows', 'rw'], ['macos', 'ro'], ['linux', 'rw']],
  ext4: [['windows', 'no'], ['macos', 'no'], ['linux', 'rw']],
  btrfs: [['windows', 'no'], ['macos', 'no'], ['linux', 'rw']],
  apfs: [['windows', 'no'], ['macos', 'rw'], ['linux', 'ro']],
};

export class DeviceMock {
  disks: DiskCandidate[];
  private plans = new Map<string, DevicePlan>();
  private outcomes = new Map<JobId, DeviceOutcome>();
  private logs = new Map<JobId, string>();
  private cancelled = new Set<JobId>();
  private seq = 0;
  private fetchTries = 0;

  constructor(
    private host: DeviceMockHost,
    private opts: DeviceMockOptions = { scenario: null, fail: null, offline: false },
  ) {
    this.disks = opts.scenario === 'empty' ? [] : MOCK_DISKS.map((d) => structuredClone(d));
    if (opts.scenario === 'incomplete') this.makeLayout(this.disks[1], 'exfat', false);
  }

  private makeLayout(d: DiskCandidate, fs: DataFsId, marker: boolean) {
    d.partitions = [
      { path: `${d.id}p1`, label: 'NiceVault', fs: 'exfat', size: 128 * MIB, mountpoints: [`/run/media/${USER}/NiceVault`] },
      { path: `${d.id}p2`, label: 'VaultData', fs, size: d.size - 130 * MIB, mountpoints: [`/run/media/${USER}/VaultData`] },
    ];
    d.nicevault = marker ? 'complete' : 'incomplete';
  }

  private disk(id: unknown): DiskCandidate {
    const d = this.disks.find((x) => x.id === id);
    if (!d) throw err('device_not_found', 'The drive is no longer connected.');
    return d;
  }

  /** Tests / devtools: plug in or pull out a drive. */
  plug(d: DiskCandidate) {
    this.disks = [...this.disks.filter((x) => x.id !== d.id), d];
  }
  pull(id: string) {
    this.disks = this.disks.filter((x) => x.id !== id);
  }

  private options(id: string): DeviceOptions {
    const d = this.disk(id);
    const fs: FsOption[] = (['exfat', 'ntfs', 'ext4', 'btrfs', 'apfs'] as DataFsId[]).map((f) => ({
      id: f,
      creatable: f !== 'apfs' && f !== 'btrfs',
      missing_tool: f === 'btrfs' ? 'mkfs.btrfs' : null,
      package: f === 'btrfs' ? 'btrfs-progs' : null,
      readable_on: READ[f],
    }));
    return {
      os: 'linux',
      fs,
      sources: [
        { os: 'linux', local: true },
        { os: 'windows', local: false },
        { os: 'macos', local: false },
      ],
      gpt_required: d.size > 2 * 2 ** 40,
      default_table: 'mbr',
      data_size: d.size - 130 * MIB,
      update_host: 'github.com',
    };
  }

  private plan(o: PlanOpts): DevicePlan {
    const d = this.disk(o.id);
    if (o.data_fs === 'apfs') throw err('fs_unsupported', 'apfs:');
    if (o.data_fs === 'btrfs') throw err('fs_unsupported', 'btrfs:mkfs.btrfs');
    const table = o.table;
    const dev = d.id.replace('/dev/', '');
    const obj = `/org/freedesktop/UDisks2/block_devices/${dev}`;
    const ptype = (fs: string) => (table === 'mbr' ? (fs === 'ext4' ? "'0x83'" : "'0x07'") : fs === 'ext4' ? "'0fc63daf-8483-4772-8e79-3d69d8477de4'" : "'ebd0a0a2-b9e5-4433-87c0-68b6b72699c7'");
    const ia = "'auth.no_user_interaction': <false>";
    const call = (o2: string, mth: string, args: string) => `gdbus call --system --dest org.freedesktop.UDisks2 --object-path ${o2} --method org.freedesktop.UDisks2.${mth} ${args}`;
    const preview = [
      ...d.partitions.filter((p) => p.mountpoints.length).map((p) => call(`/org/freedesktop/UDisks2/block_devices/${p.path.replace('/dev/', '')}`, 'Filesystem.Unmount', `"{${ia}, 'force': <false>}"`)),
      call(obj, 'Block.Format', `'${table === 'mbr' ? 'dos' : 'gpt'}' "{${ia}}"`),
      call(obj, 'PartitionTable.CreatePartitionAndFormat', `'uint64 1048576' 'uint64 536870912' "${ptype('exfat')}" "''" "{…}" "'exfat'" "{'label': <'NiceVault'>, 'take-ownership': <true>, ${ia}}"`),
      call(obj, 'PartitionTable.CreatePartitionAndFormat', `'uint64 537919488' 'uint64 0' "${ptype(o.data_fs)}" "''" "{…}" "'${o.data_fs}'" "{'label': <'VaultData'>, 'take-ownership': <true>, ${ia}}"`),
      `${call('$partition', 'Filesystem.Mount', `"{${ia}}"`)} ($client, $data)`,
    ];
    const steps = STEPS.filter((s) => (s !== 'download' || o.download) && (s !== 'volume' || o.first_volume));
    // the backend's plan carries a bare Disk: no name, no serial_tail (device.ts normalizePlan)
    const { name: _n, serial_tail: _s, ...bare } = structuredClone(d);
    const p = {
      id: `plan${++this.seq}`,
      os: 'linux',
      disk: bare as DiskCandidate,
      table,
      table_forced: false,
      data_fs: o.data_fs,
      client: { label: 'NiceVault', fs: 'exfat', offset: MIB, size: 128 * MIB },
      data: { label: 'VaultData', fs: o.data_fs, offset: 129 * MIB, size: 0 },
      data_size: d.size - 130 * MIB,
      first_volume: o.first_volume,
      download: o.download,
      steps,
      commands_preview: preview,
      confirm_name: d.name,
      volumes: this.opts.volumes && d.id === '/dev/sdd' ? structuredClone(MOCK_VOLUMES) : [],
    } satisfies DevicePlan as DevicePlan;
    this.plans.set(p.id, p);
    return p;
  }

  private failAt(): [string, DeviceStep] | null {
    if (!this.opts.fail) return null;
    const [code, step] = this.opts.fail.split('@');
    return [code, (step as DeviceStep) || 'partition'];
  }

  private run(p: DevicePlan, steps: DeviceStep[], resume: boolean): JobId {
    const job = `dev${++this.seq}`;
    const log: string[] = [JSON.stringify({ kind: 'plan', data: p })];
    this.logs.set(job, '');
    const fail = this.failAt();
    const ms = (n: number) => n * this.host.speed();
    void (async () => {
      const d = this.disks.find((x) => x.id === p.disk.id);
      for (const s of steps.filter((x) => x !== 'volume')) {
        if (this.cancelled.has(job)) return; // job_cancel already emitted job-done "cancelled"
        const bytes = s === 'download' || s === 'copy';
        const total = s === 'download' ? (this.opts.offline ? 0 : 58 * MIB) : s === 'copy' ? 62 * MIB : 0;
        this.host.emit('job-progress', { job, step: s, done_bytes: 0, total_bytes: total, current: s } as EventMap['job-progress']);
        log.push(JSON.stringify({ kind: 'step', step: s }));
        if (this.opts.slow === 'auth' && s === 'unmount') await this.host.wait(120_000);
        if (fail && fail[1] === s) {
          await this.host.wait(ms(300));
          if (fail[0] === 'device_gone' && d) this.makeLayout(d, p.data_fs, false);
          log.push(JSON.stringify({ kind: 'error', code: fail[0] }));
          this.logs.set(job, log.join('\n'));
          this.host.emit('job-done', { job, error: err(fail[0], `mock failure at ${s}`) });
          return;
        }
        if (bytes && total > 0) {
          for (let i = 1; i <= 8; i++) {
            await this.host.wait(ms(120));
            this.host.emit('job-progress', { job, step: s, done_bytes: Math.round((total * i) / 8), total_bytes: total, current: s } as EventMap['job-progress']);
          }
        } else await this.host.wait(ms(s === 'format_data' ? 700 : 350));
      }
      if (d) this.makeLayout(d, p.data_fs, true);
      const missing: OsId[] = this.opts.offline ? ['windows', 'macos'] : [];
      const outcome: DeviceOutcome = {
        disk_id: p.disk.id,
        client_mount: `/run/media/${USER}/NiceVault`,
        data_mount: `/run/media/${USER}/VaultData`,
        data_fs: p.data_fs,
        data_free: p.data_size - 2 * MIB,
        whole_volume_size: Math.floor((p.data_size - 160 * MIB) / MIB) * MIB,
        missing,
        first_volume: p.first_volume,
        log: `/home/${USER}/.cache/NiceVault/device-${job}.log`,
      };
      log.push(JSON.stringify({ kind: 'done', resume }));
      this.logs.set(job, log.join('\n'));
      this.outcomes.set(job, outcome);
      this.host.emit('job-done', { job, error: null, outcome } as EventMap['job-done']);
    })();
    return job;
  }

  async invoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
    const slow = { device_candidates: 'candidates', device_options: 'options', device_plan: 'plan', device_create: 'create' }[cmd];
    // a call that never answers (the busy state stays on screen)
    if (slow && this.opts.slow === slow) await new Promise(() => {});
    switch (cmd) {
      case 'device_candidates':
        await this.host.wait(250);
        return this.disks.map((d) => structuredClone(d));
      case 'device_options':
        await this.host.wait(120);
        return this.options(String(a.id));
      case 'device_plan':
        await this.host.wait(120);
        return this.plan(a.opts as PlanOpts);
      case 'device_create': {
        const p = this.plans.get(String(a.plan_id));
        if (!p) throw err('not_found', 'Plan not found.');
        const typed = String(a.typed ?? '');
        const norm = (s: string) => s.trim().split(/\s+/).filter(Boolean).join(' ').toLowerCase();
        const ok = p.confirm_name.trim() ? norm(typed) === norm(p.confirm_name) : ['cancella', 'erase'].includes(norm(typed));
        if (!ok) throw err('confirmation_mismatch', 'The name typed does not match the drive.');
        this.disk(p.disk.id);
        this.plans.delete(p.id);
        return this.run(p, p.steps, false);
      }
      case 'device_resume': {
        const d = this.disk(a.id);
        if (d.nicevault === 'none') throw err('device_changed', 'This drive does not have the NiceVault layout.');
        const p = this.plan({ id: d.id, table: 'mbr', data_fs: (d.partitions[1]?.fs as DataFsId) ?? 'exfat', first_volume: null, download: d.nicevault === 'incomplete' });
        const steps: DeviceStep[] = d.nicevault === 'complete' ? ['check', 'mount'] : ['check', 'download', 'mount', 'copy', 'verify', 'marker'];
        return this.run({ ...p, steps }, steps, true);
      }
      case 'device_eject':
        await this.host.wait(300);
        this.pull(String(a.id));
        return null;
      case 'device_log':
        return this.logs.get(String(a.job)) ?? '';
      case 'device_outcome': {
        const o = this.outcomes.get(String(a.job));
        if (!o) throw err('not_found', 'Device not found.');
        return o;
      }
      case 'device_open':
        return null;
      case 'device_fetch_missing': {
        const o = this.outcomes.get(String(a.job));
        if (!o) throw err('not_found', 'Device not found.');
        if (!o.missing.length) return o;
        await this.host.wait(600);
        if (this.opts.offline && this.fetchTries++ === 0) throw err('device_download', 'The programs could not be downloaded (offline).');
        const done = { ...o, missing: [] };
        this.outcomes.set(String(a.job), done);
        return done;
      }
      default:
        throw err('unknown_command', `Unknown command: ${cmd}`);
    }
  }

  cancel(job: JobId): boolean {
    if (!this.logs.has(job)) return false;
    this.cancelled.add(job);
    return true;
  }
}
