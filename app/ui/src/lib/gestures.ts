// Touch gestures of the full-screen preview (QuickLook): swipe left/right between the files of
// the folder, swipe down to close, pinch / double-tap to zoom a picture. Pointer Events only
// (touch and pen; a mouse drags nothing here), transform-only while the finger moves.

export interface SwipeOptions {
  onLeft?: () => void;
  onRight?: () => void;
  onDown?: () => void;
  /** a tap (no movement): shows / hides the chrome */
  onTap?: () => void;
  /** false while a picture is zoomed: its pan wins */
  enabled?: () => boolean;
}

/** Minimum travel (CSS px) and the dominant-axis ratio for a swipe. */
export const SWIPE_MIN = 56;
/** A flick: shorter than SWIPE_MIN but at least this far, this fast (px/ms) at release. */
export const FLICK_MIN = 24;
export const FLICK_SPEED = 0.45;
const TAP_SLOP = 10;

export interface Stroke {
  dx: number;
  dy: number;
  /** velocity at release, px/ms (optional: a slow stroke) */
  vx?: number;
  vy?: number;
}

/** What a finished one-finger stroke means. */
export function classify({ dx, dy, vx = 0, vy = 0 }: Stroke): 'left' | 'right' | 'down' | 'tap' | null {
  const ax = Math.abs(dx);
  const ay = Math.abs(dy);
  if (ax < TAP_SLOP && ay < TAP_SLOP) return 'tap';
  const flickX = ax >= FLICK_MIN && Math.abs(vx) >= FLICK_SPEED && Math.sign(vx) === Math.sign(dx);
  if ((ax >= SWIPE_MIN || flickX) && ax > ay * 1.4) return dx < 0 ? 'left' : 'right';
  const flickY = dy >= FLICK_MIN * 2 && vy >= FLICK_SPEED;
  if ((dy >= SWIPE_MIN * 1.5 || flickY) && dy > ax * 1.4) return 'down';
  return null;
}

/** Rubber band past the end of a drag: the further, the stiffer. */
export function rubber(d: number, limit: number): number {
  const a = Math.abs(d);
  return Math.sign(d) * (limit * a) / (limit + a);
}

const reduced = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/**
 * The page in the finger's hand: transform only, and the release / settle animations go through the
 * registered --nv-t / --nv-o (styles/motion.css: never an animation of transform or opacity itself).
 */
function glide(node: HTMLElement, from: string, to: string, o: [number, number] | null, ms: number, easing: string, hold?: (a: Animation) => void): Promise<void> {
  if (reduced() || typeof node.animate !== 'function') return Promise.resolve();
  node.style.transform = 'var(--nv-t)';
  if (o) node.style.opacity = 'var(--nv-o)';
  const frames = o ? [{ '--nv-t': from, '--nv-o': String(o[0]) }, { '--nv-t': to, '--nv-o': String(o[1]) }] : [{ '--nv-t': from }, { '--nv-t': to }];
  const a = node.animate(frames as Keyframe[], { duration: ms, easing, fill: 'forwards' });
  hold?.(a);
  // a cancelled glide (a new touch took the page) resolves too: its caller just stops
  return a.finished.then(
    () => a.cancel(),
    () => undefined,
  );
}

/** A touch this soon after the document scrolled stops its momentum: not a tap (ms). */
const FLING_STOP = 180;

const scrolls = (el: Element, axis: 'x' | 'y') => {
  const cs = getComputedStyle(el);
  const ov = axis === 'y' ? cs.overflowY : cs.overflowX;
  if (ov !== 'auto' && ov !== 'scroll') return false;
  return axis === 'y' ? el.scrollHeight > el.clientHeight + 1 : el.scrollWidth > el.clientWidth + 1;
};
/** The nearest element between `from` and `root` that scrolls along `axis`, if any. */
export function scrollerOf(from: Element | null, root: Element, axis: 'x' | 'y'): Element | null {
  for (let el = from; el && el !== root; el = el.parentElement) if (scrolls(el, axis)) return el;
  return null;
}
const interactive = (t: Element | null) => !!t?.closest('a[href], button, input, select, textarea, summary, label, [role="button"], [role="link"], [contenteditable="true"]');

