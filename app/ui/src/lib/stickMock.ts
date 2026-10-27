import type { OsId } from './device';
import type { Stick, StickOs, StickOsResult, StickOutcome, StickSpace } from './stick';
import type { ApiError, EventMap, EventName, JobId } from './types';

export interface StickMockHost {
  wait(ms: number): Promise<void>;
  emit<E extends EventName>(event: E, payload: EventMap[E]): void;
  speed(): number;
}

export type StickScenario = 'old' | 'missing' | 'current' | 'unmounted' | 'tight' | null;

export interface StickMockOptions {
  scenario: StickScenario;
  offline: boolean;
  fail: string | null;
  dataMount: string;
  running: string;
  latest: string;
}

const err = (code: string, message: string): ApiError => ({ code, message });
const MIB = 1048576;
const OSES: OsId[] = ['linux', 'windows', 'macos'];
const FILES: Record<OsId, string> = { linux: 'NiceVault-linux-x86_64', windows: 'NiceVault-windows-x86_64.exe', macos: 'NiceVault.app' };
const SIZES: Record<OsId, number> = { linux: 21 * MIB, windows: 17 * MIB, macos: 34 * MIB };
const TIGHT: Record<OsId, number> = { linux: 28_300_000, windows: 24_500_000, macos: 42_000_000 };

function older(a: string | null, b: string): boolean {
  if (!a) return true;
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) < (pb[i] ?? 0);
  return false;
}

export class StickMock {
  private versions: Record<OsId, string | null>;
  private mounted: boolean;
  private seq = 0;
  private busy = false;
  private cancelled = new Set<JobId>();

  constructor(
    private host: StickMockHost,
    private opts: StickMockOptions,
  ) {
    const v = opts.scenario === 'current' ? opts.running : '0.0.5';
    this.versions = { linux: v, windows: v, macos: opts.scenario === 'missing' ? null : v };
    this.mounted = opts.scenario !== 'unmounted';
  }

  private get exists() {
    return this.opts.scenario !== null;
  }

  private stick(): Stick {
    const running = this.opts.running;
    const programs: StickOs[] = OSES.map((os) => {
      const present = this.mounted && this.versions[os] !== null;
      const version = present ? this.versions[os] : null;
      return {
        os,
        file: FILES[os],
        present,
        version,
        target: os === 'linux' ? running : null,
        source: os === 'linux' ? 'running' : 'download',
        current: present && !older(version, running),
      };
    });
    return {
      disk_id: '/dev/sdd',
      name: 'SanDisk Ultra',
      client_mount: this.mounted ? '/run/media/utente/NiceVault' : null,
      mounted: this.mounted,
      complete: this.mounted,
      data_fs: this.mounted ? 'exfat' : null,
      running_version: running,
      programs,
      outdated: this.mounted && programs.some((p) => !p.current),
      runs_from_stick: false,
      space: this.mounted ? this.space(programs.filter((p) => !p.current).map((p) => p.os)) : null,
    };
  }

  private space(oses: OsId[]): StickSpace {
    const tight = this.opts.scenario === 'tight';
    const sizes = tight ? TIGHT : SIZES;
    const order = [...oses].sort((a, b) => sizes[a] - sizes[b]);
    const size = tight ? 128 * MIB : 256 * MIB;
    const zip = tight && this.zip ? 26_000_000 : 0;
    const free0 = size - zip - OSES.reduce((n, os) => n + (this.versions[os] ? sizes[os] : 0), 0);
    let free = free0 + zip;
    const programs = order.map((os) => {
      const now = this.versions[os] ? sizes[os] : 0;
      const next = sizes[os] + (tight ? 200_000 : 0);
      const mode = free >= next + MIB ? ('safe' as const) : free + now >= next + MIB ? ('tight' as const) : null;
      if (mode) free = free + now - next;
      return { os, now, next, mode };
    });
    return {
      size,
      free: free0,
      reclaim: zip,
      programs,
      tight: programs.some((p) => p.mode === 'tight'),
      fits: programs.every((p) => p.mode !== null),
    };
  }

  private zip = true;

  private holds(path: unknown): boolean {
    if (typeof path !== 'string' || !path) return true;
    const root = this.opts.dataMount.replace(/\/+$/, '');
    return path === root || path.startsWith(`${root}/`);
  }

