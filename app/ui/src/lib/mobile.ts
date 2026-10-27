import * as api from './api';
import { isMobile } from './platform';
import { installKeyboardInset, installLongPress } from './touch';

export { detectMobile, isMobile } from './platform';

export const BACKGROUND_LOCK_MS = 30_000;

export const DOCS_RECHECK_MS = 10_000;

async function lockAll(): Promise<boolean> {
  if (await api.mobileDocsBusy().catch(() => false)) return false;
  const open = await api.vaultListOpen().catch(() => []);
  await Promise.all(open.map((v) => api.vaultLock(v.id, 'screen').catch(() => undefined)));
  return true;
}

export function installBackgroundLock(doc: Document = document, now: () => number = Date.now, lock: () => Promise<boolean | void> = lockAll): () => void {
  let hiddenAt: number | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
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
