import { call, isTauri, mock } from './api';

export interface PairPrompt {
  id: string;
  label: string;
  code: string;
  caller: string;
}

export interface PairedBrowser {
  id: string;
  label: string;
  created: number;
  last_used: number;
}

export interface BrowserAccess {
  at: number;
  site: string;
  title: string;
  kind: 'fill' | 'totp' | 'copy';
  browser: string;
  refused: boolean;
}

export interface BrowserInfo {
  link: boolean;
  portable: boolean;
  bridge: string | null;
  registered: { browser: string; path: string }[];
  error: string | null;
  sandboxed: string[];
  paired: PairedBrowser[];
  accesses: BrowserAccess[];
  default_volume: string | null;
  volumes: { path: string; name: string }[];
  pending: PairPrompt | null;
}

export type PairAnswer = 'accept' | 'refuse' | 'block';

export const browserInfo = () => call<BrowserInfo>('browser_info');
export const browserSet = (patch: { link?: boolean; default_volume?: string }) => call<BrowserInfo>('browser_set', { patch });
export const browserUnpair = (id: string) => call<BrowserInfo>('browser_unpair', { id });
export const browserPairAnswer = (id: string, answer: PairAnswer) => call<boolean>('browser_pair_answer', { id, answer });
export const browserUnlockCancel = () => call<void>('browser_unlock_cancel');

export async function onBrowser<T>(event: 'browser-pair' | 'browser-pair-closed' | 'browser-changed' | 'vault-opened', h: (p: T) => void): Promise<() => void> {
  if (isTauri) {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<T>(event, (e) => h(e.payload));
  }
  return mock().listen(event as never, h as never);
}

export interface CompactSpec {
  kind: 'unlock';
  path: string;
}

export function compactSpec(): CompactSpec | null {
  const w = globalThis as unknown as { __NV_COMPACT__?: CompactSpec };
  if (w.__NV_COMPACT__?.kind === 'unlock') return w.__NV_COMPACT__;
  if (typeof location !== 'undefined') {
    const q = new URLSearchParams(location.search);
    if (q.get('compact') === 'unlock') return { kind: 'unlock', path: q.get('path') ?? '' };
  }
  return null;
}
