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
  results: StickOsResult[];
  error: string | null;
  before: Stick;
  space?: StickSpace | null;
}

const ALL = '*';

class Sticks {
  found = $state<Record<string, Stick[]>>({});
  runs = $state<Record<string, StickRun>>({});
  mounting = $state<Record<string, boolean>>({});
  mountError = $state<Record<string, string | null>>({});
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

  clear(id: string) {
    if (this.runs[id] && this.runs[id].phase !== 'running') delete this.runs[id];
  }

  dismiss(id: string) {
    this.dismissed[id] = true;
  }
}

export const sticks = new Sticks();
