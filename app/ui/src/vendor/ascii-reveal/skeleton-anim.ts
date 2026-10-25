/**
 * Shimmer for ASCII skeletons, drawn on a small canvas inside each skeleton.
 *
 * The skeleton markup is a still glyph pattern that paints with no JavaScript. Once this runs, each row gets the
 * sweep: a soft band moves across it (rows staggered), and inside the band denser glyphs flicker between two sets.
 * Same look as a CSS mask animation, at roughly a third of the CPU, because the browser never repaints the rows:
 * every row's glyphs are rasterised once, and a frame only copies the band region.
 */
import { BLOCK_LEN, glyphRow, LINE_LEN, SKELETON_VARIANTS, type SkeletonVariant } from './skeleton';
import { isBrowser, reducedMotion } from './util';

const FPS_MS = 33; // ~30 fps: the glyphs only change every 120 ms anyway
const FLICK_MS = 120;
const STAGGER_MS = 90;
const STRIPS = 16; // soft band edges: transparent → opaque (45–55%) → transparent

interface Row {
  i: number;
  x: number; y: number; w: number; h: number;
  radius: number;
  img: HTMLCanvasElement;
  fh: number;
}
interface Sk {
  root: HTMLElement;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  rows: Row[];
  dirty: boolean;
  visible: boolean;
  band: number; opacity: number; speed: number;
  ro: ResizeObserver | null;
  mw: number; mh: number;
  /** rAF frame it was added on: measuring waits until the skeleton has painted once */
  born: number;
}

const all = new Set<Sk>();
const byRoot = new WeakMap<HTMLElement, Sk>();
let raf = 0, last = 0, t0 = 0, frame = 0;
let io: IntersectionObserver | null = null;

// cubic-bezier(.45,.05,.55,.95), the same easing as the original CSS sweep
const EASE = (() => {
  const lut = new Float32Array(257);
  for (let k = 0; k <= 256; k++) {
    const t = k / 256;
    let lo = 0, hi = 1, u = t;
    for (let n = 0; n < 28; n++) {
      u = (lo + hi) / 2;
      const x = 3 * (1 - u) ** 2 * u * 0.45 + 3 * (1 - u) * u * u * 0.55 + u ** 3;
      if (x < t) lo = u; else hi = u;
    }
    lut[k] = 3 * (1 - u) ** 2 * u * 0.05 + 3 * (1 - u) * u * u * 0.95 + u ** 3;
  }
  return (p: number) => lut[Math.round(p * 256)];
})();

const variantOf = (root: HTMLElement): SkeletonVariant =>
  ((Object.keys(SKELETON_VARIANTS) as SkeletonVariant[]).find((v) => root.classList.contains('ars-' + v)) || 'dither');

/** Reads layout and rasterises glyphs (offscreen, so no layout writes); returns the canvas resize to apply after all reads. */
function measure(S: Sk): () => void {
  S.dirty = false;
  const root = S.root;
  const rr = root.getBoundingClientRect();
  S.mw = rr.width; S.mh = rr.height;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const cs = getComputedStyle(root);
  S.band = (parseFloat(cs.getPropertyValue('--ars-band')) || 45) / 100;
  S.opacity = parseFloat(cs.getPropertyValue('--ars-wave')) || 0.9;
  S.speed = (parseFloat(cs.getPropertyValue('--ars-speed')) || 1.8) * 1000;
  const variant = variantOf(root);
  const seed = Number(root.getAttribute('data-ars-animate')) || 1;
  let waveColor = cs.getPropertyValue('--ars-wave-color').trim();
  S.rows = Array.from(root.querySelectorAll<HTMLElement>('.ars-line,.ars-block')).map((el, i) => {
    const r = el.getBoundingClientRect();
    const base = el.querySelector<HTMLElement>('.ars-base') || el;
    const bs = getComputedStyle(base), es = getComputedStyle(el);
    const block = el.classList.contains('ars-block');
    const nRows = (base.textContent || '').split('\n').length;
    const lh = parseFloat(es.lineHeight) || r.height / nRows;
    const color = !waveColor || /currentcolor/i.test(waveColor) ? bs.color : waveColor;
    // both flicker frames go into one canvas, stacked: frame k starts at y = k * height
    const fw = Math.max(1, Math.ceil(r.width * dpr)), fh = Math.max(1, Math.ceil(r.height * dpr));
    const c = document.createElement('canvas');
    c.width = fw; c.height = fh * 2;
    const x = c.getContext('2d')! as CanvasRenderingContext2D & { letterSpacing?: string };
    x.font = `${bs.fontStyle} ${bs.fontWeight} ${bs.fontSize} ${bs.fontFamily}`;
    x.fillStyle = color;
    x.textBaseline = 'middle';
    const ls = parseFloat(bs.letterSpacing) || 0;
    const cw = x.measureText('M').width + ls;
    // only the glyphs that can show; one fillText per line where the canvas supports letter spacing
    const n = Math.min(block ? BLOCK_LEN : LINE_LEN, Math.ceil(r.width / Math.max(1, cw)) + 1);
    const whole = !ls || typeof x.letterSpacing === 'string';
    if (ls && whole) x.letterSpacing = ls + 'px';
    const v = SKELETON_VARIANTS[variant];
    const rs = seed * 97 + i * 13;
    for (let k = 0; k < 2; k++) {
      x.setTransform(dpr, 0, 0, dpr, 0, k * fh);
      for (let li = 0; li < nRows; li++) {
        // same glyph rows the CSS version used for its two flicker layers
        const line = glyphRow(rs + li + 300 * (k + 1), n, v.wave, v.gap, v.bias);
        const cy = li * lh + lh / 2;
        if (whole) x.fillText(line, 0, cy);
        else for (let j = 0; j < n; j++) if (line[j] !== ' ') x.fillText(line[j], j * cw, cy);
      }
    }
    const rad = es.borderTopLeftRadius;
    const radius = rad.endsWith('%') ? (parseFloat(rad) / 100) * Math.min(r.width, r.height) : parseFloat(rad) || 0;
    return { i, x: r.left - rr.left, y: r.top - rr.top, w: r.width, h: r.height, radius: Math.min(radius, Math.min(r.width, r.height) / 2), img: c, fh };
  });
  return () => {
    S.canvas.width = Math.max(1, Math.ceil(rr.width * dpr));
    S.canvas.height = Math.max(1, Math.ceil(rr.height * dpr));
    S.canvas.style.width = rr.width + 'px';
    S.canvas.style.height = rr.height + 'px';
  };
}

