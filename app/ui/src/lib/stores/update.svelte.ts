// The app update: start-up check (at most once a day, can be turned off), manual
// "Cerca aggiornamenti", download (with progress), install in place, restart.
import * as api from '../api';
import { errorText } from '../errors';
import type { InstallOutcome, UpdatePrefs, UpdateStatus } from '../types';

export type UpdatePhase =
  | 'idle' // nothing known yet
  | 'checking'
  | 'up_to_date'
  | 'available'
  | 'downloading'
  | 'installing'
  | 'ready' // replaced in place: Riavvia
  | 'saved' // location not writable: saved in Downloads
  | 'error';

type Available = Extract<UpdateStatus, { status: 'available' }>;

class Update {
  phase = $state<UpdatePhase>('idle');
  release = $state<Available | null>(null);
  /** 0..1, null = unknown length */
  progress = $state<number | null>(null);
  outcome = $state<InstallOutcome | null>(null);
  error = $state<string | null>(null);
  prefs = $state<UpdatePrefs | null>(null);

  /** The notice is worth showing (Benvenuto, sidebar). */
  get pending(): boolean {
    return this.release !== null && this.phase !== 'up_to_date' && this.phase !== 'checking';
  }

  private apply(s: UpdateStatus): void {
    if (s.status === 'available') {
      this.release = s;
      this.phase = 'available';
    } else if (s.status === 'up_to_date') {
      this.release = null;
      this.phase = 'up_to_date';
    }
  }

  /** On start: silent, skipped by the backend when off or already done today. */
  async autoCheck(): Promise<void> {
    try {
      this.prefs = await api.updatePrefsGet();
      const s = await api.updateCheck(true);
      if (s.status === 'available') this.apply(s);
    } catch {
      /* offline at start: say nothing */
    }
  }

  /** Impostazioni: the start-up switch needs the preferences even when no check ran. */
  async loadPrefs(): Promise<void> {
    try {
      this.prefs = await api.updatePrefsGet();
    } catch {
      /* the switch stays disabled */
    }
  }

  /** "Cerca aggiornamenti". */
  async check(): Promise<void> {
    if (this.busy) return;
    this.phase = 'checking';
    this.error = null;
    try {
      this.apply(await api.updateCheck(false));
      this.prefs = await api.updatePrefsGet();
    } catch (e) {
      this.fail(e);
    }
  }

  get busy(): boolean {
    return this.phase === 'checking' || this.phase === 'downloading' || this.phase === 'installing';
  }

  /** Download, verify, install. */
  async install(): Promise<void> {
    if (!this.release || this.busy) return;
    this.phase = 'downloading';
    this.progress = 0;
    this.error = null;
    const off = await api.onUpdateProgress((p) => {
      this.progress = p.total ? Math.min(1, p.downloaded / p.total) : null;
    });
    try {
      await api.updateDownload();
      this.phase = 'installing';
      this.outcome = await api.updateInstall();
      this.phase = this.outcome.mode === 'replaced' ? 'ready' : 'saved';
    } catch (e) {
      this.fail(e);
    } finally {
      off();
    }
  }

  async restart(): Promise<void> {
    try {
      await api.updateRestart();
    } catch (e) {
      this.fail(e);
    }
  }

  async setCheckOnStart(on: boolean): Promise<void> {
    try {
      this.prefs = await api.updatePrefsSet(on);
    } catch (e) {
      this.fail(e);
    }
  }

  private fail(e: unknown): void {
    this.error = errorText(e);
    this.phase = 'error';
  }

  /** Tests. */
  reset(): void {
    this.phase = 'idle';
    this.release = null;
    this.progress = null;
    this.outcome = null;
    this.error = null;
    this.prefs = null;
  }
}

export const update = new Update();
