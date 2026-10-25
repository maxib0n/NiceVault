// Input modality: which kind of input the person last used, on <html data-input="pointer|keyboard">.
// Focus moved by the app (a dialog or a preview opening, a menu) counts as :focus-visible in
// WebKitGTK and Chromium even after a click; base.css hides the ring while the last input was a
// pointer, and Dialog/QuickLook/Menu read pointerLast() to focus their own panel instead of their
// first control. Keyboard users keep every ring and every focus move.

const KEYS = new Set(['Tab', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Enter', ' ', 'Escape', 'Home', 'End', 'PageUp', 'PageDown', 'F2', 'F6']);

import { isMobile } from './platform';

// a phone / tablet starts with a finger (a keyboard plugged in switches it on its first key)
let mode: 'pointer' | 'keyboard' = isMobile ? 'pointer' : 'keyboard';

function set(m: 'pointer' | 'keyboard') {
  if (mode === m) return;
  mode = m;
  document.documentElement.dataset.input = m;
}

/** Installs the listeners once (main.ts). */
export function trackModality(): void {
  if (typeof window === 'undefined') return;
  document.documentElement.dataset.input = mode;
  window.addEventListener('pointerdown', () => set('pointer'), { capture: true, passive: true });
  window.addEventListener(
    'keydown',
    (e) => {
      // a shortcut (Ctrl/Cmd+key) opening something is still the keyboard
      if (KEYS.has(e.key) || e.ctrlKey || e.metaKey || e.altKey) set('keyboard');
    },
    { capture: true },
  );
}

/** The last input was a pointer (a click or a tap). */
export const pointerLast = (): boolean => mode === 'pointer';
