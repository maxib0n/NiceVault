import { describe, expect, it, vi } from 'vitest';
import { DELETE_SHOW_AFTER_MS, jobs } from './stores/jobs.svelte';

describe('jobs store', () => {
  it('buffers events that arrive before track() and computes progress', () => {
    jobs.progressEvent({ job: 'early', done_bytes: 50, total_bytes: 200, current: 'a.jpg' });
    jobs.track('early', 'import', 'v1', 'a.jpg');
    expect(jobs.items[0]).toMatchObject({ id: 'early', done_bytes: 50, total_bytes: 200, status: 'running' });
    expect(jobs.progress).toBe(0.25);

    const finished: string[] = [];
    const off = jobs.onFinished((j) => finished.push(`${j.id}:${j.status}`));
    jobs.doneEvent({ job: 'early', error: null });
    expect(jobs.items[0].status).toBe('done');
    expect(jobs.progress).toBeNull();

    jobs.track('bad', 'export', 'v1', 'Foto');
    jobs.doneEvent({ job: 'bad', error: { code: 'io', message: 'Spazio esaurito sulla chiavetta.' } });
    expect(jobs.items[0]).toMatchObject({ status: 'error', error: 'Errore di lettura o scrittura sul disco.' });
    expect(finished).toEqual(['early:done', 'bad:error']);
    off();
    jobs.clearFinished();
    expect(jobs.items).toHaveLength(0);
  });

  it('delete jobs: in the tray only once they take a moment, by items, gone when done, kept with what stayed', () => {
    vi.useFakeTimers();
    try {
      jobs.track('d1', 'delete', 'v1', '616 elementi');
      expect(jobs.shown).toHaveLength(0);
      jobs.progressEvent({ job: 'd1', done_bytes: 1000, total_bytes: 4000, current: 'a.jpg', done_items: 154, total_items: 616 });
      vi.advanceTimersByTime(DELETE_SHOW_AFTER_MS);
      expect(jobs.shown.map((j) => j.id)).toEqual(['d1']);
      expect(jobs.progress).toBe(0.25);
      jobs.doneEvent({ job: 'd1', error: null, outcome: { deleted: 616, total: 616, freed_bytes: 4000, errors: [] } });
      expect(jobs.items.find((j) => j.id === 'd1')).toBeUndefined();

      // a quick one that left something behind shows up with it
      jobs.track('d2', 'delete', 'v1', 'Foto');
      jobs.doneEvent({
        job: 'd2',
        error: null,
        outcome: { deleted: 3, total: 5, freed_bytes: 10, errors: [{ node: 9, name: 'Sub', code: 'not_empty', message: 'The folder is not empty.' }] },
      });
      expect(jobs.shown[0]).toMatchObject({ id: 'd2', status: 'error', done_items: 3 });
      expect(jobs.shown[0].error).toContain('Sub');
      jobs.clearFinished();
    } finally {
      vi.useRealTimers();
    }
  });
});