export function swipe(node: HTMLElement, opts: SwipeOptions) {
  let o = opts;
  /** ys / xs: the document under the finger scrolls that way (its own stroke, not ours) */
  /** top: the document under it is at its top, so a pull down is ours (close), not its scroll */
  let start: { id: number; x: number; y: number; ys: Element | null; xs: Element | null; top: boolean; flung: boolean; target: Element | null } | null = null;
  /** the stroke is ours (decided): the browser must not pan with it */
  let owned = false;
  let scrolledAt = -1e9;
  let last: { x: number; y: number; t: number; vx: number; vy: number } | null = null;
  let axis: 'x' | 'y' | null = null;
  let multi = false;
  // leaving: the page is on its way out, the callback still to come (touches wait)
  let settling = false;
  // arriving / springing back: a new touch takes the page where it is
  let anim: Animation | null = null;
  const hold = (a: Animation) => (anim = a);
  // each touch starts a new generation: a glide that resolves after a newer touch leaves the page alone
  let gen = 0;
  const touches = new Set<number>();
  const rest = () => {
    node.style.transform = '';
    node.style.opacity = '';
  };
  const down = (e: PointerEvent) => {
    if (e.pointerType === 'mouse' || settling) return;
    gen++;
    if (anim) {
      anim.cancel();
      anim = null;
      rest();
    }
    touches.add(e.pointerId);
    if (touches.size > 1) {
      multi = true; // a pinch, not a swipe
      start = null;
      rest();
      return;
    }
    multi = false;
    axis = null;
    owned = false;
    const t = e.target as Element | null;
    const ys = scrollerOf(t, node, 'y');
    start = { id: e.pointerId, x: e.clientX, y: e.clientY, ys, xs: scrollerOf(t, node, 'x'), top: !ys || ys.scrollTop <= 0, flung: performance.now() - scrolledAt < FLING_STOP, target: t };
    last = { x: e.clientX, y: e.clientY, t: e.timeStamp, vx: 0, vy: 0 };
  };
  const move = (e: PointerEvent) => {
    if (!start || e.pointerId !== start.id || multi || o.enabled?.() === false) return;
    const dx = e.clientX - start.x;
    const dy = e.clientY - start.y;
    if (last) {
      const dt = Math.max(1, e.timeStamp - last.t);
      // a smoothed velocity: the last few moves, not one noisy sample
      last = { x: e.clientX, y: e.clientY, t: e.timeStamp, vx: last.vx * 0.4 + ((e.clientX - last.x) / dt) * 0.6, vy: last.vy * 0.4 + ((e.clientY - last.y) / dt) * 0.6 };
    }
    // the axis is decided once, after a few pixels: a page never wobbles diagonally
    if (!axis && Math.hypot(dx, dy) > TAP_SLOP) {
      // sideways only when clearly sideways (a document scrolled with a slightly slanted thumb
      // stays a scroll); a document that scrolls that way keeps the stroke: it is its scroll
      axis = Math.abs(dx) > Math.abs(dy) * (start.ys ? 2 : 1) ? 'x' : 'y';
      const mine = axis === 'x' ? !start.xs : !start.ys || (start.top && dy > 0);
      owned = mine;
      if (!mine) {
        start = null;
        last = null;
        rest();
        return;
      }
    }
    // the page follows the finger 1:1 sideways (it is the next page that comes in), and down
    // with a little give; up resists (nothing is there)
    if (axis === 'x') node.style.transform = `translate3d(${dx}px, 0, 0) scale(1)`;
    else if (axis === 'y') {
      const y = dy > 0 ? dy : rubber(dy, 40);
      const s = dy > 0 ? 1 - Math.min(0.12, dy / 2400) : 1;
      node.style.transform = `translate3d(0, ${y}px, 0) scale(${s})`;
      node.style.opacity = dy > 0 ? String(1 - Math.min(0.5, dy / 900)) : '';
    }
  };
  const up = (e: PointerEvent) => {
    touches.delete(e.pointerId);
    if (e.pointerType !== 'mouse') strokeAt = performance.now();
    if (!start || e.pointerId !== start.id) return;
    const s: Stroke = { dx: e.clientX - start.x, dy: e.clientY - start.y, vx: last?.vx ?? 0, vy: last?.vy ?? 0 };
    const tgt = start.target;
    const s0flung = start.flung;
    const held = node.style.transform;
    const heldO = Number(node.style.opacity || 1);
    const g = gen;
    const done = () => {
      if (g !== gen) return;
      rest();
      anim = null;
    };
    start = null;
    last = null;
    if (multi || e.type === 'pointercancel' || o.enabled?.() === false) {
      if (held) void settle(held, null, done);
      else rest();
      return;
    }
    const what = classify(s);
    if (what === 'tap') {
      rest();
      // a touch that stopped a scroll's momentum is not a tap on the page
      if (s0flung) return;
      // a link or a key in the document is its own tap, not the chrome's
      return interactive(tgt) ? undefined : o.onTap?.();
    }
    const w = node.clientWidth || window.innerWidth;
    if (what === 'left' || what === 'right') {
      const sign = what === 'left' ? -1 : 1;
      settling = true;
      // out with the finger's speed, the next one in from the other side
      void glide(node, held || 'translate3d(0, 0, 0) scale(1)', `translate3d(${sign * w}px, 0, 0) scale(1)`, null, 150, 'cubic-bezier(0.3, 0, 0.8, 0.6)')
        .then(() => {
          rest();
          settling = false;
          (what === 'left' ? o.onLeft : o.onRight)?.();
          return glide(node, `translate3d(${-sign * w * 0.3}px, 0, 0) scale(1)`, 'translate3d(0, 0, 0) scale(1)', [0, 1], 240, 'cubic-bezier(0.2, 0.8, 0.2, 1)', hold);
        })
        .then(done);
      return;
    }
    if (what === 'down') {
      settling = true;
      void glide(node, held || 'translate3d(0, 0, 0) scale(1)', `translate3d(0, ${Math.max(s.dy, 0) + w * 0.5}px, 0) scale(0.88)`, [heldO, 0], 170, 'cubic-bezier(0.3, 0, 0.8, 0.6)').then(() => {
        settling = false;
        o.onDown?.();
        rest();
      });
      return;
    }
    void settle(held, heldO, done);
  };
  // not far enough: back to rest, with a little spring
  function settle(held: string, heldO: number | null, done: () => void) {
    if (!held) return rest();
    return glide(node, held, 'translate3d(0, 0, 0) scale(1)', heldO === null || heldO === 1 ? null : [heldO, 1], 260, 'cubic-bezier(0.2, 0.9, 0.25, 1.08)', hold).then(done);
  }
  // a pull down on a document at its top: with touch-action pan-y the browser would claim the
  // stroke (an overscroll that goes nowhere) and cancel our pointer; a cancelled touchmove keeps
  // it ours. Only then: everything else stays the document's own scroll.
  const tmove = (e: TouchEvent) => {
    if (!start || multi || e.touches.length !== 1 || !e.cancelable || o.enabled?.() === false) return;
    const t = e.touches[0];
    const dx = t.clientX - start.x;
    const dy = t.clientY - start.y;
    if (owned || (!axis && start.ys && start.top && dy >= 6 && dy >= Math.abs(dx))) e.preventDefault();
  };
  // momentum follows a finger: a scroll counts only within a few seconds of the last stroke
  // (a PDF laying out its pages, scroll anchoring, a key: not a fling a tap could stop)
  let strokeAt = -1e9;
  const scrolled = () => {
    if (performance.now() - strokeAt < 3000) scrolledAt = performance.now();
  };
  node.addEventListener('pointerdown', down);
  node.addEventListener('pointermove', move);
  node.addEventListener('pointerup', up);
  node.addEventListener('pointercancel', up);
  node.addEventListener('touchmove', tmove, { passive: false });
  node.addEventListener('scroll', scrolled, { capture: true, passive: true });
  return {
    update(n: SwipeOptions) {
      o = n;
    },
    destroy() {
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('pointermove', move);
      node.removeEventListener('pointerup', up);
      node.removeEventListener('pointercancel', up);
      node.removeEventListener('touchmove', tmove);
      node.removeEventListener('scroll', scrolled, { capture: true });
    },
  };
}