function draw(S: Sk, t: number) {
  const { ctx, canvas } = S;
  const dpr = canvas.width / Math.max(1, parseFloat(canvas.style.width) || 1);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const flick = Math.floor(t / FLICK_MS) % 2;
  for (const R of S.rows) {
    const p = ((((t - R.i * STAGGER_MS) % S.speed) + S.speed) % S.speed) / S.speed;
    const bw = R.w * S.band;
    // the CSS version swept mask-position from -120% to 220%: offset = (width - band) * position
    const bx = (R.w - bw) * (-1.2 + 3.4 * EASE(p));
    const x0 = Math.max(0, bx), x1 = Math.min(R.w, bx + bw);
    if (x1 <= x0) continue;
    ctx.save();
    ctx.beginPath();
    if (R.radius > 0 && ctx.roundRect) ctx.roundRect(R.x, R.y, R.w, R.h, R.radius);
    else ctx.rect(R.x, R.y, R.w, R.h);
    ctx.clip();
    for (let k = 0; k < STRIPS; k++) {
      const a0 = bx + (bw * k) / STRIPS, a1 = bx + (bw * (k + 1)) / STRIPS;
      const s0 = Math.max(x0, a0), s1 = Math.min(x1, a1);
      if (s1 <= s0) continue;
      const m = (k + 0.5) / STRIPS;
      ctx.globalAlpha = (m < 0.45 ? m / 0.45 : m > 0.55 ? (1 - m) / 0.45 : 1) * S.opacity;
      ctx.drawImage(R.img, s0 * dpr, flick * R.fh, (s1 - s0) * dpr, R.h * dpr, R.x + s0, R.y, s1 - s0, R.h);
    }
    ctx.restore();
  }
}

function tick(now: number) {
  raf = all.size ? requestAnimationFrame(tick) : 0;
  frame++;
  if (document.hidden || now - last < FPS_MS) return;
  last = now;
  const t = now - t0;
  // measuring rasterises glyphs: spread it over frames (~6 ms each) so a page full of skeletons never blocks input
  // first paint is never delayed: a new skeleton is measured two frames after it mounts.
  // all layout reads first, then the canvas resizes, so measuring many skeletons costs one layout, not one each
  let budget = 6;
  const writes: (() => void)[] = [];
  for (const S of all) {
    if (!S.dirty || !S.visible || !S.root.isConnected || budget <= 0 || frame - S.born < 2) continue;
    const m0 = performance.now();
    writes.push(measure(S));
    budget -= performance.now() - m0;
  }
  writes.forEach((w) => w());
  for (const S of all) if (!S.dirty && S.visible && S.root.isConnected) draw(S, t);
}

/** Start the shimmer on one skeleton root (an element with class `ars`). Returns a function that stops it. */
export function animateSkeleton(root: HTMLElement): () => void {
  if (!isBrowser() || reducedMotion()) return () => {};
  const existing = byRoot.get(root);
  if (existing) return () => stop(existing);
  const canvas = document.createElement('canvas');
  canvas.setAttribute('aria-hidden', 'true');
  root.appendChild(canvas);
  const S: Sk = { root, canvas, ctx: canvas.getContext('2d')!, rows: [], dirty: true, visible: true, band: 0.45, opacity: 0.9, speed: 1800, ro: null, mw: -1, mh: -1, born: frame };
  if (typeof ResizeObserver !== 'undefined') {
    // fires once on observe too: only re-measure when the size really changed
    S.ro = new ResizeObserver(() => {
      const r = root.getBoundingClientRect();
      if (r.width !== S.mw || r.height !== S.mh) S.dirty = true;
    });
    S.ro.observe(root);
  }
  if (!io && typeof IntersectionObserver !== 'undefined') {
    io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        const s = byRoot.get(e.target as HTMLElement);
        if (!s) continue;
        s.visible = e.isIntersecting;
        if (!s.visible) s.ctx.clearRect(0, 0, s.canvas.width, s.canvas.height);
      }
    });
  }
  io?.observe(root);
  all.add(S);
  byRoot.set(root, S);
  // glyph widths depend on the font: re-rasterise once web fonts have loaded
  if (document.fonts && document.fonts.status !== 'loaded') document.fonts.ready.then(() => { S.dirty = true; });
  if (!t0) t0 = performance.now();
  if (!raf) raf = requestAnimationFrame(tick);
  return () => stop(S);
}

function stop(S: Sk) {
  if (!all.has(S)) return;
  all.delete(S);
  byRoot.delete(S.root);
  S.ro?.disconnect();
  io?.unobserve(S.root);
  S.canvas.remove();
}

/** Start the shimmer on every animated skeleton inside `scope` (for markup from `asciiSkeletonHTML`). */
export function animateSkeletons(scope: ParentNode = document): () => void {
  if (!isBrowser()) return () => {};
  const stops = Array.from(scope.querySelectorAll<HTMLElement>('[data-ars-animate]')).map(animateSkeleton);
  return () => stops.forEach((s) => s());
}
