import { describe, expect, it, vi } from 'vitest';
import { BACKGROUND_LOCK_MS, DOCS_RECHECK_MS, detectMobile, installBackgroundLock } from './mobile';
import { detectIos } from './platform';
import * as api from './api';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';
import type { PickProgress } from './types';
import { ROOT } from './types';

describe('mobile', () => {
  it('tells iOS from Android (iPadOS says Macintosh, with touch)', () => {
    expect(detectIos('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15', 5)).toBe(true);
    expect(detectIos('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15', 5)).toBe(true);
    expect(detectIos('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15', 0)).toBe(false);
    expect(detectIos('Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 Chrome/131 Mobile Safari/537.36', 5)).toBe(false);
  });

  it('imports the camera roll item by item, reporting progress to the end (mock parity)', async () => {
    const mock = new MockBackend({ speed: 0 });
    api.useMock(mock);
    const v = await api.vaultOpen(`${STICK}/Documenti.nvault`, MOCK_PASSWORD);
    const events: PickProgress[] = [];
    const done = new Promise<void>((res) => {
      void api.onPickProgress((p) => {
        events.push(p);
        if (p.finished) res();
      });
    });
    const r = await api.mobileImportPhotos(v.id, ROOT);
    expect(r?.total).toBe(3);
    await done;
    const last = events.at(-1)!;
    expect(last).toMatchObject({ job: r!.job, total: 3, done: 3, imported: 3, finished: true, failures: [] });
    expect(events.map((e) => e.done)).toEqual([...events.map((e) => e.done)].sort((a, b) => a - b));
    const names = (await api.fsList(v.id, ROOT)).map((n) => n.name);
    for (const n of ['IMG_0412.HEIC', 'IMG_0413.MOV', 'IMG_0415.JPG']) expect(names).toContain(n);
    expect((await api.mobilePickFiles(false))[0]).toMatch(/\.nvault$/);
  });

  it('detects the mobile webviews and the screenshot switch', () => {
    expect(detectMobile('Mozilla/5.0 (Linux; Android 15; sdk_gphone64_x86_64) AppleWebKit/537.36 Chrome/131 Mobile Safari/537.36', '')).toBe(true);
    expect(detectMobile('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15', '')).toBe(true);
    expect(detectMobile('Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15', '')).toBe(false);
    expect(detectMobile('Mozilla/5.0 (X11; Linux x86_64)', '?platform=mobile')).toBe(true);
    expect(detectMobile('Mozilla/5.0 (Linux; Android 15)', '?platform=desktop')).toBe(false);
  });

  it('locks after the grace period in the background, also when timers were frozen', async () => {
    vi.useFakeTimers();
    let state: DocumentVisibilityState = 'visible';
    const target = new EventTarget();
    const doc = Object.defineProperty(target, 'visibilityState', { get: () => state }) as unknown as Document;
    let t = 0;
    const lock = vi.fn(async () => {});
    const off = installBackgroundLock(doc, () => t, lock);
    const flip = (s: DocumentVisibilityState) => { state = s; target.dispatchEvent(new Event('visibilitychange')); };

    flip('hidden'); t += 5_000; vi.advanceTimersByTime(5_000); flip('visible');
    expect(lock).not.toHaveBeenCalled();
    flip('hidden'); vi.advanceTimersByTime(BACKGROUND_LOCK_MS);
    expect(lock).toHaveBeenCalledTimes(1);
    flip('visible');
    flip('hidden'); t += BACKGROUND_LOCK_MS + 1; flip('visible');
    expect(lock).toHaveBeenCalledTimes(2);
    off();
    vi.useRealTimers();
  });

  it('waits while another app uses a volume through the Files provider (Android)', async () => {
    vi.useFakeTimers();
    let state: DocumentVisibilityState = 'visible';
    const target = new EventTarget();
    const doc = Object.defineProperty(target, 'visibilityState', { get: () => state }) as unknown as Document;
    let busy = true;
    let locked = 0;
    const lock = vi.fn(async () => {
      if (busy) return false;
      locked++;
      return true;
    });
    const off = installBackgroundLock(doc, () => 0, lock);
    state = 'hidden';
    target.dispatchEvent(new Event('visibilitychange'));
    await vi.advanceTimersByTimeAsync(BACKGROUND_LOCK_MS);
    expect(lock).toHaveBeenCalledTimes(1);
    expect(locked).toBe(0);
    await vi.advanceTimersByTimeAsync(DOCS_RECHECK_MS);
    expect(lock).toHaveBeenCalledTimes(2);
    busy = false;
    await vi.advanceTimersByTimeAsync(DOCS_RECHECK_MS);
    expect(locked).toBe(1);
    await vi.advanceTimersByTimeAsync(DOCS_RECHECK_MS * 3);
    expect(lock).toHaveBeenCalledTimes(3);
    off();
    vi.useRealTimers();
  });
});

describe('goBack (Android Back)', () => {
  function docWith(prevent: boolean) {
    const dlg = Object.assign(new EventTarget(), { closed: false, close() { this.closed = true; } });
    if (prevent) dlg.addEventListener('cancel', (e) => e.preventDefault());
    let open = true;
    const doc = {
      querySelectorAll: () => (open && !dlg.closed ? [dlg] : []),
      querySelector: () => null,
    } as unknown as Document;
    return { dlg, doc, shut: () => (open = false) };
  }
  it('closes the topmost dialog as Esc would; then there is nothing to go back from', async () => {
    const { goBack } = await import('./mobile');
    const { dlg, doc } = docWith(false);
    expect(goBack(doc)).toBe(true);
    expect(dlg.closed).toBe(true);
    expect(goBack(doc)).toBe(false);
  });
  it('a cancel that is prevented keeps the dialog (full screen leaves first)', async () => {
    const { goBack } = await import('./mobile');
    const { dlg, doc } = docWith(true);
    expect(goBack(doc)).toBe(true);
    expect(dlg.closed).toBe(false);
  });
});