export interface ZoomState {
  scale: number;
  x: number;
  y: number;
}

export const ZOOM_MAX = 6;

/** The scale after a pinch from `d0` to `d1` finger distance, clamped to 1..ZOOM_MAX. */
export function pinchScale(from: number, d0: number, d1: number): number {
  if (d0 <= 0) return from;
  return Math.min(ZOOM_MAX, Math.max(1, from * (d1 / d0)));
}

/** Pinch to zoom, pan while zoomed, double-tap toggles 2x. Reports whether it is zoomed. */
export function pinchZoom(node: HTMLElement, onZoom?: (zoomed: boolean) => void) {
  let z: ZoomState = { scale: 1, x: 0, y: 0 };
  const pts = new Map<number, { x: number; y: number }>();
  let pinch: { d: number; scale: number } | null = null;
  let pan: { x: number; y: number; ox: number; oy: number } | null = null;
  let lastTap = 0;
  const apply = () => {
    node.style.transform = z.scale === 1 ? '' : `translate3d(${z.x}px, ${z.y}px, 0) scale(${z.scale})`;
    node.style.touchAction = z.scale === 1 ? '' : 'none';
    onZoom?.(z.scale > 1);
  };
  const dist = () => {
    const [a, b] = [...pts.values()];
    return Math.hypot(a.x - b.x, a.y - b.y);
  };
  const down = (e: PointerEvent) => {
    if (e.pointerType === 'mouse') return;
    pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pts.size === 2) {
      pinch = { d: dist(), scale: z.scale };
      pan = null;
    } else if (pts.size === 1 && z.scale > 1) {
      pan = { x: e.clientX, y: e.clientY, ox: z.x, oy: z.y };
      e.stopPropagation(); // zoomed: the finger pans, it doesn't swipe
    }
  };
  const move = (e: PointerEvent) => {
    if (!pts.has(e.pointerId)) return;
    pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pinch && pts.size === 2) {
      z = { ...z, scale: pinchScale(pinch.scale, pinch.d, dist()) };
      if (z.scale === 1) z = { scale: 1, x: 0, y: 0 };
      apply();
    } else if (pan) {
      z = { ...z, x: pan.ox + e.clientX - pan.x, y: pan.oy + e.clientY - pan.y };
      apply();
      e.stopPropagation();
    }
  };
  const up = (e: PointerEvent) => {
    if (!pts.delete(e.pointerId)) return;
    if (pts.size < 2) pinch = null;
    if (pts.size === 0) {
      pan = null;
      const now = e.timeStamp;
      if (now - lastTap < 300) {
        z = z.scale > 1 ? { scale: 1, x: 0, y: 0 } : { scale: 2, x: 0, y: 0 };
        apply();
        lastTap = 0;
      } else lastTap = now;
    }
  };
  node.addEventListener('pointerdown', down);
  node.addEventListener('pointermove', move);
  node.addEventListener('pointerup', up);
  node.addEventListener('pointercancel', up);
  return {
    destroy() {
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('pointermove', move);
      node.removeEventListener('pointerup', up);
      node.removeEventListener('pointercancel', up);
      onZoom?.(false);
    },
  };
}