  private update(): JobId {
    const job = `stick${++this.seq}`;
    const ms = (n: number) => n * this.host.speed();
    const progress = (step: string, os: OsId | null, done: number, total: number) =>
      this.host.emit('job-progress', { job, step, os, done_bytes: done, total_bytes: total, current: step } as unknown as EventMap['job-progress']);
    this.busy = true;
    void (async () => {
      const done = (error: ApiError | null, outcome: StickOutcome | null) => {
        this.busy = false;
        this.host.emit('job-done', { job, error, outcome } as unknown as EventMap['job-done']);
      };
      progress('check', null, 0, 0);
      await this.host.wait(ms(200));
      progress('download', null, 0, 0);
      const plan = OSES.map((os) => {
        const from = this.versions[os];
        if (os === 'linux') return { os, from, to: this.opts.running, change: from === this.opts.running ? 'unchanged' : 'updated' } as const;
        if (this.opts.offline) return { os, from, to: from, change: 'offline' } as const;
        return { os, from, to: this.opts.latest, change: older(from, this.opts.latest) ? 'updated' : 'unchanged' } as const;
      });
      const downloads = plan.filter((p) => p.os !== 'linux' && p.change === 'updated');
      for (const d of downloads) {
        for (let i = 1; i <= 4; i++) {
          if (this.cancelled.has(job)) return;
          await this.host.wait(ms(110));
          progress('download', d.os, (SIZES[d.os] * i) / 4, SIZES[d.os]);
        }
      }
      const space = this.space(plan.filter((p) => p.change === 'updated').map((p) => p.os));
      this.zip = false;
      this.host.emit('job-progress', { job, step: 'space', os: null, done_bytes: 0, total_bytes: 0, current: 'space', space } as unknown as EventMap['job-progress']);
      const writes = plan.filter((p) => p.change === 'updated');
      const total = writes.reduce((s, p) => s + SIZES[p.os], 0) || 1;
      let written = 0;
      const results: StickOsResult[] = [];
      const rank = (os: OsId) => {
        const i = space.programs.findIndex((s) => s.os === os);
        return i < 0 ? 9 : i;
      };
      for (const p of [...plan].sort((a, b) => rank(a.os) - rank(b.os))) {
        if (p.change !== 'updated') {
          results.push({ os: p.os, from: p.from, to: p.to, result: p.change, error: p.change === 'offline' ? err('update_network', 'Cannot reach the update server.') : null });
          continue;
        }
        if (this.cancelled.has(job)) return;
        progress('copy', p.os, written, total);
        await this.host.wait(ms(380));
        const mode = space.programs.find((s) => s.os === p.os)?.mode ?? 'safe';
        if (this.opts.fail === 'rewrite' && mode === 'tight') {
          this.opts.fail = null;
          this.versions[p.os] = null;
          results.push({ os: p.os, from: p.from, to: null, result: 'rewrite', error: err('copy_failed', `${FILES[p.os]}: mock failure while writing`), mode });
          continue;
        }
        if (this.opts.fail && this.opts.fail !== 'rewrite') {
          done(err(this.opts.fail, `mock failure while copying ${FILES[p.os]}`), null);
          return;
        }
        written += SIZES[p.os];
        this.versions[p.os] = p.to;
        progress('verify', p.os, written, total);
        await this.host.wait(ms(120));
        results.push({ os: p.os, from: p.from, to: p.to, result: 'updated', error: null, mode });
      }
      results.sort((a, b) => OSES.indexOf(a.os) - OSES.indexOf(b.os));
      progress('marker', null, total, total);
      await this.host.wait(ms(150));
      done(null, { results, stick: this.stick(), space });
    })();
    return job;
  }

  async invoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
    switch (cmd) {
      case 'device_sticks':
        await this.host.wait(60);
        return this.exists && this.holds(a.path) ? [this.stick()] : [];
      case 'device_stick_mount':
        if (!this.exists || a.id !== '/dev/sdd') throw err('device_not_found', 'The drive is no longer connected.');
        await this.host.wait(250);
        this.mounted = true;
        return this.stick();
      case 'device_stick_update':
        if (!this.exists || a.id !== '/dev/sdd') throw err('device_not_found', 'The drive is no longer connected.');
        if (!this.mounted) throw err('stick_not_mounted', 'The NiceVault partition is not mounted.');
        if (this.busy) throw err('busy', 'The programs on this drive are already being updated.');
        return this.update();
      default:
        throw err('unknown_command', `Unknown command: ${cmd}`);
    }
  }

  cancel(job: JobId): boolean {
    if (!job.startsWith('stick')) return false;
    this.cancelled.add(job);
    this.busy = false;
    return true;
  }
}
