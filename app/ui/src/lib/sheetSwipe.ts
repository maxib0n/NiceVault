import { scrollerOf } from './gestures';

export interface SheetSwipeOptions {
  onclose: () => void;
  enabled?: () => boolean;
}

export const HANDLE_H = 56;
export const DISMISS_FRACTION = 0.3;
export const FLICK_SPEED = 0.5;
const SLOP = 8;

export function shouldDismiss(dy: number, height: number, vy: number): boolean {
  return dy > 0 && (dy >= height * DISMISS_FRACTION || (vy >= FLICK_SPEED && dy >= 24));
}

const reduced = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

export function sheetSwipe(node: HTMLElement, opts: SheetSwipeOptions) {
  let o = opts;
  let start: { id: number; x: number; y: number; handle: boolean; top: boolean } | null = null;
  let owned = false;
  let last = { y: 0, t: 0, vy: 0 };
  let anim: Animation | null = null;
  let cleanup = 0;
  node.style.transform = 'var(--nv-t)';

  const reset = () => {
    clearTimeout(cleanup);
    anim?.cancel();
    anim = null;
    node.style.removeProperty('--nv-t');
    delete node.dataset.dragging;
  };
  const down = (e: PointerEvent) => {
    if (e.pointerType === 'mouse' || !e.isPrimary || o.enabled?.() === false) return;
    reset();
    const t = e.target as Element | null;
    if (t?.closest('input[type="range"], textarea, [contenteditable="true"]')) return;
    const r = node.getBoundingClientRect();
    const handle = e.clientY - r.top <= HANDLE_H || !!t?.closest('[data-sheet-handle]');
    const ys = scrollerOf(t, node, 'y');
    const own = node.scrollHeight > node.clientHeight + 1 && getComputedStyle(node).overflowY !== 'visible';
    const top = (!ys || ys.scrollTop <= 0) && (!own || node.scrollTop <= 0);
    start = { id: e.pointerId, x: e.clientX, y: e.clientY, handle, top };
    owned = false;
    last = { y: e.clientY, t: e.timeStamp, vy: 0 };
  };
  const move = (e: PointerEvent) => {
    if (!start || e.pointerId !== start.id) return;
    const dx = e.clientX - start.x;
    const dy = e.clientY - start.y;
    const dt = Math.max(1, e.timeStamp - last.t);
    last = { y: e.clientY, t: e.timeStamp, vy: last.vy * 0.4 + ((e.clientY - last.y) / dt) * 0.6 };
    if (!owned) {
      if (Math.hypot(dx, dy) < SLOP) return;
      if (dy > 0 && dy > Math.abs(dx) && (start.handle || start.top)) owned = true;
      else {
        start = null;
        return;
      }
      node.dataset.dragging = 'true';
      try {
        node.setPointerCapture(e.pointerId);
      } catch {
      }
    }
    node.style.setProperty('--nv-t', `translate3d(0, ${Math.max(0, dy)}px, 0)`);
  };
  const up = (e: PointerEvent) => {
    if (!start || e.pointerId !== start.id) return;
    const dy = e.clientY - start.y;
    const was = owned;
    const vy = last.vy;
    start = null;
    owned = false;
    if (!was) return;
    const h = node.getBoundingClientRect().height || node.clientHeight;
    const dismiss = e.type !== 'pointercancel' && shouldDismiss(dy, h, vy);
    const from = `translate3d(0, ${Math.max(0, dy)}px, 0)`;
    const to = dismiss ? `translate3d(0, ${h + 40}px, 0)` : 'translate3d(0, 0, 0)';
    const finish = () => {
      if (dismiss) {
        o.onclose();
        cleanup = window.setTimeout(reset, 120);
      } else reset();
    };
    if (reduced() || typeof node.animate !== 'function') return finish();
    node.style.setProperty('--nv-t', from);
    anim = node.animate([{ '--nv-t': from }, { '--nv-t': to }] as Keyframe[], {
      duration: dismiss ? 160 : 260,
      easing: dismiss ? 'cubic-bezier(0.3, 0, 0.8, 0.6)' : 'cubic-bezier(0.2, 0.9, 0.25, 1.08)',
      fill: 'forwards',
    });
    anim.finished.then(finish, () => undefined);
  };
  const tmove = (e: TouchEvent) => {
    if (!e.cancelable || e.touches.length !== 1) return;
    if (owned) return e.preventDefault();
    if (!start || !(start.handle || start.top)) return;
    const dy = e.touches[0].clientY - start.y;
    if (dy >= 6 && dy >= Math.abs(e.touches[0].clientX - start.x)) e.preventDefault();
  };
  node.addEventListener('pointerdown', down);
  node.addEventListener('pointermove', move);
  node.addEventListener('pointerup', up);
  node.addEventListener('pointercancel', up);
  node.addEventListener('touchmove', tmove, { passive: false });
  return {
    update(n: SheetSwipeOptions) {
      o = n;
    },
    destroy() {
      clearTimeout(cleanup);
      anim?.cancel();
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('pointermove', move);
      node.removeEventListener('pointerup', up);
      node.removeEventListener('pointercancel', up);
      node.removeEventListener('touchmove', tmove);
    },
  };
}
