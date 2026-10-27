import * as api from '../api';
import type { DeleteOutcome, JobDone, JobId, JobProgress, VaultId } from '../types';
import { errorText } from '../errors';
import { play } from '../sound';
import { isMobile } from '../platform';
import { feedRate, startRate, type Rate } from '../jobSpeed';
import { m } from '../../paraglide/messages.js';
import { toasts } from './toasts.svelte';

export type JobKind = 'import' | 'export' | 'delete';

export const DELETE_SHOW_AFTER_MS = 400;
export type JobStatus = 'running' | 'done' | 'error' | 'cancelled';

export interface Job {
  id: JobId;
  kind: JobKind;
  vault: VaultId;
  label: string;
  target?: string;
  done_bytes: number;
  total_bytes: number;
  current: string;
  done_items: number;
  total_items: number;
  outcome: DeleteOutcome | null;
  hidden: boolean;
  status: JobStatus;
  error: string | null;
  startedAt: number;
  rate: Rate;
  finishedAt: number | null;
}

type FinishedHandler = (job: Job) => void;

function leftNames(o: DeleteOutcome): string {
  const names = o.errors.slice(0, 3).map((e) => e.name);
  const more = o.errors.length > 3 ? '…' : '';
  const why = o.errors.length === 1 ? ` (${errorText({ code: o.errors[0].code, message: o.errors[0].message })})` : '';
  return names.join(', ') + more + why;
}

class Jobs {
  items = $state<Job[]>([]);
  open = $state(false);

  shown = $derived(this.items.filter((j) => !j.hidden));
  running = $derived(this.shown.filter((j) => j.status === 'running'));
  progress = $derived.by(() => {
    const r = this.running;
    if (r.length === 0) return null;
    const moving = r.filter((j) => j.kind !== 'delete');
    if (moving.length === 0) {
      const total = r.reduce((s, j) => s + j.total_items, 0);
      return total > 0 ? r.reduce((s, j) => s + j.done_items, 0) / total : 0;
    }
    const total = moving.reduce((s, j) => s + j.total_bytes, 0);
    const done = moving.reduce((s, j) => s + j.done_bytes, 0);
    return total > 0 ? done / total : 0;
  });

  private pendingProgress = new Map<JobId, JobProgress>();
  private pendingDone = new Map<JobId, JobDone>();
  private finishedHandlers = new Set<FinishedHandler>();
  private unlisten: (() => void)[] = [];

  async listen() {
    if (this.unlisten.length) return;
    this.unlisten.push(await api.onJobProgress((p) => this.progressEvent(p)));
    this.unlisten.push(await api.onJobDone((d) => this.doneEvent(d)));
  }

  onFinished(h: FinishedHandler): () => void {
    this.finishedHandlers.add(h);
    return () => this.finishedHandlers.delete(h);
  }

  track(id: JobId, kind: JobKind, vault: VaultId, label: string, target?: string): void {
    this.items.unshift({
      id,
      kind,
      vault,
      label,
      target,
      done_bytes: 0,
      total_bytes: 0,
      current: '',
      done_items: 0,
      total_items: 0,
      outcome: null,
      hidden: kind === 'delete',
      status: 'running',
      error: null,
      startedAt: Date.now(),
      rate: startRate(Date.now()),
      finishedAt: null,
    });
    const p = this.pendingProgress.get(id);
    if (p) this.progressEvent(p);
    const d = this.pendingDone.get(id);
    if (d) this.doneEvent(d);
    this.pendingProgress.delete(id);
    this.pendingDone.delete(id);
    if (kind === 'delete')
      setTimeout(() => {
        const j = this.find(id);
        if (j?.status === 'running') j.hidden = false;
      }, DELETE_SHOW_AFTER_MS);
  }

  private find(id: JobId): Job | undefined {
    return this.items.find((j) => j.id === id);
  }

  progressEvent(p: JobProgress): void {
    const j = this.find(p.job);
    if (!j) {
      this.pendingProgress.set(p.job, p);
      return;
    }
    if (j.status !== 'running') return;
    j.done_bytes = p.done_bytes;
    j.total_bytes = p.total_bytes;
    j.current = p.current;
    if (p.done_items !== undefined) j.done_items = p.done_items;
    if (p.total_items !== undefined) j.total_items = p.total_items;
    feedRate(j.rate, Date.now(), j.kind === 'delete' ? j.done_items : j.done_bytes);
  }

  doneEvent(d: JobDone): void {
    const j = this.find(d.job);
    if (!j) {
      this.pendingDone.set(d.job, d);
      return;
    }
    if (j.status !== 'running') return;
    const code = typeof d.error === 'object' && d.error ? d.error.code : null;
    const outcome = j.kind === 'delete' ? ((d.outcome as DeleteOutcome | undefined) ?? null) : null;
    j.outcome = outcome;
    const left = outcome?.errors.length ?? 0;
    j.status = d.error ? (code === 'cancelled' ? 'cancelled' : 'error') : left > 0 ? 'error' : 'done';
    j.error = d.error ? errorText(d.error) : left > 0 ? m.job_delete_left({ n: String(left), names: leftNames(outcome!) }) : null;
    if (outcome) {
      j.done_items = outcome.deleted;
      j.done_bytes = outcome.freed_bytes;
    }
    if (!d.error && !left) j.done_bytes = j.total_bytes;
    j.finishedAt = Date.now();
    if (j.status === 'error') j.hidden = false;
    if (j.status === 'done') play(j.kind === 'delete' ? 'delete' : 'done');
    if (j.kind === 'delete' && j.status !== 'error') this.items = this.items.filter((x) => x.id !== j.id);
    const snapshot = { ...j };
    for (const h of this.finishedHandlers) h(snapshot);
    if (isMobile) this.settlePhone();
  }

  private settlePhone() {
    if (this.items.some((j) => j.status === 'running')) return;
    const failed = this.items.filter((j) => j.status === 'error');
    const shown = this.shown.length > 0;
    this.items = failed;
    this.open = false;
    if (failed.length === 0 && shown) toasts.push(m.jobs_done(), 'success');
  }

  async cancel(id: JobId) {
    try {
      await api.jobCancel(id);
    } catch {
    }
  }

  dismiss(id: JobId) {
    this.items = this.items.filter((j) => j.id !== id);
  }

  clearFinished() {
    this.items = this.items.filter((j) => j.status === 'running');
  }

  dropVault(vault: VaultId) {
    this.items = this.items.filter((j) => j.vault !== vault || j.status === 'running');
  }
}

export const jobs = new Jobs();
