import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import { MOCK_UPDATE, MockBackend } from './mock';
import { update } from './stores/update.svelte';
import type { UpdateProgress } from './types';

describe('updater (mock backend)', () => {
  beforeEach(() => {
    api.useMock(new MockBackend({ speed: 0 }));
    update.reset();
  });

  it('the start-up check is silent and skipped unless asked for', async () => {
    expect(await api.updateCheck(true)).toEqual({ status: 'skipped' });
    await update.autoCheck();
    expect(update.pending).toBe(false);
    expect(update.prefs?.check_on_start).toBe(true);
  });

  it('a manual check finds the fake newer version', async () => {
    const s = await api.updateCheck();
    expect(s).toMatchObject({ status: 'available', current: MOCK_UPDATE.current, version: MOCK_UPDATE.version });
    if (s.status === 'available') expect(s.notes.it).toBeTruthy();
  });

  it('download reports progress, install replaces, restart is allowed after', async () => {
    await expect(api.updateDownload()).rejects.toMatchObject({ code: 'update_none' });
    await api.updateCheck();
    const seen: UpdateProgress[] = [];
    const off = await api.onUpdateProgress((p) => seen.push(p));
    await api.updateDownload();
    off();
    expect(seen.length).toBeGreaterThan(1);
    expect(seen.at(-1)).toEqual({ downloaded: MOCK_UPDATE.size, total: MOCK_UPDATE.size });
    expect(await api.updateInstall()).toMatchObject({ mode: 'replaced' });
    await expect(api.updateRestart()).resolves.toBeNull();
  });

  it('the store walks available -> ready', async () => {
    api.useMock(new MockBackend({ speed: 0, updateOnStart: true }));
    await update.autoCheck();
    expect(update.phase).toBe('available');
    expect(update.pending).toBe(true);
    await update.install();
    expect(update.phase).toBe('ready');
    expect(update.progress).toBe(1);
  });

  it('read-only location: saved to Downloads, no restart', async () => {
    api.useMock(new MockBackend({ speed: 0, updateMode: 'downloaded' }));
    await update.check();
    await update.install();
    expect(update.phase).toBe('saved');
    expect(update.outcome?.path).toMatch(/Scaricati/);
    await expect(api.updateRestart()).rejects.toMatchObject({ code: 'update_none' });
  });

  it('the start-up check can be turned off', async () => {
    api.useMock(new MockBackend({ speed: 0, updateOnStart: true }));
    await update.autoCheck();
    await update.setCheckOnStart(false);
    expect(update.prefs?.check_on_start).toBe(false);
    expect(await api.updateCheck(true)).toEqual({ status: 'skipped' });
  });
});
