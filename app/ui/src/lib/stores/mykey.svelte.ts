import * as api from '../api';
import type { MyKey, RecentEntry } from '../types';

class MyKeyStore {
  key = $state<MyKey | null>(null);
  private loading: Promise<void> | null = null;

  load(force = false): Promise<void> {
    if (this.loading && !force) return this.loading;
    this.loading = api.myKeyGet().then(
      (k) => void (this.key = k),
      () => {},
    );
    return this.loading;
  }

  set(k: MyKey) {
    this.key = k;
  }

  get fingerprint(): string | null {
    return this.key?.fingerprint ?? null;
  }

  opens(r: RecentEntry | null | undefined): boolean {
    const f = this.fingerprint;
    return !!f && !!r?.recipients?.includes(f);
  }
}

export const myKey = new MyKeyStore();

export const groupFingerprint = (hex: string) => hex.replace(/(.{4})(?=.)/g, '$1 ');
