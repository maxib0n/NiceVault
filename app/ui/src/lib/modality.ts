const KEYS = new Set(['Tab', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Enter', ' ', 'Escape', 'Home', 'End', 'PageUp', 'PageDown', 'F2', 'F6']);

import { isMobile } from './platform';

let mode: 'pointer' | 'keyboard' = isMobile ? 'pointer' : 'keyboard';

function set(m: 'pointer' | 'keyboard') {
  if (mode === m) return;
  mode = m;
  document.documentElement.dataset.input = m;
}

export function trackModality(): void {
  if (typeof window === 'undefined') return;
  document.documentElement.dataset.input = mode;
  window.addEventListener('pointerdown', () => set('pointer'), { capture: true, passive: true });
  window.addEventListener(
    'keydown',
    (e) => {
      if (KEYS.has(e.key) || e.ctrlKey || e.metaKey || e.altKey) set('keyboard');
    },
    { capture: true },
  );
}

export const pointerLast = (): boolean => mode === 'pointer';
