// Android / iOS: marks <html data-platform="mobile"> for styles/mobile.css
// (hides what can't work on a phone: mount as drive, Crea dispositivo, updater, tray/autostart)
// and locks every open volume when the app stays in the background.
import * as api from './api';
import { isMobile } from './platform';
import { installKeyboardInset, installLongPress } from './touch';

export { detectMobile, isMobile } from './platform';

/** In the background longer than this: every open volume is locked (reason "screen"). */
export const BACKGROUND_LOCK_MS = 30_000;

/** While another app reads / writes a volume through the Files provider (Android), asked again this often. */
export const DOCS_RECHECK_MS = 10_000;

/** false: postponed, another app is using a volume through the Files provider (Android, docs.rs). */
async function lockAll(): Promise<boolean> {
  if (await api.mobileDocsBusy().catch(() => false)) return false;
  const open = await api.vaultListOpen().catch(() => []);
  await Promise.all(open.map((v) => api.vaultLock(v.id, 'screen').catch(() => undefined)));
  return true;
}

/**
 * Lock on background: a timer while hidden, and a check on return (the OS may freeze the
 * page's timers while it is in the background, so the timer alone is not enough).
 */
export function installBackgroundLock(doc: Document = document, now: () => number = Date.now, lock: () => Promise<boolean | void> = lockAll): () => void {
  let hiddenAt: number | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  // postponed (the Files provider is in use): try again while still in the background
  const fire = () => {
    timer = null;
    void lock().then((done) => {
      if (done === false && doc.visibilityState === 'hidden' && timer === null) timer = setTimeout(fire, DOCS_RECHECK_MS);
    });
  };
  const on = () => {
    if (doc.visibilityState === 'hidden') {
      hiddenAt = now();
      if (timer) clearTimeout(timer);
      timer = setTimeout(fire, BACKGROUND_LOCK_MS);
    } else {
      if (timer) clearTimeout(timer);
      timer = null;
      if (hiddenAt !== null && now() - hiddenAt >= BACKGROUND_LOCK_MS) void lock();
      hiddenAt = null;
    }
  };
  doc.addEventListener('visibilitychange', on);
  return () => {
    doc.removeEventListener('visibilitychange', on);
    if (timer) clearTimeout(timer);
  };
}

const shown = (el: Element | null): el is HTMLElement => !!el && 'click' in el && el.getClientRects().length > 0;

/**
 * Android's Back (MainActivity.kt calls `window.__nvBack()`): one step back in the app, true when
 * something was closed; false lets the OS put the app in the background. In order: the topmost
 * dialog (the preview, a page-dialog: as Esc would, so full screen / pickers behave the same),
 * a menu, picking several (Fine), an entry's Indietro, Impostazioni, Sblocca / Nuovo volume, a page of the volume other
 * than File, a subfolder.
 */
export function goBack(doc: Document = document): boolean {
  const dialogs = [...doc.querySelectorAll<HTMLDialogElement>('dialog[open]')];
  const top = dialogs.at(-1);
  if (top) {
    if (top.dispatchEvent(new Event('cancel', { cancelable: true }))) top.close();
    return true;
  }
  const menu = doc.querySelector('[role="menu"]');
  if (menu) {
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    return true;
  }
  const click = (sel: string) => {
    const el = doc.querySelector(sel);
    if (!shown(el) || (el as HTMLButtonElement).disabled) return false;
    el.click();
    return true;
  };
  if (click('[data-testid="mobile-select-done"]')) return true;
  if (click('[data-testid="mobile-back"]')) return true;
  if (click('[data-testid="settings-back"]')) return true;
  const screen = doc.querySelector<HTMLElement>('[data-screen]')?.dataset.screen;
  if (screen === 'unlock' || screen === 'create' || screen === 'device') {
    return click('[data-screen] .top .back') || click('[data-screen] footer.nav > .btn.ghost:first-child');
  }
  if (screen === 'passwords' || screen === 'details') return click('[data-testid="nav-files"]');
  if (screen === 'vault') return click('.folder-head .up');
  return false;
}

export function installMobile(): void {
  if (!isMobile) return;
  document.documentElement.dataset.platform = 'mobile';
  installBackgroundLock();
  installLongPress(window, { buzz: () => navigator.vibrate?.(8) });
  installKeyboardInset();
  (window as unknown as { __nvBack?: () => boolean }).__nvBack = () => goBack();
}
