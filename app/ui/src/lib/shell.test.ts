import { describe, expect, it } from 'vitest';
import { installShellGuards } from './shell';

function fakeWin() {
  const ls: Record<string, ((e: unknown) => void)[]> = {};
  return {
    addEventListener: (t: string, f: (e: unknown) => void) => (ls[t] ??= []).push(f),
    removeEventListener: () => undefined,
    fire(type: string, init: Record<string, unknown>) {
      let prevented = false;
      const e = { ...init, preventDefault: () => (prevented = true) };
      for (const f of ls[type] ?? []) f(e);
      return prevented;
    },
  };
}

describe('shell guards', () => {
  it('blocks the browser context menu except in text fields', () => {
    const w = fakeWin();
    installShellGuards(w as unknown as Window, { app: false, devMenu: false });
    expect(w.fire('contextmenu', { target: null })).toBe(true);
  });
  it('a Tauri dev build keeps it', () => {
    const w = fakeWin();
    installShellGuards(w as unknown as Window, { app: true, devMenu: true });
    expect(w.fire('contextmenu', { target: null })).toBe(false);
  });
  it('blocks reload and page navigation only in the app', () => {
    const app = fakeWin();
    installShellGuards(app as unknown as Window, { app: true, devMenu: false });
    expect(app.fire('keydown', { key: 'F5', target: null })).toBe(true);
    expect(app.fire('keydown', { key: 'r', ctrlKey: true, target: null })).toBe(true);
    expect(app.fire('keydown', { key: 'ArrowLeft', altKey: true, target: null })).toBe(true);
    expect(app.fire('mouseup', { button: 3 })).toBe(true);
    expect(app.fire('keydown', { key: 'a', ctrlKey: true, target: null })).toBe(false);
    const mock = fakeWin();
    installShellGuards(mock as unknown as Window, { app: false, devMenu: false });
    expect(mock.fire('keydown', { key: 'F5', target: null })).toBe(false);
  });
});
