import * as api from '../api';
import { m } from '../../paraglide/messages.js';
import type { JobId, NodeId, PickFailure, PickProgress, VaultId } from '../types';
import { jobs } from './jobs.svelte';
import { toasts } from './toasts.svelte';

export interface PhotoRun {
  job: JobId;
  vault: VaultId;
  parent: NodeId;
  folder: string;
  total: number;
  done: number;
  imported: number;
  failed: number;
  fetching: number;
  fraction: number;
  finished: boolean;
  cancelled: boolean;
  failures: PickFailure[];
}

export const PHOTOS_DONE_MS = 1800;

class Photos {
  run = $state<PhotoRun | null>(null);
  private early = new Map<JobId, PickProgress>();
  private unlisten: (() => void) | null = null;
  private closer: ReturnType<typeof setTimeout> | null = null;

  private async listen() {
    if (!this.unlisten) this.unlisten = await api.onPickProgress((p) => this.event(p));
  }

  async start(vault: VaultId, parent: NodeId, folder: string, retry?: number[]): Promise<void> {
    await this.listen();
    let r;
    try {
      r = await api.mobileImportPhotos(vault, parent, retry);
    } catch (e) {
      toasts.error(e);
      return;
    }
    if (!r) return;
    this.clearCloser();
    this.run = { job: r.job, vault, parent, folder, total: r.total, done: 0, imported: 0, failed: 0, fetching: 0, fraction: 0, finished: false, cancelled: false, failures: [] };
    jobs.track(r.job, 'import', vault, m.pick_label({ n: String(r.total) }), folder);
    const p = this.early.get(r.job);
    this.early.clear();
    if (p) this.event(p);
  }

  event(p: PickProgress): void {
    const run = this.run;
    if (!run || run.job !== p.job) {
      this.early.set(p.job, p);
      return;
    }
    if (run.finished) return;
    Object.assign(run, {
      total: p.total,
      done: p.done,
      imported: p.imported,
      failed: p.failed,
      fetching: p.fetching,
      fraction: p.fraction,
      finished: p.finished,
      cancelled: p.cancelled,
      failures: p.failures ?? run.failures,
    });
    if (p.finished && !p.cancelled && run.failures.length === 0) {
      this.clearCloser();
      this.closer = setTimeout(() => this.close(), PHOTOS_DONE_MS);
    }
  }

  cancel() {
    if (this.run && !this.run.finished) void jobs.cancel(this.run.job);
  }

  async retry() {
    const run = this.run;
    if (!run || !run.finished || run.failures.length === 0) return;
    await this.start(run.vault, run.parent, run.folder, run.failures.map((f) => f.index));
  }

  close() {
    this.clearCloser();
    if (this.run && !this.run.finished) return;
    this.run = null;
  }

  dropVault(vault: VaultId) {
    if (this.run?.vault === vault) {
      this.clearCloser();
      this.run = null;
    }
  }

  private clearCloser() {
    if (this.closer) clearTimeout(this.closer);
    this.closer = null;
  }
}

export const photos = new Photos();
