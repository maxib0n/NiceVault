// "Aggiorna i programmi sulla chiavetta": the NiceVault devices the app sees (for the active
// volume and for Benvenuto), the update running on one of them, and which notices were put away.
// Dettagli > Volume (StickPrograms.svelte) and the notices (StickNotice.svelte) share it, so an
// update started in one shows its progress in the other.
import * as api from '../api';
import type { OsId } from '../device';
import { errorText } from '../errors';
import { play } from '../sound';
import { deviceStickMount, deviceSticks, deviceStickUpdate, type Stick, type StickDone, type StickOsResult, type StickProgress, type StickSpace, type StickStep } from '../stick';
import type { JobId } from '../types';

export interface StickRun {
  job: JobId | null;
  phase: 'running' | 'done' | 'error';
  step: StickStep | null;
  os: OsId | null;
  done_bytes: number;
  total_bytes: number;
  /** per OS, once finished */
  results: StickOsResult[];
  error: string | null;
  /** the stick before the update (versions "from") */
  before: Stick;
  /** the room with the real sizes, once known (tight: one program at a time) */
  space?: StickSpace | null;
}

/** the key of a lookup: a volume's path, or '*' (every connected device) */
const ALL = '*';

class Sticks {
  /** lookup key -> devices found (path: 0 or 1) */
  found = $state<Record<string, Stick[]>>({});
  /** disk id -> the update in progress or just finished */
  runs = $state<Record<string, StickRun>>({});
  /** disk id -> mount in progress / its error */
  mounting = $state<Record<string, boolean>>({});
  mountError = $state<Record<string, string | null>>({});
  /** notices put away this session ("Non ora"), by disk id */
  dismissed = $state<Record<string, boolean>>({});

  private pending = new Map<JobId, StickProgress[]>();
  private pendingDone = new Map<JobId, StickDone>();
  private listening = false;
  private seq: Record<string, number> = {};

  private async listen() {
    if (this.listening) return;
    this.listening = true;
    await api.onJobProgress((p) => this.progressEvent(p as unknown as StickProgress));
    await api.onJobDone((d) => this.doneEvent(d as unknown as StickDone));
    await api.onDrivesChanged(() => this.refresh());
  }

  private runOf(job: JobId): [string, StickRun] | null {
    for (const [id, r] of Object.entries(this.runs)) if (r.job === job) return [id, r];
    return null;
  }

  private progressEvent(p: StickProgress) {
    const hit = this.runOf(p.job);
    if (!hit) {
      // an update whose command has not answered yet: its first events may come first
      if (this.awaiting()) this.pending.set(p.job, [...(this.pending.get(p.job) ?? []), p].slice(-4));
      return;
    }
    const r = hit[1];
    if (r.phase !== 'running') return;
    if (p.step === 'space') {
      r.space = p.space ?? null;
      return;
    }
    r.step = p.step;
    if (p.os !== undefined) r.os = p.os;
    r.done_bytes = p.done_bytes;
    r.total_bytes = p.total_bytes;
  }

  private doneEvent(d: StickDone) {
    const hit = this.runOf(d.job);
    if (!hit) {
      if (this.awaiting()) this.pendingDone.set(d.job, d);
      return;
    }
    const r = hit[1];
    if (r.phase !== 'running') return;
    if (d.error) {
      r.phase = 'error';
      r.error = errorText(d.error);
    } else {
      r.phase = 'done';
      play('device');
      r.results = d.outcome?.results ?? [];
      r.space = d.outcome?.space ?? r.space ?? null;
    }
    r.step = null;
    r.os = null;
    if (d.outcome?.stick) this.replace(d.outcome.stick);
    void this.refresh();
  }

  private awaiting(): boolean {
    return Object.values(this.runs).some((r) => r.job === null && r.phase === 'running');
  }

  private replace(s: Stick) {
    for (const k of Object.keys(this.found)) this.found[k] = this.found[k].map((x) => (x.disk_id === s.disk_id ? s : x));
  }

  /** Looks up the device holding `path` (null/undefined: every connected one). Cheap, read-only. */
  async load(path?: string | null): Promise<Stick[]> {
    void this.listen();
    const key = path || ALL;
    const n = (this.seq[key] = (this.seq[key] ?? 0) + 1);
    try {
      const list = await deviceSticks(path || null);
      if (this.seq[key] === n) this.found[key] = list;
      return list;
    } catch {
      if (this.seq[key] === n) this.found[key] = [];
      return [];
    }
  }

  /** What `load(path)` found (path: the one device or null). */
  forPath(path: string | null | undefined): Stick | null {
    return this.found[path || ALL]?.[0] ?? null;
  }
  all(): Stick[] {
    return this.found[ALL] ?? [];
  }

  async refresh() {
    await Promise.all(Object.keys(this.found).map((k) => this.load(k === ALL ? null : k)));
  }

  async mount(id: string) {
    if (this.mounting[id]) return;
    this.mounting[id] = true;
    this.mountError[id] = null;
    try {
      const s = await deviceStickMount(id);
      this.replace(s);
    } catch (e) {
      this.mountError[id] = errorText(e);
    } finally {
      this.mounting[id] = false;
    }
    await this.refresh();
  }

  async update(s: Stick) {
    const id = s.disk_id;
    if (this.runs[id]?.phase === 'running') return;
    void this.listen();
    this.runs[id] = { job: null, phase: 'running', step: 'check', os: null, done_bytes: 0, total_bytes: 0, results: [], error: null, before: s };
    try {
      const job = await deviceStickUpdate(id);
      const r = this.runs[id];
      r.job = job;
      const early = this.pending.get(job) ?? [];
      const d = this.pendingDone.get(job);
      if (!this.awaiting()) {
        this.pending.clear();
        this.pendingDone.clear();
      }
      for (const p of early) this.progressEvent(p);
      if (d) this.doneEvent(d);
    } catch (e) {
      const r = this.runs[id];
      r.phase = 'error';
      r.error = errorText(e);
      if (!this.awaiting()) {
        this.pending.clear();
        this.pendingDone.clear();
      }
    }
  }

  /** forgets a finished run (its result line) */
  clear(id: string) {
    if (this.runs[id] && this.runs[id].phase !== 'running') delete this.runs[id];
  }

  dismiss(id: string) {
    this.dismissed[id] = true;
  }
}

export const sticks = new Sticks();
