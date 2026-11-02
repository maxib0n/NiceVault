import { isTauri } from './api';
import { isMobile } from './platform';

export type FrameKind = 'linux' | 'windows' | 'macos' | null;

export function detectFrame(ua: string, tauri: boolean, mobile: boolean, search = ''): FrameKind {
  if (mobile) return null;
  const forced = new URLSearchParams(search).get('chrome');
  if (forced === 'linux' || forced === 'windows' || forced === 'macos') return forced;
  if (!tauri) return null;
  if (/Macintosh|Mac OS X/.test(ua)) return 'macos';
  if (/Windows/.test(ua)) return 'windows';
  return 'linux';
}

export const frame = $state({
  kind: typeof navigator === 'undefined' ? null : detectFrame(navigator.userAgent, isTauri, isMobile, location.search),
  maximized: false,
  fullscreen: false,
  focused: true,
});

const win = () => import('@tauri-apps/api/window').then(({ getCurrentWindow }) => getCurrentWindow());

function mirror() {
  const html = document.documentElement;
  if (frame.kind) html.dataset.frame = frame.kind;
  else delete html.dataset.frame;
  html.toggleAttribute('data-maximized', frame.maximized);
  html.toggleAttribute('data-fullscreen', frame.fullscreen);
  html.toggleAttribute('data-window-blur', !frame.focused);
}

export function installFrame(): () => void {
  if (!frame.kind) return () => {};
  const focus = (e: FocusEvent) => {
    if (e.target !== window) return;
    frame.focused = e.type === 'focus';
    mirror();
  };
  addEventListener('focus', focus);
  addEventListener('blur', focus);
  mirror();
  let unlisten: (() => void) | undefined;
  if (isTauri)
    void win().then(async (w) => {
      const sync = async () => {
        frame.maximized = await w.isMaximized();
        frame.fullscreen = await w.isFullscreen();
        mirror();
      };
      await sync();
      unlisten = await w.onResized(() => void sync());
    });
  return () => {
    removeEventListener('focus', focus);
    removeEventListener('blur', focus);
    unlisten?.();
  };
}

export async function startDrag() {
  if (isTauri) await (await win()).startDragging();
}

export async function minimizeWindow() {
  if (isTauri) await (await win()).minimize();
}

export async function toggleMaximizeWindow() {
  if (!isTauri) {
    frame.maximized = !frame.maximized;
    return mirror();
  }
  await (await win()).toggleMaximize();
}

export async function closeWindow() {
  if (isTauri) await (await win()).close();
}
