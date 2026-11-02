import { isTauri } from './api';
import { switches } from './switches';

const GIVE_UP_MS = 8000;

let el: HTMLElement | null = null;

export function installSplash(compact: boolean): void {
  el = document.getElementById('nv-splash');
  if (!el) return;
  if (compact) {
    el.remove();
    el = null;
    return;
  }
  if (!isTauri) {
    const theme = switches.theme ?? switches.app_theme;
    if (theme && theme !== 'system' && !el.dataset.theme) el.dataset.theme = theme;
    const accent = switches.app_accent ?? switches.accent;
    if (accent && !el.dataset.accent) el.dataset.accent = accent;
  }
  el.dataset.state = 'in';
  el.addEventListener('click', skip);
  addEventListener('keydown', skip, true);
  if (!(!isTauri && switches.splash === 'hold')) setTimeout(() => leave(false), GIVE_UP_MS);
}

export function splashReady(): void {
  const s = isTauri ? null : switches.splash;
  if (s === 'hold') return;
  const wait = typeof s === 'number' ? s - performance.now() : 0;
  if (wait > 0) setTimeout(() => leave(false), wait);
  else leave(false);
}

function skip(): void {
  leave(true);
}

function leave(skipped: boolean): void {
  const s = el;
  if (!s || s.dataset.state === 'out') return;
  s.removeEventListener('click', skip);
  removeEventListener('keydown', skip, true);
  s.dataset.state = 'out';
  s.classList.add('out');
  if (skipped) s.classList.add('skip');
  const gone = () => {
    s.remove();
    if (el === s) el = null;
  };
  s.addEventListener('animationend', (e) => e.target === s && gone());
  setTimeout(gone, 600);
}
