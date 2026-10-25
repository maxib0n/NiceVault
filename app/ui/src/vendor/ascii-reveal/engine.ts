import { resolvePreset, type Preset, type PresetName } from './presets';
import { fade } from '../../lib/motion';
import { clamp, dpr as getDpr, ease, easeIO, hash, isBrowser, lineAdvances, parseColor, reducedMotion } from './util';
import { imageMascot, mascots, type MascotDraw, type MascotName } from './mascots';

/* ------------------------------------------------------------------ */
/* Public types                                                        */
/* ------------------------------------------------------------------ */

export type Origin = 'center' | 'top-left' | 'pointer' | { x: number; y: number };
/** `fast` (default) keeps data readable within ~400 ms; `cinematic` is the slow showcase wave. */
export type Duration = 'fast' | 'cinematic';

export interface AsciiRevealOptions {
  /** Built-in preset name or a custom preset object. Default `decode`. */
  preset?: PresetName | Preset;
  /** Playback speed multiplier. 2 is twice as fast. Default 1. */
  speed?: number;
  /** `fast` (default) for data, `cinematic` for heroes and demos. */
  duration?: Duration;
  /** Where the reveal wave starts. Default `center` of the element. */
  origin?: Origin;
  /** Only animate content inside elements matching this selector. */
  selector?: string;
  /** Elements that get the dot-field and corner frame before fading in. Default `[data-ascii="surface"]`. */
  surfaces?: string;
  /** Elements that simply fade in on the wave (buttons, chips). Default `[data-ascii="fade"]`. */
  fades?: string;
  /** Elements rendered through the glyph grid. Default `img, canvas, video, [data-ascii="media"]`. */
  media?: string;
  /** Element the effect draws into. Created automatically when omitted. */
  overlay?: HTMLElement;
  /**
   * Show an animated ASCII mascot while loading instead of the plain shimmer:
   * `spider`, `ghost`, `pumpkin`, `bat`, an SVG URL, SVG markup, or your own draw function.
   */
  mascot?: MascotName | string | MascotDraw | null;
  /**
   * Reveal sections below the fold as they scroll into view instead of all at once.
   * Applies to full reveals (no `within`). Default true.
   */
  lazy?: boolean;
  /** If the device can't keep up (frames over ~45 ms), finish instantly instead of stuttering. Default true. */
  fallback?: boolean;
  /**
   * (NiceVault) Selector of the repeated item roots in the element (list rows, grid tiles). When set,
   * only the items on screen when a run starts are held, walked, measured and animated (all of them:
   * `cap` is only a safety bound); the others are never touched (they show as they are, and so do items that arrive later).
   * Items are expected in layout order, top to bottom (found by bisection, not by measuring them all).
   */
  items?: string;
  /** (NiceVault) Most items one run animates (with `items`). Default 1000 (a bound, not a budget). */
  cap?: number;
}

export interface RunOptions {
  origin?: Origin;
  /** Limit this run to a part of the tree, for example one card. */
  within?: Element;
  speed?: number;
  duration?: Duration;
  preset?: PresetName | Preset;
  /** Hide only: skip the animation and hide immediately. */
  instant?: boolean;
  /** (NiceVault) called once, on the first frame this run draws (or at once when it doesn't animate). */
  onStart?: () => void;
  /** (NiceVault) Exactly these elements, as media (pictures), instead of walking a scope. */
  only?: Element[];
}

export interface AsciiRevealController {
  /** Decode the content into place. Resolves when every target has landed. */
  reveal(options?: RunOptions): Promise<void>;
  /** Scramble the content back out. It stays hidden until the next `reveal()`. */
  hide(options?: RunOptions): Promise<void>;
  /** Show a glyph shimmer (or the mascot) where hidden content was, for loading states. */
  shimmer(on: boolean): void;
  update(options: Partial<AsciiRevealOptions>): void;
  destroy(): void;
}

/* ------------------------------------------------------------------ */
/* Timing                                                              */
/* ------------------------------------------------------------------ */

interface Timing {
  vel: number; // wave speed, px per ms
  maxWave: number; // cap on how long the wave may take to reach a target
  spread: (n: number) => number; // how long a text node takes to start all its characters
  char: [number, number]; // per-character scramble duration: base + random range
  media: number;
  surface: number;
  fade: number;
  onSurface: number; // content on a surface starts this much later
}
const TIMING: Record<Duration, Timing> = {
  fast: { vel: 3.2, maxWave: 220, spread: (n) => Math.min(200, 40 + n * 4), char: [90, 110], media: 450, surface: 300, fade: 220, onSurface: 60 },
  cinematic: { vel: 1.15, maxWave: Infinity, spread: (n) => Math.min(720, 90 + n * 13), char: [200, 240], media: 1150, surface: 720, fade: 420, onSurface: 140 },
};

/* ------------------------------------------------------------------ */
/* Global stylesheet, pointer tracking, scheduler                      */
/* ------------------------------------------------------------------ */

const T = 'data-ar-t'; // text parent hidden
const M = 'data-ar-m'; // media hidden / fading
const TC = 'data-ar-tc'; // gradient text being hidden
const S = 'data-ar-s'; // surface fading
const F = 'data-ar-f'; // fade element
const DEFAULT_FONT = 'ui-monospace, "SF Mono", "Geist Mono", "JetBrains Mono", Menlo, Consolas, monospace';

let styled = false;
function ensureStyles() {
  if (styled || !isBrowser()) return;
  styled = true;
  const el = document.createElement('style');
  el.setAttribute('data-ascii-reveal', '');
  // text is hidden with text-fill-color, not color, so icons drawn with currentColor stay visible
  el.textContent = `
[${T}]{-webkit-text-fill-color:transparent!important;text-shadow:none!important;text-decoration-color:transparent!important}
[${TC}]{background-image:none!important}
[${M}],[${S}],[${F}]{opacity:var(--ar-o,0)!important}
[data-ar-pending]{visibility:hidden}
.ar-overlay{position:absolute;inset:0;pointer-events:none;z-index:2;overflow:visible}
.ar-overlay>canvas{position:absolute;pointer-events:none;display:block}`;
  document.head.appendChild(el);
}

let pointer: { x: number; y: number } | null = null;
let tracking = false;
function trackPointer() {
  if (tracking || !isBrowser()) return;
  tracking = true;
  document.addEventListener('pointermove', (e) => (pointer = { x: e.clientX, y: e.clientY }), { passive: true });
  document.addEventListener('pointerdown', (e) => (pointer = { x: e.clientX, y: e.clientY }), { passive: true });
}

interface Target {
  key: object;
  holdEl: Element;
  holdName: string;
  cx: number;
  cy: number;
  offset: number;
  dur: number;
  text?: boolean;
  insideSurface?: boolean;
  init(dir: 1 | -1): void;
  prepare?(): void;
  ready?(): Promise<void> | undefined;
  /** `paint` is true on frames where the shared canvas was cleared and must be redrawn (~30 fps). */
  render(lt: number, dir: 1 | -1, paint: boolean): void;
  dispose(): void;
  /** (NiceVault) its hold already let go (a cross-fade handed it over before the run ended) */
  early?: boolean;
  /** (NiceVault) read the source again (a picture arrived in it mid-run) */
  resample?(): void;
  /** (NiceVault) prepare() has run (lazily, on the target's first frame) */
  prepared?: boolean;
  /** (NiceVault) its source changed after prepare(): prepared again on a frame with budget left */
  stale?: boolean;
  rect?: { x: number; y: number; w: number; h: number };
}

/** Two canvases per run: `live` is redrawn each frame, `bake` receives text once it has landed. */
interface Layer {
  live: HTMLCanvasElement;
  lctx: CanvasRenderingContext2D;
  bake: HTMLCanvasElement;
  bctx: CanvasRenderingContext2D;
  x: number; // overlay coords of the layer's top-left (css px)
  y: number;
  w: number;
  h: number;
  dpr: number;
}

interface Run {
  ctrl: Controller;
  dir: 1 | -1;
  start: number;
  targets: Target[];
  byKey: Map<object, Target>;
  baked: Set<Target>;
  layer: Layer | null;
  lastPaint: number;
  done: Promise<void>;
  resolve: () => void;
  live: boolean;
  onStart?: () => void;
  /** (NiceVault) bookkeeping for window.__nvRevealRuns (frame-capture checks, cost) */
  stat: RunStat;
}

/** (NiceVault) One run, as window.__nvRevealRuns logs it when it ends. */
export interface RunStat {
  t: number;
  dir: 1 | -1;
  root: string;
  targets: number;
  items: number;
  /** the layout pending when it started (the page's, not the decode's), ms */
  layout: number;
  /** of the setup: collecting and measuring the targets (of which media pieces, text), ms */
  collect: number;
  media: number;
  text: number;
  /** of the setup: holding the targets and starting them; letting go of the hidden marks, ms */
  hold: number;
  clear: number;
  /** of `hold`: reading what the holds need; creating the run's canvases */
  reads: number;
  canvas: number;
  /** synchronous setup (collect + measure + init, then prepare: pictures and ink maps sampled), ms */
  setup: number;
  prepare: number;
  /** waited for pictures to decode and icons to rasterise before the first frame, ms */
  ready: number;
  /** when the last target lands, ms after the first frame */
  end: number;
  /** ms after the first frame when it ended, and why */
  at: number;
  why: 'natural' | 'watchdog' | 'resize' | 'destroy';
  frames: number;
  /** the engine's own time per frame for this run: mean, max (ms) */
  tickMean: number;
  tickMax: number;
  tickSum: number;
  /** of which drawing text / media (ms, summed over the run) */
  textMs: number;
  mediaMs: number;
}
function logStat(run: Run, why: RunStat['why']) {
  const st = run.stat;
  st.why = why;
  st.at = run.start >= 0 && run.start !== Infinity ? Math.round(performance.now() - run.start) : 0;
  st.tickMean = st.frames ? +(st.tickSum / st.frames).toFixed(2) : 0;
  st.tickMax = +st.tickMax.toFixed(2);
  st.setup = +st.setup.toFixed(2);
  st.collect = +st.collect.toFixed(1);
  st.media = +st.media.toFixed(1);
  st.text = +st.text.toFixed(1);
  st.hold = +st.hold.toFixed(1);
  st.clear = +st.clear.toFixed(1);
  st.reads = +st.reads.toFixed(1);
  st.canvas = +st.canvas.toFixed(1);
  st.prepare = +st.prepare.toFixed(2);
  st.textMs = Math.round(st.textMs);
  st.mediaMs = Math.round(st.mediaMs);
  st.tickSum = Math.round(st.tickSum);
  const w = window as unknown as { __nvRevealRuns?: RunStat[] };
  const log = (w.__nvRevealRuns ??= []);
  log.push({ ...st });
  if (log.length > 100) log.splice(0, log.length - 100);
}

interface Loop {
  render(now: number): boolean; // return false to stop
}

const runs = new Set<Run>();
const loops = new Set<Loop>();
const owner = new WeakMap<object, Run>();
const holds = new WeakMap<Element, Map<string, number>>();
let raf = 0;
// frame-budget watchdog
let lastTick = 0;
let slowFrames = 0;
/** what the last frame's tick itself took (ms) */
let lastCost = 0;
let degradedUntil = 0; // after the watchdog fires, reveals fade instead of decoding for a while
const isDegraded = () => performance.now() < degradedUntil;
/** Run.start before its first frame (NiceVault). */
const PENDING = -1;

function hold(el: Element, name: string) {
  let m = holds.get(el);
  if (!m) holds.set(el, (m = new Map()));
  const n = (m.get(name) || 0) + 1;
  m.set(name, n);
  if (n === 1) {
    // gradient text (background-clip: text) paints through a transparent fill, so hide its background too
    const gt = name === T && clipsText(el);
    el.setAttribute(name, '');
    if (gt) el.setAttribute(TC, '');
  }
}
/** (NiceVault) Whether `el` paints its text through its background (read once per element). */
const textClips = new WeakMap<Element, boolean>();
function clipsText(el: Element): boolean {
  let v = textClips.get(el);
  if (v === undefined) {
    const cs = getComputedStyle(el);
    v = (cs.backgroundClip || (cs as unknown as Record<string, string>).webkitBackgroundClip || '').includes('text');
    textClips.set(el, v);
  }
  return v;
}
/**
 * (NiceVault) Reads what holding these will need before any of them is held: one style
 * computation for all, instead of one forced by every hold that follows another's write.
 */
function readHolds(list: { holdEl: Element; holdName: string }[]) {
  for (const t of list) if (t.holdName === T && !holds.get(t.holdEl)?.get(T)) clipsText(t.holdEl);
}
function release(el: Element, name: string) {
  const m = holds.get(el);
  if (!m) return;
  const n = (m.get(name) || 0) - 1;
  if (n > 0) return void m.set(name, n);
  m.delete(name);
  el.removeAttribute(name);
  if (name === T) el.removeAttribute(TC);
  if (name !== T && !el.hasAttribute(M) && !el.hasAttribute(S) && !el.hasAttribute(F)) {
    (el as HTMLElement).style.removeProperty('--ar-o');
  }
}

function kick() {
  if (!raf) {
    lastTick = 0;
    raf = requestAnimationFrame(tick);
  }
}

function tick() {
  const now = performance.now();
  spriteSpent = 0;
  // watchdog: if frames keep blowing the budget while runs are animating, finish them instead of stuttering
  // (NiceVault: a single long stall — the window not yet painted, a busy first frame — is not
  // stutter and does not count; it needs 12 slow frames in a row, and degrades for 3 s only)
  if (lastTick && runs.size) {
    const dt = now - lastTick;
    // (NiceVault) only frames the decode itself made slow count: a loaded machine or a heavy page
    // (a mount, a compositor stall) is not helped by cutting the decode short — that only shows it
    // ending before its end
    const ours = lastCost > 8;
    slowFrames = dt > 45 && dt < 250 && ours ? slowFrames + 1 : dt >= 250 ? slowFrames : Math.max(0, slowFrames - 2);
    if (slowFrames >= 12) {
      let fired = false;
      for (const run of Array.from(runs)) if (run.ctrl.opts.fallback !== false) { finishRun(run, 'watchdog'); fired = true; }
      if (fired) {
        degradedUntil = performance.now() + 3_000;
        // (NiceVault) counted for the frame-capture checks (e2e/real/reveal.mjs)
        const w = window as unknown as { __nvRevealDegraded?: number };
        w.__nvRevealDegraded = (w.__nvRevealDegraded ?? 0) + 1;
      }
      slowFrames = 0;
    }
  }
  lastTick = now;
  for (const run of Array.from(runs)) {
    if (!run.live) continue;
    // (NiceVault: the clock starts at the first frame actually drawn, so a run started while the
    // window was not painting yet — app start, a webview still mapping — is seen, not skipped)
    if (run.start === PENDING) {
      run.start = now;
      const f = run.onStart;
      run.onStart = undefined;
      f?.();
    }
    const local = now - run.start;
    if (local < 0) continue;
    const tk0 = performance.now();
    const L = run.layer;
    // the shared canvas is repainted at ~30 fps (glyphs only change every 60-90 ms); opacities update every frame
    const paint = now - run.lastPaint >= 30;
    if (paint) {
      run.lastPaint = now;
      if (L) L.lctx.setTransform(1, 0, 0, 1, 0, 0), forget(L.lctx), L.lctx.clearRect(0, 0, L.live.width, L.live.height);
    }
    let finished = true;
    for (const t of run.targets) {
      if (owner.get(t.key) !== run || run.baked.has(t)) continue;
      const tl = local - t.offset;
      if (tl < 0) {
        finished = false;
        // a hide that hasn't reached this text yet still shows it in full
        if (run.dir < 0 && t.text) t.render(t.dur, run.dir, paint);
        continue;
      }
      if (t.prepare && (!t.prepared || t.stale) && !(t.stale && spriteSpent > SPRITE_BUDGET)) {
        // over this frame's budget: it starts on the next frame (it is held meanwhile); a stale
        // one (a picture arrived in it) goes on with what it has
        if (!t.prepared && spriteSpent > SPRITE_BUDGET && tl < t.dur * 0.5) {
          finished = false;
          continue;
        }
        const p0 = performance.now();
        t.prepare();
        t.prepared = true;
        t.stale = false;
        const pd = performance.now() - p0;
        spriteSpent += pd;
        run.stat.prepare += pd;
      }
      const lt = run.dir > 0 ? Math.min(tl, t.dur) : Math.max(0, t.dur - tl);
      const r0 = performance.now();
      t.render(lt, run.dir, paint);
      const rd = performance.now() - r0;
      if (t.text) run.stat.textMs += rd;
      else if (t.holdName === M) run.stat.mediaMs += rd;
      if (tl < t.dur) finished = false;
      else if (run.dir > 0 && t.text && L) {
        // landed: draw it once onto the bake layer and stop redrawing it every frame
        run.baked.add(t);
        (t as TextTarget).land(t);
      } else if (run.dir > 0) run.baked.add(t); // landed: nothing more to draw for it
    }
    const tk = performance.now() - tk0;
    run.stat.frames++;
    run.stat.tickSum += tk;
    if (tk > run.stat.tickMax) run.stat.tickMax = tk;
    if (finished) finishRun(run, 'natural');
  }
  for (const loop of Array.from(loops)) if (!loop.render(now)) loops.delete(loop);
  lastCost = performance.now() - now;
  raf = runs.size || loops.size ? requestAnimationFrame(tick) : 0;
}

function finishRun(run: Run, why: RunStat['why']) {
  runs.delete(run);
  logStat(run, why);
  for (const t of run.targets) {
    if (owner.get(t.key) !== run) continue;
    owner.delete(t.key);
    if (run.dir > 0) t.render(t.dur, 1, false);
    t.dispose();
    if (run.dir > 0) {
      letGo(t);
      if (t.holdName === M) shownPieces.add(t.holdEl);
    } else run.ctrl.markHidden(t.holdEl, t.holdName);
  }
  run.layer?.live.remove();
  run.layer?.bake.remove();
  run.ctrl.active(-1);
  run.resolve();
}

/** A target's hold lets go (unless a cross-fade already let it go). */
function letGo(t: Target) {
  if (!t.early) release(t.holdEl, t.holdName);
  t.early = false;
}

/** (NiceVault) Pieces a forward run has shown (decoded, or landed by the watchdog): their one showing. */
const shownPieces = new WeakSet<Element>();
/** (NiceVault) `el`, or the piece it sits in (up to `root`), was already shown by a decode. */
export function pieceShown(el: Element, root?: Element): boolean {
  for (let n: Element | null = el; n && n !== root; n = n.parentElement) if (shownPieces.has(n)) return true;
  return false;
}
/**
 * (NiceVault) A picture that arrived inside a piece a run is holding: 'live' when that run is
 * animating it (its glyphs are drawn from the piece again, now with the picture: it takes over
 * mid-decode), 'held' when the piece is held for a run not started yet (it will be read then),
 * null when no decode holds it.
 */
export function adoptPicture(el: Element): 'live' | 'held' | null {
  for (let n = el.parentElement, i = 0; n && i < 8; n = n.parentElement, i++) {
    const run = owner.get(n);
    const t = run?.byKey.get(n);
    if (run && t && t.holdName === M && run.dir > 0) {
      t.resample?.();
      return 'live';
    }
    if (n.hasAttribute(M)) return 'held';
  }
  return null;
}

function claim(run: Run, t: Target) {
  const prev = owner.get(t.key);
  if (prev && prev !== run) {
    const pt = prev.byKey.get(t.key);
    if (pt) {
      pt.dispose();
      letGo(pt);
      prev.baked.delete(pt);
    }
  }
  owner.set(t.key, run);
  hold(t.holdEl, t.holdName);
}

/* ------------------------------------------------------------------ */
/* Environment: colors and fonts read once per run                     */
/* ------------------------------------------------------------------ */

interface Env {
  preset: Preset;
  timing: Timing;
  scale: number;
  ox: number; // overlay client left
  oy: number;
  ow: number;
  oh: number;
  /** region the text layer covers, in overlay coords (visible part of the element, plus a margin) */
  region: { x: number; y: number; w: number; h: number };
  overlay: HTMLElement;
  root: Element;
  font: string;
  accent: string | null;
  flash: [number, number, number, number] | null;
  glyph: string | null;
  dpr: number;
  origin: { x: number; y: number };
  styles: Map<Element, CSSStyleDeclaration>;
  clips: Map<Element, { x0: number; y0: number; x1: number; y1: number } | null>;
  /** (NiceVault) backgrounds found above elements, ink maps and sprites shared by identical pieces */
  grounds: Map<Element, [number, number, number]>;
  cache: Map<string, unknown>;
}

function cssVar(el: Element, name: string) {
  return getComputedStyle(el).getPropertyValue(name).trim();
}

function backdropColor(env: Env, el: Element): [number, number, number] {
  const forced = parseColor(styleOf(env, el).getPropertyValue('--ascii-surface').trim());
  if (forced) return [forced[0], forced[1], forced[2]];
  return el.parentElement ? groundOf(env, el.parentElement) : [255, 255, 255];
}
/** The first opaque background at or above `el` (cached per run: siblings share their ancestors). */
function groundOf(env: Env, el: Element): [number, number, number] {
  let g = env.grounds.get(el);
  if (g) return g;
  const c = parseColor(styleOf(env, el).backgroundColor);
  g = c && c[3] >= 0.5 ? [c[0], c[1], c[2]] : el.parentElement ? groundOf(env, el.parentElement) : [255, 255, 255];
  env.grounds.set(el, g);
  return g;
}

const styleOf = (env: Env, el: Element) => {
  let cs = env.styles.get(el);
  if (!cs) env.styles.set(el, (cs = getComputedStyle(el)));
  return cs;
};

/** Nearest clipping ancestor inside the root (scroll containers, overflow:hidden), in overlay coords. */
type Clip = { x0: number; y0: number; x1: number; y1: number } | null;
function clipOf(env: Env, el: Element): Clip {
  if (env.clips.has(el)) return env.clips.get(el)!;
  let res: { x0: number; y0: number; x1: number; y1: number } | null = null;
  const p = el.parentElement;
  if (el !== env.root && p) {
    const cs = styleOf(env, el);
    const clipping = /(hidden|auto|scroll|clip)/.test(cs.overflowX + cs.overflowY);
    const inherited: Clip = env.root.contains(p) ? clipOf(env, p) : null;
    if (clipping) {
      const r = el.getBoundingClientRect();
      res = { x0: r.left - env.ox, y0: r.top - env.oy, x1: r.right - env.ox, y1: r.bottom - env.oy };
      if (inherited) res = { x0: Math.max(res.x0, inherited.x0), y0: Math.max(res.y0, inherited.y0), x1: Math.min(res.x1, inherited.x1), y1: Math.min(res.y1, inherited.y1) };
    } else res = inherited;
  }
  env.clips.set(el, res);
  return res;
}

/* ------------------------------------------------------------------ */
/* Text: measured once, drawn on the run's canvas layer                */
/* ------------------------------------------------------------------ */

const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEXTAREA', 'INPUT', 'SELECT', 'OPTION', 'TEMPLATE']);

type Seg = { segment: string; index: number };
const segmenter: { segment(s: string): Iterable<Seg> } | null =
  typeof Intl !== 'undefined' && 'Segmenter' in Intl
    ? new (Intl as unknown as { Segmenter: new (l?: string, o?: object) => { segment(s: string): Iterable<Seg> } }).Segmenter(undefined, { granularity: 'grapheme' })
    : null;
function graphemes(s: string): Seg[] {
  // (NiceVault) text where every code unit is a grapheme (Latin, no combining marks or emoji)
  // needs no segmenter: most names in a folder
  if (segmenter && !/^[\u0020-\u02ff]*$/.test(s)) return Array.from(segmenter.segment(s));
  const out: Seg[] = [];
  let i = 0;
  for (const ch of s) { out.push({ segment: ch, index: i }); i += ch.length; }
  return out;
}

function transformText(s: string, tt: string, first: boolean) {
  if (tt === 'uppercase') return s.toUpperCase();
  if (tt === 'lowercase') return s.toLowerCase();
  if (tt === 'capitalize' && first) return s.toUpperCase();
  return s;
}

interface TextTarget extends Target {
  /** its last character has landed: the text itself takes over */
  land(t: Target): void;
}

function textTarget(node: Text, env: Env, seed: number, layer: () => Layer): TextTarget | null {
  const parent = node.parentElement;
  if (!parent) return null;
  const cs = styleOf(env, parent);
  if (cs.visibility === 'hidden' || cs.display === 'none') return null;
  const str = node.nodeValue || '';
  const range = document.createRange();
  range.selectNodeContents(node);
  const br = range.getBoundingClientRect();
  const R = env.region;
  const bx = br.left - env.ox, by = br.top - env.oy;
  // skip text that isn't anywhere near the screen: it is left alone, not hidden
  if (bx > R.x + R.w || by > R.y + R.h || bx + br.width < R.x || by + br.height < R.y) return null;
  const clip = clipOf(env, parent);

  const xs: number[] = [], ys: number[] = [], ws: number[] = [], hs: number[] = [], chs: string[] = [];
  const gis: number[] = []; // (NiceVault) each character's index on its line (one-line placement)
  let prevSpace = true;
  const realFont = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  const segs = graphemes(str);
  // (NiceVault) a node on one line: its characters placed from the font's advances (cached per
  // font and character), scaled to the node's measured width — one layout read for the node, not
  // one per character. Wrapped lines, and nodes the advances don't account for, are measured per character.
  const lines = range.getClientRects();
  let placed = false;
  if (lines.length === 1 && br.width > 0 && !/^(justify)/.test(cs.textAlign) && cs.wordSpacing.startsWith('0')) {
    const ls = parseFloat(cs.letterSpacing) || 0;
    const pre = cs.whiteSpace.startsWith('pre') || cs.whiteSpace === 'break-spaces';
    const adv = lineAdvances(segs.map((g) => g.segment), (ch, first) => advanceOf(realFont, ch === ' ' ? ' ' : transformText(ch, cs.textTransform, first)), ls, pre);
    const total = adv.reduce((a, b) => a + b, 0);
    const k = total > 0 ? br.width / total : 0;
    if (k > 0.9 && k < 1.1) {
      placed = true;
      let cx = br.left;
      prevSpace = true;
      segs.forEach(({ segment }, i) => {
        const w = adv[i] * k;
        const space = /^\s+$/.test(segment);
        if (!space) {
          const x = cx - env.ox, y = br.top - env.oy;
          const inRegion = x + w > R.x && y + br.height > R.y && x < R.x + R.w && y < R.y + R.h;
          const inClip = !clip || (x + w > clip.x0 && x < clip.x1 && y + br.height > clip.y0 && y < clip.y1);
          if (inRegion && inClip) {
            xs.push(x); ys.push(y); ws.push(w - ls * k); hs.push(br.height);
            chs.push(transformText(segment, cs.textTransform, prevSpace));
            gis.push(i);
          }
        }
        prevSpace = space;
        cx += w;
      });
    }
  }
  if (!placed) for (const { segment, index } of segs) {
    const space = /^\s+$/.test(segment);
    if (!space) {
      range.setStart(node, index);
      range.setEnd(node, index + segment.length);
      const r = range.getBoundingClientRect();
      if (r.width || r.height) {
        const x = r.left - env.ox, y = r.top - env.oy;
        const inRegion = x + r.width > R.x && y + r.height > R.y && x < R.x + R.w && y < R.y + R.h;
        const inClip = !clip || (x + r.width > clip.x0 && x < clip.x1 && y + r.height > clip.y0 && y < clip.y1);
        if (inRegion && inClip) {
          xs.push(x); ys.push(y); ws.push(r.width); hs.push(r.height);
          chs.push(transformText(segment, cs.textTransform, prevSpace));
        }
      }
    }
    prevSpace = space;
  }
  range.detach?.();
  const n = chs.length;
  if (!n) return null;

  const p = env.preset;
  const tm = env.timing;
  const sc = env.scale;
  const spread = tm.spread(n) * sc;
  const s0 = new Float32Array(n), s1 = new Float32Array(n), ph = new Float32Array(n);
  let dur = 1;
  for (let k = 0; k < n; k++) {
    s0[k] = (k / n) * spread * 0.72 + hash(k, seed, 1) * spread * 0.28;
    s1[k] = s0[k] + (tm.char[0] + hash(k, seed, 2) * tm.char[1]) * sc;
    ph[k] = hash(k, seed, 3) * 1000;
    if (s1[k] > dur) dur = s1[k];
  }
  const digits = !!parent.closest('[data-ascii-digits]') || /^[\d.,%:+\-\s€$£¥]+$/.test(str.trim());
  const set = digits ? '0123456789' : p.scramble;
  let color = cs.color;
  const pc = parseColor(color);
  if (!pc || pc[3] === 0) color = getComputedStyle(env.root).color; // gradient / transparent text
  const glyphFont = `400 ${cs.fontSize} ${env.font}`;
  // (NiceVault) a node on one line scrambles as one string: its glyphs in their slots (a space
  // where a character is not scrambling), stretched to the line's pitch — one fillText per node
  // and frame, not one per character (WebKit: 300 strings of 10 cost 1 ms, 3,000 glyphs 13 ms). The monospace glyphs sit within a fraction of a slot of the
  // characters they stand for; nodes they would not fit are drawn glyph by glyph.
  let slot: Int32Array | null = null, slots = 0, pitch = 0, mono = 0, x0 = 0;
  if (placed && gis.length === n && n >= 2) {
    const c0 = xs[0] + ws[0] / 2, g0 = gis[0], span = gis[n - 1] - g0;
    pitch = span > 0 ? (xs[n - 1] + ws[n - 1] / 2 - c0) / span : 0;
    mono = advanceOf(glyphFont, '0');
    let fits = pitch > 0 && mono > 0 && pitch / mono > 0.7 && pitch / mono < 1.4;
    for (let k = 0; fits && k < n; k++) fits = Math.abs(xs[k] + ws[k] / 2 - (c0 + (gis[k] - g0) * pitch)) < 0.4 * pitch;
    if (fits) {
      slot = Int32Array.from(gis, (g) => g - g0);
      slots = span + 1;
      x0 = c0 - pitch / 2;
    }
  }
  const buf: string[] = slot ? new Array(slots).fill(' ') : [];
  const stretch = mono > 0 ? pitch / mono : 1;
  const glyphColor = p.accentGlyphs && env.accent ? env.accent : env.glyph || color;
  const glyphAlpha = p.accentGlyphs && env.accent ? 0.9 : env.glyph ? 1 : 0.5;
  const rgb = parseColor(color) || [128, 128, 128, 1];
  const flash = env.flash;
  const flashMs = 420;
  let baseline = 0; // offset from a box's top to the alphabetic baseline, set on init

  let s0min = Infinity;
  for (let k = 0; k < n; k++) if (s0[k] < s0min) s0min = s0[k];
  const draw = (ctx: CanvasRenderingContext2D, lt: number, dir: 1 | -1, L: Layer, landed = true) => {
    // (NiceVault) nothing scrambling yet, or any more: no state to set up for it
    if (!landed && (lt < s0min || lt >= dur)) return;
    const d = L.dpr;
    // (NiceVault) the state it draws with is set only where it differs from what the canvas has
    // (the texts of a folder share it: a few setters per frame, not a few per text); a
    // save/restore only to undo a clip
    if (clip) {
      ctx.save();
      forget(ctx);
    }
    putTransform(ctx, d, -L.x * d, -L.y * d);
    if (clip) {
      ctx.beginPath();
      ctx.rect(clip.x0, clip.y0, clip.x1 - clip.x0, clip.y1 - clip.y0);
      ctx.clip();
    }
    // pass 1: scrambling glyphs
    let anyReal = false, inBuf = false, set1 = false;
    const pass1 = () => {
      set1 = true;
      put(ctx, 'font', glyphFont);
      put(ctx, 'textAlign', 'center');
      put(ctx, 'textBaseline', 'middle');
      put(ctx, 'fillStyle', glyphColor);
      put(ctx, 'globalAlpha', glyphAlpha);
    };
    for (let k = 0; k < n; k++) {
      if (lt < s0[k]) continue;
      if (lt >= s1[k]) { anyReal = true; continue; }
      const q = (lt - s0[k]) / (s1[k] - s0[k]);
      const t = Math.floor((lt + ph[k]) / p.tick);
      let g: string;
      if (p.cursor && q < 0.2) {
        if (!set1) pass1();
        put(ctx, 'fillStyle', env.accent || color);
        put(ctx, 'globalAlpha', 1);
        ctx.fillText(p.cursor, xs[k] + ws[k] / 2, ys[k] + hs[k] * 0.48);
        put(ctx, 'fillStyle', glyphColor);
        put(ctx, 'globalAlpha', glyphAlpha);
        continue;
      } else if (p.mode === 'ramp' && !digits && q < 0.7) {
        const Lr = p.ramp.length;
        let idx = 1 + Math.floor((q / 0.7) * (Lr - 1));
        if (hash(k, t, 5) < 0.35) idx += hash(k, t, 6) < 0.5 ? -1 : 1;
        g = p.ramp[clamp(idx, 1, Lr - 1)];
      } else g = set[Math.floor(hash(k, t, 7) * set.length)];
      if (slot) (buf[slot[k]] = g), (inBuf = true);
      else {
        if (!set1) pass1();
        ctx.fillText(g, xs[k] + ws[k] / 2, ys[k] + hs[k] * 0.48);
      }
    }
    if (inBuf) {
      if (!set1) pass1();
      put(ctx, 'textAlign', 'left');
      // the monospace advance stretched to the line's pitch (a transform: a letter spacing would
      // cost a font change per node)
      ctx.setTransform(d * stretch, 0, 0, d, (x0 - L.x) * d, -L.y * d);
      ctx.fillText(buf.join(''), 0, ys[0] + hs[0] * 0.48);
      ctx.setTransform(d, 0, 0, d, -L.x * d, -L.y * d);
      buf.fill(' ');
    }
    // pass 2: characters that have landed, in the element's own font
    if (anyReal && landed) {
      put(ctx, 'font', realFont);
      put(ctx, 'textAlign', 'left');
      put(ctx, 'textBaseline', 'alphabetic');
      put(ctx, 'globalAlpha', 1);
      put(ctx, 'fillStyle', color);
      for (let k = 0; k < n; k++) {
        if (lt < s1[k]) continue;
        if (flash && dir > 0) {
          const f = 1 - clamp((lt - s1[k]) / flashMs);
          if (f > 0) {
            const m = (a: number, b: number) => Math.round(b + (a - b) * f);
            put(ctx, 'fillStyle', `rgb(${m(flash[0], rgb[0])},${m(flash[1], rgb[1])},${m(flash[2], rgb[2])})`);
            ctx.fillText(chs[k], xs[k], ys[k] + baseline);
            put(ctx, 'fillStyle', color);
            continue;
          }
        }
        ctx.fillText(chs[k], xs[k], ys[k] + baseline);
      }
    }
    if (clip) {
      ctx.restore();
      forget(ctx);
    }
  };

  let L: Layer | null = null;
  const landedK = new Uint8Array(n);
  /** Characters landed since the last call, drawn onto the bake layer. */
  const bakeNew = (lt: number, L: Layer) => {
    let ctx: CanvasRenderingContext2D | null = null;
    for (let k = 0; k < n; k++) {
      if (landedK[k] || lt < s1[k]) continue;
      landedK[k] = 1;
      if (!ctx) {
        // (NiceVault) the state through put(): the texts of a run share it (no save/restore,
        // no font parsed again, unless there is a clip to undo)
        ctx = L.bctx;
        if (clip) ctx.save(), forget(ctx);
        const d = L.dpr;
        putTransform(ctx, d, -L.x * d, -L.y * d);
        if (clip) {
          ctx.beginPath();
          ctx.rect(clip.x0, clip.y0, clip.x1 - clip.x0, clip.y1 - clip.y0);
          ctx.clip();
        }
        put(ctx, 'font', realFont);
        put(ctx, 'textAlign', 'left');
        put(ctx, 'textBaseline', 'alphabetic');
        put(ctx, 'globalAlpha', 1);
        put(ctx, 'fillStyle', color);
      }
      ctx.fillText(chs[k], xs[k], ys[k] + baseline);
    }
    if (ctx && clip) ctx.restore(), forget(ctx);
  };
  return {
    key: node,
    holdEl: parent,
    holdName: T,
    text: true,
    cx: br.left + br.width / 2,
    cy: br.top + br.height / 2,
    offset: 0,
    dur,
    rect: { x: bx, y: by, w: br.width, h: br.height },
    init() {
      L = layer();
      // (NiceVault) the font's ascent and descent, measured once per font in a run
      let fm = env.cache.get('fm|' + realFont) as [number, number] | undefined;
      if (!fm) {
        // (on the small measuring canvas: the first use of the layer's allocates its pixels)
        const ctx = (advCtx ||= newCanvas(4, 4).getContext('2d') as CanvasRenderingContext2D | null) ?? L.lctx;
        ctx.font = advFont = realFont;
        const m = ctx.measureText('Hg');
        fm = [m.fontBoundingBoxAscent ?? m.actualBoundingBoxAscent, m.fontBoundingBoxDescent ?? m.actualBoundingBoxDescent];
        env.cache.set('fm|' + realFont, fm);
      }
      const [asc, desc] = fm;
      const h = hs[0];
      baseline = (h - (asc + desc)) / 2 + asc;
    },
    render(lt, dir, paint) {
      if (!L) return;
      if (dir > 0 && !flash) {
        // (NiceVault) a character that lands is drawn once, on the bake layer; frames redraw only
        // the glyphs still scrambling
        // (on the frames that repaint: the scrambling glyph it replaces goes on those)
        if (paint) bakeNew(lt, L), draw(L.lctx, lt, dir, L, false);
      } else if (paint) draw(L.lctx, lt, dir, L);
    },
    land(t) {
      if (!L) return;
      // (NiceVault) the text itself takes over as soon as its last character lands (the handoff is
      // spread over the run, never one restyle of everything at its end); its drawn copy goes
      release(parent, T);
      t.early = true;
      if (parent.hasAttribute(T)) {
        // another text of this element still decoding: this one stays drawn until then
        if (flash) draw(L.bctx, dur + 10_000, 1, L);
        else bakeNew(Infinity, L);
        return;
      }
      const ctx = L.bctx, d = L.dpr;
      putTransform(ctx, d, -L.x * d, -L.y * d);
      for (let k = 0; k < n; k++) if (landedK[k]) ctx.clearRect(xs[k] - 1, ys[k], ws[k] + 2, hs[k]);
    },
    dispose() {
      L = null;
    },
  };
}

/* ------------------------------------------------------------------ */
/* Media: glyph grids precomputed per size step                        */
/* ------------------------------------------------------------------ */

let sInk = 0, sR = 0, sG = 0, sB = 0;

interface GridStep {
  ch: number;
  n: number;
  x: Float32Array;
  y: Float32Array;
  gate: Float32Array;
  ink: Float32Array;
  color: string[] | null;
}

/* (NiceVault) What the glyph grid of a piece is drawn from, and how it is drawn cheaply.
 *
 * A piece that is not a picture (a file's sheet, an envelope, a print not yet developed) is drawn
 * roughly into a small "ink map": its boxes in their colours, the pictures and canvases in it, its
 * SVG icons drawn shape by shape (Path2D, synchronously). So its glyphs take its shape and converge on it, instead of
 * a neutral texture that has nothing to do with what appears when it hands over.
 *
 * The glyphs of a grid step are drawn once into a sprite (one fillText per row: a monospace font
 * sized so one advance is one cell) and the sprite is blitted every frame, clipped to the wave's
 * front: per piece and frame a few drawImage, whatever its size. Identical pieces (a folder of notes)
 * share their ink map and sprites. Sprites are built within a per-frame budget; a piece whose next
 * step is not ready keeps its current one for a frame. */

/** (NiceVault) One thing an ink map draws, read from the layout beforehand (see inkShot). */
type InkOp =
  | { box: [number, number, number, number, number]; fill: string }
  | { img: HTMLImageElement | HTMLCanvasElement; at: [number, number, number, number]; fit: string }
  | { path: Path2D; m: [number, number, number, number, number, number]; fill: string | null; stroke: string | null; fa: number; sa: number; lw: number; cap: string; join: string; rule: CanvasFillRule };

const num = (v: string, d: number) => (Number.isFinite(parseFloat(v)) ? parseFloat(v) : d);

/** (NiceVault) The shapes of `svg` as ink ops (Path2D: no SVG image to decode, which in WebKit
 *  builds a whole SVG document on the main thread), each in its resolved paint, placed by its
 *  screen CTM. Gradients stand as their most opaque stop; text, images and filters are left out. */
function svgOps(out: InkOp[], svg: SVGSVGElement, rc: DOMRect, k: number) {
  const shapes = svg.querySelectorAll('path, circle, ellipse, rect, line, polyline, polygon');
  const paint = (v: string, cs: CSSStyleDeclaration): string | null => {
    if (!v || v === 'none') return null;
    if (v.startsWith('url(')) {
      const id = /#([^)"']+)/.exec(v)?.[1];
      const g = id ? svg.querySelector(`[id="${id.replace(/"/g, '')}"]`) : null;
      let best: string | null = null, ba = -1;
      for (const st of Array.from(g?.querySelectorAll('stop') ?? [])) {
        const sc = getComputedStyle(st);
        const c = parseColor(sc.stopColor);
        const a = (c?.[3] ?? 0) * num(sc.stopOpacity, 1);
        if (c && a > ba) (ba = a), (best = `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`);
      }
      return best ?? cs.color;
    }
    return v;
  };
  for (let i = 0; i < shapes.length && i < 120; i++) {
    const e = shapes[i] as SVGGeometryElement;
    const cs = getComputedStyle(e);
    if (cs.display === 'none' || cs.visibility === 'hidden') continue;
    const m = e.getScreenCTM?.();
    if (!m) continue;
    let path: Path2D;
    try {
      if (e instanceof SVGPathElement) path = new Path2D(e.getAttribute('d') || '');
      else {
        path = new Path2D();
        if (e instanceof SVGCircleElement) path.arc(e.cx.baseVal.value, e.cy.baseVal.value, e.r.baseVal.value, 0, Math.PI * 2);
        else if (e instanceof SVGEllipseElement) path.ellipse(e.cx.baseVal.value, e.cy.baseVal.value, e.rx.baseVal.value, e.ry.baseVal.value, 0, 0, Math.PI * 2);
        else if (e instanceof SVGRectElement) path.rect(e.x.baseVal.value, e.y.baseVal.value, e.width.baseVal.value, e.height.baseVal.value);
        else if (e instanceof SVGLineElement) (path.moveTo(e.x1.baseVal.value, e.y1.baseVal.value), path.lineTo(e.x2.baseVal.value, e.y2.baseVal.value));
        else {
          const pts = (e.getAttribute('points') || '').trim().split(/[\s,]+/).map(Number);
          for (let j = 0; j + 1 < pts.length; j += 2) (j ? path.lineTo(pts[j], pts[j + 1]) : path.moveTo(pts[j], pts[j + 1]));
          if (e instanceof SVGPolygonElement) path.closePath();
        }
      }
    } catch {
      continue;
    }
    const op = num(cs.opacity, 1);
    out.push({
      path,
      m: [k * m.a, k * m.b, k * m.c, k * m.d, k * (m.e - rc.left), k * (m.f - rc.top)],
      fill: e instanceof SVGLineElement ? null : paint(cs.fill, cs),
      stroke: paint(cs.stroke, cs),
      fa: op * num(cs.fillOpacity, 1),
      sa: op * num(cs.strokeOpacity, 1),
      lw: num(cs.strokeWidth, 1),
      cap: cs.strokeLinecap,
      join: cs.strokeLinejoin,
      rule: cs.fillRule === 'evenodd' ? 'evenodd' : 'nonzero',
    });
  }
}

type Ctx2D = CanvasRenderingContext2D;

/** A scratch canvas. (A document canvas, not an OffscreenCanvas: text drawn or measured on one
 *  must resolve the page's web fonts, which WebKit's OffscreenCanvas does not do synchronously.) */
function newCanvas(w: number, h: number): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  return c;
}

function roundRectPath(ctx: Ctx2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  if (r > 0 && ctx.roundRect) ctx.roundRect(x, y, w, h, Math.min(r, w / 2, h / 2));
  else ctx.rect(x, y, w, h);
}

/**
 * The piece's ink map, part 1 (reads only, done while the run is set up and the layout is clean):
 * its boxes in their colours, its pictures and canvases, its SVG shapes, at `k` sample px per CSS px.
 */
function inkShot(el: HTMLElement, rc: DOMRect, k: number): InkOp[] {
  const out: InkOp[] = [];
  const list: Element[] = [el, ...Array.from(el.querySelectorAll('*')).slice(0, 80)];
  for (const e of list) {
    if (e instanceof SVGElement && !(e instanceof SVGSVGElement)) continue;
    const cs = getComputedStyle(e);
    if (cs.display === 'none' || cs.visibility === 'hidden') continue;
    const r = e.getBoundingClientRect();
    if (r.width < 0.5 || r.height < 0.5) continue;
    const x = (r.left - rc.left) * k, y = (r.top - rc.top) * k, w = r.width * k, h = r.height * k;
    let fill = parseColor(cs.backgroundColor);
    if ((!fill || fill[3] < 0.05) && cs.backgroundImage.includes('gradient')) {
      // a gradient: its most opaque colour stands for it
      fill = null;
      for (const m of cs.backgroundImage.match(/rgba?\([^)]*\)|color\([^)]*\)/g) ?? []) {
        const c = parseColor(m);
        if (c && (!fill || c[3] > fill[3])) fill = c;
      }
    }
    if (fill && fill[3] >= 0.05) out.push({ box: [x, y, w, h, (parseFloat(cs.borderTopLeftRadius) || 0) * k], fill: `rgba(${fill[0] | 0},${fill[1] | 0},${fill[2] | 0},${fill[3]})` });
    if (e instanceof HTMLImageElement || e instanceof HTMLCanvasElement) out.push({ img: e, at: [x, y, w, h], fit: cs.objectFit });
    else if (e instanceof SVGSVGElement) svgOps(out, e, rc, k);
  }
  return out;
}

/** The piece's ink map, part 2 (draws only: no layout read, so it can run inside a frame). */
function inkPaint(ops: InkOp[], SW: number, SH: number): Uint8ClampedArray | null {
  const off = newCanvas(SW, SH);
  const o = off.getContext('2d', { willReadFrequently: true }) as Ctx2D | null;
  if (!o) return null;
  for (const op of ops) {
    try {
      if ('box' in op) {
        const [x, y, w, h, r] = op.box;
        o.fillStyle = op.fill;
        roundRectPath(o, x, y, w, h, r);
        o.fill();
      } else if ('img' in op) {
        const e = op.img, [x, y, w, h] = op.at;
        if (e instanceof HTMLImageElement) {
          if (!e.complete || !e.naturalWidth) continue;
          const iw = e.naturalWidth, ih = e.naturalHeight;
          if (op.fit === 'cover' || op.fit === 'contain') {
            const s = op.fit === 'cover' ? Math.max(w / iw, h / ih) : Math.min(w / iw, h / ih);
            o.save();
            o.beginPath();
            o.rect(x, y, w, h);
            o.clip();
            o.drawImage(e, x + (w - iw * s) / 2, y + (h - ih * s) / 2, iw * s, ih * s);
            o.restore();
          } else o.drawImage(e, x, y, w, h);
        } else if (e.width && e.height) o.drawImage(e, x, y, w, h);
      } else {
        o.save();
        o.setTransform(...op.m);
        if (op.fill) {
          o.fillStyle = op.fill;
          o.globalAlpha = op.fa;
          o.fill(op.path, op.rule);
        }
        if (op.stroke) {
          o.strokeStyle = op.stroke;
          o.globalAlpha = op.sa;
          o.lineWidth = op.lw;
          o.lineCap = op.cap as CanvasLineCap;
          o.lineJoin = op.join as CanvasLineJoin;
          o.stroke(op.path);
        }
        o.restore();
      }
    } catch {
      /* not drawable (cross-origin, broken): its box stands for it */
    }
  }
  try {
    return o.getImageData(0, 0, SW, SH).data;
  } catch {
    return null;
  }
}

/** (NiceVault) The drawing state last set on a canvas through put(): a setter runs only when the
 *  value changes (canvas setters parse their value every time). Code that changes the state
 *  otherwise brackets it in save/restore, or calls forget(). */
const canvasState = new WeakMap<object, Record<string, unknown>>();
function put(ctx: CanvasRenderingContext2D, k: 'font' | 'textAlign' | 'textBaseline' | 'fillStyle' | 'globalAlpha', v: string | number) {
  let st = canvasState.get(ctx);
  if (!st) canvasState.set(ctx, (st = {}));
  if (st[k] === v) return;
  st[k] = v;
  (ctx as unknown as Record<string, unknown>)[k] = v;
}
function putTransform(ctx: CanvasRenderingContext2D, d: number, e: number, f: number) {
  let st = canvasState.get(ctx);
  if (!st) canvasState.set(ctx, (st = {}));
  if (st.d === d && st.e === e && st.f === f) return;
  st.d = d;
  st.e = e;
  st.f = f;
  ctx.setTransform(d, 0, 0, d, e, f);
}
function forget(ctx: CanvasRenderingContext2D) {
  canvasState.delete(ctx);
}


/** Width of one glyph advance of `font` at 100px (monospace: the same for every glyph). */
const advances = new Map<string, number>();
function advance100(font: string): number {
  let a = advances.get(font);
  if (a === undefined) {
    const c = newCanvas(4, 4).getContext('2d') as CanvasRenderingContext2D | null;
    a = 60;
    if (c) {
      c.font = `100px ${font}`;
      a = c.measureText('0000000000').width / 10 || 60;
    }
    advances.set(font, a);
  }
  return a;
}

/** Advance of one character in `font` (cached: a folder's names share their characters). */
const advCache = new Map<string, Map<string, number>>();
let advCtx: CanvasRenderingContext2D | null = null;
let advFont = '';
function advanceOf(font: string, ch: string): number {
  let m = advCache.get(font);
  if (!m) {
    if (advCache.size > 64) advCache.clear();
    advCache.set(font, (m = new Map()));
  }
  let a = m.get(ch);
  if (a === undefined) {
    advCtx ||= newCanvas(4, 4).getContext('2d') as CanvasRenderingContext2D | null;
    if (!advCtx) return 0;
    if (advFont !== font) advCtx.font = advFont = font;
    a = advCtx.measureText(ch).width;
    m.set(ch, a);
  }
  return a;
}

/** Sprites built in this frame so far (ms): the per-frame budget (see above). */
let spriteSpent = 0;
const SPRITE_BUDGET = 5;

function mediaTarget(el: HTMLElement, env: Env, layer: () => Layer): Target | null {
  const rc = el.getBoundingClientRect();
  if (rc.width < 2 || rc.height < 2) return null;
  const R = env.region;
  if (rc.left - env.ox > R.x + R.w || rc.top - env.oy > R.y + R.h || rc.right - env.ox < R.x || rc.bottom - env.oy < R.y) return null;
  const cs = styleOf(env, el);
  const dp = env.dpr;
  const W = Math.max(1, Math.round(rc.width * dp));
  const H = Math.max(1, Math.round(rc.height * dp));
  const x = rc.left - env.ox;
  const y = rc.top - env.oy;
  const p = env.preset;
  const dur = env.timing.media * env.scale;
  const cellScale = clamp(Math.min(rc.width, rc.height) / 230, 0.4, 1);
  const cells = p.cells.map((c) => Math.max(3, c * cellScale) * dp);
  const textColor = cs.color;
  const fixed = p.color === 'mono' ? textColor : p.color === 'accent' ? env.accent || textColor : null;
  const bg = backdropColor(env, el);
  const bgLum = (0.2126 * bg[0] + 0.7152 * bg[1] + 0.0722 * bg[2]) / 255;
  const ox = (env.origin.x - rc.left) * dp;
  const oy = (env.origin.y - rc.top) * dp;
  const maxD = Math.max(Math.hypot(ox, oy), Math.hypot(W - ox, oy), Math.hypot(ox, H - oy), Math.hypot(W - ox, H - oy)) || 1;
  const picture = el instanceof HTMLImageElement || el instanceof HTMLCanvasElement || el instanceof HTMLVideoElement;
  let L: Layer | null = null;
  let data: Uint8ClampedArray | null = null;
  // (NiceVault) the piece is sampled from a small copy (longest side <= SAMPLE px): the glyph cells
  // are 3+ device px apart, so a full-size read-back buys nothing but its cost
  const SAMPLE = 128;
  const ds = Math.min(1, SAMPLE / Math.max(W, H));
  const SW = Math.max(1, Math.round(W * ds)), SH = Math.max(1, Math.round(H * ds));
  let steps: (GridStep | null)[] = cells.map(() => null);
  const rr = cs.borderTopLeftRadius;
  const radius = Math.min((rr.endsWith('%') ? (parseFloat(rr) / 100) * Math.min(W, H) : (parseFloat(rr) || 0) * dp), Math.min(W, H) / 2);
  // what the ink map and the sprites are shared by: identical pieces of this run (same markup,
  // size, ground and pictures loaded)
  let key = '';
  // a piece drawn in tones close to its ground (an envelope on the board) is read with more
  // contrast: its strongest ink reaches the dense end of the ramp, as a picture's does
  let gain = 1;
  const inkGain = (d: Uint8ClampedArray | null) => {
    if (!d) return 1;
    let m = 0;
    for (let i = 0; i < d.length; i += 4) {
      const a = d[i + 3] / 255;
      const lum = (0.2126 * (d[i] * a + bg[0] * (1 - a)) + 0.7152 * (d[i + 1] * a + bg[1] * (1 - a)) + 0.0722 * (d[i + 2] * a + bg[2] * (1 - a))) / 255;
      const v = Math.abs(lum - bgLum);
      if (v > m) m = v;
    }
    m /= Math.max(bgLum, 1 - bgLum);
    return m > 0.02 ? clamp(0.9 / m, 1, 12) : 1;
  };
  const pieceKey = () => {
    let loaded = 0;
    for (const im of Array.from(el.getElementsByTagName('img'))) if (im.complete && im.naturalWidth) loaded++;
    return `${W}x${H}|${bg.join()}|${loaded}|${el.innerHTML}`;
  };
  // (NiceVault) what a piece's ink map is drawn from, read now, while the run is set up and the
  // layout is clean (prepare() runs inside frames, after their writes: a read there would force a
  // layout for every piece). Identical pieces share it.
  let shotKey = '';
  let shot: InkOp[] = [];
  const takeShot = () => {
    shotKey = pieceKey();
    const sk = 'shot|' + shotKey;
    const hit = env.cache.get(sk) as InkOp[] | undefined;
    if (hit) return void (shot = hit);
    shot = inkShot(el, el.getBoundingClientRect(), ds * dp);
    env.cache.set(sk, shot);
  };
  if (!picture) takeShot();

  const sample = (px: number, py: number) => {
    if (!data) {
      sInk = 0.22 + 0.5 * hash((px / (8 * dp)) | 0, (py / (8 * dp)) | 0, 41);
      sR = sG = sB = 128;
      return;
    }
    let ix = (px * ds) | 0, iy = (py * ds) | 0;
    if (ix < 0) ix = 0;
    if (iy < 0) iy = 0;
    if (ix >= SW) ix = SW - 1;
    if (iy >= SH) iy = SH - 1;
    const i = (iy * SW + ix) * 4;
    const a = data[i + 3] / 255;
    const r = data[i] * a + bg[0] * (1 - a);
    const g = data[i + 1] * a + bg[1] * (1 - a);
    const b = data[i + 2] * a + bg[2] * (1 - a);
    const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
    sInk = (gain * Math.abs(lum - bgLum)) / Math.max(bgLum, 1 - bgLum);
    sR = r | 0;
    sG = g | 0;
    sB = b | 0;
  };
  /** The best of five samples around a cell's centre (thin lines are not missed). */
  const best5 = (cx: number, cy: number, dx: number, dy: number) => {
    let best = -1, br = 0, bgc = 0, bb = 0;
    sample(cx, cy); if (sInk > best) { best = sInk; br = sR; bgc = sG; bb = sB; }
    sample(cx - dx, cy - dy); if (sInk > best) { best = sInk; br = sR; bgc = sG; bb = sB; }
    sample(cx + dx, cy - dy); if (sInk > best) { best = sInk; br = sR; bgc = sG; bb = sB; }
    sample(cx - dx, cy + dy); if (sInk > best) { best = sInk; br = sR; bgc = sG; bb = sB; }
    sample(cx + dx, cy + dy); if (sInk > best) { best = sInk; br = sR; bgc = sG; bb = sB; }
    sInk = best; sR = br; sG = bgc; sB = bb;
  };

  /** Build a size step once: positions, reveal gate, ink and color for every cell with something to draw. */
  const buildStep = (si: number): GridStep => {
    const ch = cells[si], cw = ch * 0.62;
    const cols = Math.ceil(W / cw), rows = Math.ceil(H / ch);
    const dx = cw / 3, dy = ch / 3, qd = 6 * dp;
    const xs: number[] = [], ys: number[] = [], gs: number[] = [], ks: number[] = [];
    const colors: string[] | null = fixed ? null : [];
    for (let gy = 0; gy < rows; gy++) {
      const cy = (gy + 0.5) * ch;
      for (let gx = 0; gx < cols; gx++) {
        const cx = (gx + 0.5) * cw;
        best5(cx, cy, dx, dy);
        if (sInk < 0.04) continue; // nothing visible here at any flicker level
        xs.push(cx); ys.push(cy); ks.push(sInk);
        gs.push(0.5 * hash((cx / qd) | 0, (cy / qd) | 0, 11) + 0.5 * (Math.hypot(cx - ox, cy - oy) / maxD));
        colors?.push(`rgb(${sR},${sG},${sB})`);
      }
    }
    return { ch, n: xs.length, x: Float32Array.from(xs), y: Float32Array.from(ys), gate: Float32Array.from(gs), ink: Float32Array.from(ks), color: colors };
  };

  /** (NiceVault) A size step drawn once, one fillText per row of cells (fixed colour only). */
  const buildSprite = (si: number) => {
    const t0 = performance.now();
    const ch = cells[si];
    // the font sized so that one glyph advance is one cell (the grid's pitch is the font's)
    const cw = ch * 0.62;
    const fs = (cw * 100) / advance100(env.font);
    const cols = Math.ceil(W / cw), rows = Math.ceil(H / ch);
    const dx = cw / 3, dy = ch / 3, Lr = p.ramp.length;
    const c = newCanvas(W, H);
    const ctx = c.getContext('2d') as CanvasRenderingContext2D | null;
    if (ctx) {
      roundRectPath(ctx, 0, 0, W, H, radius);
      ctx.clip();
      ctx.font = `${fs.toFixed(2)}px ${env.font}`;
      ctx.textAlign = 'left';
      ctx.textBaseline = 'middle';
      ctx.fillStyle = fixed!;
      for (let gy = 0; gy < rows; gy++) {
        const cy = (gy + 0.5) * ch;
        let row = '', last = -1;
        for (let gx = 0; gx < cols; gx++) {
          best5((gx + 0.5) * cw, cy, dx, dy);
          const idx = sInk < 0.04 ? 0 : Math.round(clamp(sInk * 1.15) * (Lr - 1));
          row += idx > 0 ? p.ramp[idx] : ' ';
          if (idx > 0) last = gx;
        }
        if (last >= 0) ctx.fillText(row.slice(0, last + 1), 0, cy);
      }
    }
    spriteSpent += performance.now() - t0;
    return c;
  };
  const sprites: ((HTMLCanvasElement) | null)[] = cells.map(() => null);
  const sprite = (si: number): HTMLCanvasElement => {
    if (sprites[si]) return sprites[si]!;
    const k = key + '|' + si;
    let s = env.cache.get(k) as HTMLCanvasElement | undefined;
    if (!s) {
      // over this frame's budget: the step already drawn holds for one more frame
      if (spriteSpent > SPRITE_BUDGET) for (let j = si - 1; j >= 0; j--) if (sprites[j]) return sprites[j]!;
      s = buildSprite(si);
      env.cache.set(k, s);
    }
    // the steps before this one are not needed again
    for (let j = 0; j < si; j++) sprites[j] = null;
    return (sprites[si] = s);
  };

  // the source's opacity: written only when it changes (held at 0, a piece costs no style write
  // until its cross-fade)
  let srcO = -1;
  const setSrc = (o: number) => {
    const v = Math.round(o * 1000);
    if (v === srcO) return;
    srcO = v;
    (el as HTMLElement).style.setProperty('--ar-o', (v / 1000).toFixed(3));
  };
  const target: Target = {
    key: el,
    holdEl: el,
    holdName: M,
    cx: rc.left + rc.width / 2,
    cy: rc.top + rc.height / 2,
    offset: 0,
    dur,
    rect: { x, y, w: rc.width, h: rc.height },
    init(dir) {
      L = layer();
      setSrc(dir > 0 ? 0 : 1);
    },
    ready() {
      if (el instanceof HTMLImageElement && !(el.complete && el.naturalWidth)) {
        return Promise.race([el.decode().catch(() => {}), new Promise((r) => setTimeout(r, 900))]).then(() => {});
      }
      return undefined;
    },
    prepare() {
      data = null;
      gain = 1;
      steps = cells.map(() => null);
      for (let j = 0; j < sprites.length; j++) sprites[j] = null;
      if (picture) {
        key = `${el instanceof HTMLImageElement ? el.currentSrc || el.src : Math.random()}|${W}x${H}`;
        const hit = env.cache.get(key) as Uint8ClampedArray | undefined;
        if (hit) return void (data = hit);
        try {
          const off = newCanvas(SW, SH);
          const o = off.getContext('2d', { willReadFrequently: true }) as CanvasRenderingContext2D;
          if (el instanceof HTMLImageElement) {
            if (!el.naturalWidth) throw 0;
            const iw = el.naturalWidth, ih = el.naturalHeight;
            if (cs.objectFit === 'cover' || cs.objectFit === 'contain') {
              const sc = cs.objectFit === 'cover' ? Math.max(SW / iw, SH / ih) : Math.min(SW / iw, SH / ih);
              o.drawImage(el, (SW - iw * sc) / 2, (SH - ih * sc) / 2, iw * sc, ih * sc);
            } else o.drawImage(el, 0, 0, SW, SH);
          } else o.drawImage(el as HTMLCanvasElement | HTMLVideoElement, 0, 0, SW, SH);
          data = o.getImageData(0, 0, SW, SH).data;
        } catch {
          data = null; // cross-origin or not decodable: a neutral glyph texture
        }
      } else {
        key = shotKey;
        const hit = env.cache.get(key) as Uint8ClampedArray | null | undefined;
        if (hit !== undefined) {
          data = hit;
          gain = (env.cache.get(key + '|gain') as number) ?? 1;
          return;
        }
        data = inkPaint(shot, SW, SH);
        gain = inkGain(data);
        env.cache.set(key, data);
        env.cache.set(key + '|gain', gain);
      }
      if (picture) env.cache.set(key, data);
    },
    render(lt, _dir, paint) {
      const q = lt / dur;
      if (q >= 1) return setSrc(1);
      // (NiceVault) the piece comes through over the last 45 % and converges (ease-out): the last
      // frames before it is handed over already show it, never a jump from glyphs to it
      setSrc(q < 0.55 ? 0 : ease((q - 0.55) / 0.45));
      if (q <= 0 || !paint || !L) return;
      const alpha = q < 0.6 ? 1 : 1 - ease((q - 0.6) / 0.4);
      const si = Math.min(cells.length - 1, Math.floor((q / 0.8) * cells.length));
      const ctx = L.lctx;
      const gate = q * 2.3;
      if (fixed) {
        // the sprite of this step, clipped to the wave's front (three soft passes: the front is
        // ragged, as when each cell had its own threshold)
        const s = sprite(si);
        ctx.save();
        ctx.setTransform(1, 0, 0, 1, (x - L.x) * L.dpr, (y - L.y) * L.dpr);
        if (gate >= 1) {
          ctx.globalAlpha = alpha;
          ctx.drawImage(s, 0, 0);
        } else {
          for (const h of [0.15, 0.5, 0.85]) {
            const r = (2 * gate - h) * maxD;
            if (r <= 0) continue;
            ctx.save();
            ctx.beginPath();
            ctx.arc(ox, oy, r, 0, Math.PI * 2);
            ctx.clip();
            ctx.globalAlpha = alpha / 3;
            ctx.drawImage(s, 0, 0);
            ctx.restore();
          }
        }
        ctx.restore();
        return;
      }
      const st = steps[si] || (steps[si] = buildStep(si));
      ctx.save();
      // cells are in device px relative to the element; place them on the shared layer and clip to its shape
      ctx.setTransform(1, 0, 0, 1, (x - L.x) * L.dpr, (y - L.y) * L.dpr);
      roundRectPath(ctx, 0, 0, W, H, radius);
      ctx.clip();
      ctx.globalAlpha = alpha;
      ctx.font = `${Math.round(st.ch * 1.02)}px ${env.font}`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      if (fixed) ctx.fillStyle = fixed;
      const t = Math.floor(lt / p.tick);
      const Lr = p.ramp.length;
      for (let i = 0; i < st.n; i++) {
        if (st.gate[i] > gate) continue;
        let lv = st.ink[i];
        if (hash(i, t, 3) < 0.16) lv += (hash(i, t, 77) - 0.5) * 0.35;
        const idx = Math.round(clamp(lv * 1.15) * (Lr - 1));
        if (idx <= 0) continue;
        if (st.color) ctx.fillStyle = st.color[i];
        ctx.fillText(p.ramp[idx], st.x[i], st.y[i]);
      }
      ctx.restore();
    },
    resample() {
      // a picture arrived in the piece: read it again now (outside a frame: the caller reads
      // the layout anyway); it is drawn again on a frame with budget left
      if (!picture) takeShot();
      if (target.prepared) target.stale = true;
    },
    dispose() {
      L = null;
      for (let j = 0; j < sprites.length; j++) sprites[j] = null;
    },
  };
  return target;
}

/* ------------------------------------------------------------------ */
/* Surfaces and fades                                                  */
/* ------------------------------------------------------------------ */

function surfaceTarget(el: HTMLElement, env: Env, layer: () => Layer): Target | null {
  const rc = el.getBoundingClientRect();
  if (rc.width < 2 || rc.height < 2) return null;
  const R = env.region;
  if (rc.left - env.ox > R.x + R.w || rc.top - env.oy > R.y + R.h || rc.right - env.ox < R.x || rc.bottom - env.oy < R.y) return null;
  const cs = styleOf(env, el);
  const dp = env.dpr;
  const W = Math.round(rc.width * dp), H = Math.round(rc.height * dp);
  const x = rc.left - env.ox, y = rc.top - env.oy;
  const dur = env.timing.surface * env.scale;
  const ox = (env.origin.x - rc.left) * dp, oy = (env.origin.y - rc.top) * dp;
  const maxD = Math.max(Math.hypot(ox, oy), Math.hypot(W - ox, oy), Math.hypot(ox, H - oy), Math.hypot(W - ox, H - oy)) || 1;
  const cell = 14 * dp, cols = Math.ceil(W / cell), rows = Math.ceil(H / cell);
  const rad = Math.min((parseFloat(cs.borderTopLeftRadius) || 0) * dp, Math.min(W, H) / 2);
  const color = env.preset.accentGlyphs && env.accent ? env.accent : env.glyph || cs.color;
  const stroke = env.accent && env.preset.accentGlyphs ? env.accent : cs.color;
  const dotA = env.preset.accentGlyphs ? 0.75 : 0.86;
  const dotAlpha = env.glyph ? 0.9 : 0.45;
  let L: Layer | null = null;
  return {
    key: el,
    holdEl: el,
    holdName: S,
    cx: rc.left + rc.width / 2,
    cy: rc.top + rc.height / 2,
    offset: 0,
    dur,
    rect: { x, y, w: rc.width, h: rc.height },
    init() {
      L = layer();
      el.style.setProperty('--ar-o', '0');
    },
    render(lt, _dir, paint) {
      const q = lt / dur;
      el.style.setProperty('--ar-o', easeIO((q - 0.15) / 0.6).toFixed(3));
      if (q >= 1 || q <= 0 || !paint || !L) return;
      const ctx = L.lctx;
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, (x - L.x) * L.dpr, (y - L.y) * L.dpr);
      ctx.fillStyle = color;
      ctx.font = `${Math.round(11 * dp)}px ${env.font}`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      for (let gy = 0; gy < rows; gy++)
        for (let gx = 0; gx < cols; gx++) {
          const cx = (gx + 0.5) * cell, cy = (gy + 0.5) * cell;
          const v = 0.45 * hash(gx, gy, 21) + 0.55 * (Math.hypot(cx - ox, cy - oy) / maxD);
          const l = (q - v * 0.55) / 0.32;
          if (l <= 0 || l >= 1) continue;
          ctx.globalAlpha = Math.sin(l * Math.PI) * dotAlpha;
          ctx.fillText(hash(gx, gy, 22) > dotA ? '+' : '·', cx, cy);
        }
      const a = q < 0.12 ? q / 0.12 : q > 0.55 ? 1 - (q - 0.55) / 0.3 : 1;
      if (a > 0) {
        ctx.globalAlpha = clamp(a) * 0.75;
        ctx.strokeStyle = stroke;
        ctx.lineWidth = 1.25 * dp;
        const o = 0.75 * dp, k = rad + 10 * dp;
        ctx.beginPath();
        ctx.moveTo(o, k); ctx.arcTo(o, o, k, o, rad); ctx.lineTo(k, o);
        ctx.moveTo(W - k, o); ctx.arcTo(W - o, o, W - o, k, rad); ctx.lineTo(W - o, k);
        ctx.moveTo(W - o, H - k); ctx.arcTo(W - o, H - o, W - k, H - o, rad); ctx.lineTo(W - k, H - o);
        ctx.moveTo(k, H - o); ctx.arcTo(o, H - o, o, H - k, rad); ctx.lineTo(o, H - k);
        ctx.stroke();
      }
      ctx.restore();
    },
    dispose() {
      L = null;
    },
  };
}

function fadeTarget(el: HTMLElement, env: Env): Target | null {
  const rc = el.getBoundingClientRect();
  if (!rc.width && !rc.height) return null;
  const dur = env.timing.fade * env.scale;
  return {
    key: el,
    holdEl: el,
    holdName: F,
    cx: rc.left + rc.width / 2,
    cy: rc.top + rc.height / 2,
    offset: 0,
    dur,
    init(dir) {
      el.style.setProperty('--ar-o', dir > 0 ? '0' : '1');
    },
    render(lt) {
      el.style.setProperty('--ar-o', ease(lt / dur).toFixed(3));
    },
    dispose() {},
  };
}

/* ------------------------------------------------------------------ */
/* Controller                                                          */
/* ------------------------------------------------------------------ */

class Controller implements AsciiRevealController {
  root: HTMLElement;
  opts: AsciiRevealOptions;
  overlay: HTMLElement;
  ownOverlay: boolean;
  hidden = new Map<Element, Set<string>>();
  lastRects: { x: number; y: number; w: number; h: number }[] = [];
  shimmerLoop: (Loop & { stopAt: number; canvas: HTMLCanvasElement }) | null = null;
  seed = (Math.random() * 1e6) | 0;
  n = 0;
  destroyed = false;
  private activeRuns = 0;
  private ro: ResizeObserver | null = null;
  private size = { w: 0, h: 0 };

  constructor(root: HTMLElement, opts: AsciiRevealOptions) {
    ensureStyles();
    trackPointer();
    this.root = root;
    this.opts = { ...opts };
    if (getComputedStyle(root).position === 'static') root.style.position = 'relative';
    this.ownOverlay = !opts.overlay;
    this.overlay = opts.overlay || document.createElement('div');
    this.overlay.classList.add('ar-overlay');
    this.overlay.setAttribute('aria-hidden', 'true');
    if (this.ownOverlay) root.appendChild(this.overlay);
    // if the layout changes size mid-run, positions are stale: land everything instead of misaligning
    if (typeof ResizeObserver !== 'undefined') {
      this.ro = new ResizeObserver(([e]) => {
        const { width: w, height: h } = e.contentRect;
        const changed = Math.abs(w - this.size.w) > 1 || Math.abs(h - this.size.h) > 1;
        this.size = { w, h };
        if (!changed || !this.activeRuns) return;
        for (const run of Array.from(runs)) if (run.ctrl === this && run.dir > 0) finishRun(run, 'resize');
      });
      this.ro.observe(root);
    }
  }

  /** Content-box size of the root, in the same terms as ResizeObserver's contentRect. */
  private contentSize() {
    const r = this.root, cs = getComputedStyle(r);
    return {
      w: r.clientWidth - (parseFloat(cs.paddingLeft) || 0) - (parseFloat(cs.paddingRight) || 0),
      h: r.clientHeight - (parseFloat(cs.paddingTop) || 0) - (parseFloat(cs.paddingBottom) || 0),
    };
  }

  update(o: Partial<AsciiRevealOptions>) {
    Object.assign(this.opts, o);
  }

  /** Track running animations and tell the page (bubbling `asciireveal` event) when activity starts and stops. */
  active(d: 1 | -1) {
    const was = this.activeRuns > 0;
    this.activeRuns = Math.max(0, this.activeRuns + d);
    const is = this.activeRuns > 0;
    if (was !== is) this.root.dispatchEvent(new CustomEvent('asciireveal', { bubbles: true, detail: { active: is } }));
  }

  markHidden(el: Element, name: string) {
    let set = this.hidden.get(el);
    if (!set) this.hidden.set(el, (set = new Set()));
    if (set.has(name)) release(el, name); // already holding a hidden mark
    else set.add(name);
  }

  clearHidden(scope: Element) {
    for (const [el, names] of Array.from(this.hidden)) {
      if (!scope.contains(el) && el !== scope) continue;
      for (const n of names) release(el, n);
      this.hidden.delete(el);
    }
  }

  private origin(o: Origin | undefined, scope: Element) {
    const r = scope.getBoundingClientRect();
    const c = { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    if (!o || o === 'center') return c;
    if (o === 'top-left') return { x: r.left, y: r.top };
    if (o === 'pointer') return pointer || c;
    return o;
  }

  private env(o: RunOptions, scope: Element): Env {
    const orc = this.overlay.getBoundingClientRect();
    const rs = getComputedStyle(this.root);
    const v = (n: string) => rs.getPropertyValue(n).trim();
    const flash = parseColor(v('--ascii-flash'));
    // the text layer covers the visible part of the element plus a margin, never the whole page
    const m = 120;
    const vx0 = Math.max(0, -orc.left - m), vy0 = Math.max(0, -orc.top - m);
    const vx1 = Math.min(orc.width, window.innerWidth - orc.left + m), vy1 = Math.min(orc.height, window.innerHeight - orc.top + m);
    return {
      preset: resolvePreset(o.preset ?? this.opts.preset),
      timing: TIMING[o.duration ?? this.opts.duration ?? 'fast'],
      scale: 1 / Math.max(0.1, o.speed ?? this.opts.speed ?? 1),
      ox: orc.left,
      oy: orc.top,
      ow: orc.width,
      oh: orc.height,
      region: { x: vx0, y: vy0, w: Math.max(0, vx1 - vx0), h: Math.max(0, vy1 - vy0) },
      overlay: this.overlay,
      root: this.root,
      font: v('--ascii-font') || DEFAULT_FONT,
      accent: v('--ascii-accent') || null,
      flash,
      glyph: v('--ascii-glyph') || null,
      dpr: getDpr(),
      origin: this.origin(o.origin ?? this.opts.origin, scope),
      styles: new Map(),
      clips: new Map(),
      grounds: new Map(),
      cache: new Map(),
    };
  }

  /**
   * (NiceVault) The item roots of `scope` on screen now (see `items`), at most `cap`: found by
   * bisection over the items in layout order, so a run over 500 rows measures a dozen of them.
   */
  private visibleItems(scope: Element): Element[] {
    const list = scope.querySelectorAll(this.opts.items!);
    const n = list.length;
    if (!n) return [];
    const sr = scope.getBoundingClientRect();
    const top = Math.max(sr.top, 0), bottom = Math.min(sr.bottom, window.innerHeight);
    if (bottom <= top) return [];
    let lo = 0, hi = n;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (list[mid].getBoundingClientRect().bottom > top) hi = mid;
      else lo = mid + 1;
    }
    // the whole of that row (grid tiles of one row share their top, not their height)
    if (lo < n) {
      const t = list[lo].getBoundingClientRect().top;
      while (lo > 0 && list[lo - 1].getBoundingClientRect().top === t) lo--;
    }
    const out: Element[] = [];
    const cap = this.opts.cap ?? 1000;
    for (let i = lo; i < n && out.length < cap; i++) {
      const r = list[i].getBoundingClientRect();
      if (r.top >= bottom) break;
      if (r.bottom > top && r.right > sr.left && r.left < sr.right) out.push(list[i]);
    }
    return out;
  }

  /** Where a run looks for content: the scope itself, or (with `items`) the items on screen. */
  lastItems = 0;
  /** (NiceVault) the last collect's time on media and on text targets (RunStat) */
  lastSplit = { media: 0, text: 0 };
  private roots(scope: Element): Element[] {
    const r = this.opts.items ? this.visibleItems(scope) : [scope];
    this.lastItems = this.opts.items ? r.length : 0;
    return r;
  }

  private collect(scope: Element, env: Env, layer: () => Layer, only?: Element[]): Target[] {
    const targets: Target[] = [];
    if (only) {
      for (const el of only) { const t = el.isConnected ? mediaTarget(el as HTMLElement, env, layer) : null; if (t) targets.push(t); }
      return targets;
    }
    const sel = this.opts.selector;
    const inScope = (el: Element) => !el.closest('.ar-overlay') && (!sel || !!el.closest(sel));
    const roots = this.roots(scope);
    // (NiceVault) one query over the scope per kind, kept when inside an item on screen (not one
    // query per item)
    const rootSet = new Set<Element>(roots);
    const itemsSel = this.opts.items;
    const all = (q: string) => {
      const list: HTMLElement[] = [];
      if (!itemsSel) {
        for (const r of roots) {
          if (r.matches(q)) list.push(r as HTMLElement);
          list.push(...Array.from(r.querySelectorAll<HTMLElement>(q)));
        }
      } else if (roots.length) {
        for (const el of Array.from(scope.querySelectorAll<HTMLElement>(q))) {
          const it = el.closest(itemsSel);
          if (it && rootSet.has(it)) list.push(el);
        }
      }
      return list.filter((el) => inScope(el) && !el.closest('[data-ascii="skip"],[data-ascii-skeleton]'));
    };
    const surfaces = all(this.opts.surfaces || '[data-ascii="surface"]');
    const fades = all(this.opts.fades || '[data-ascii="fade"]');
    const media = all(this.opts.media || 'img, canvas, video, [data-ascii="media"]');
    const seed = this.seed + ++this.n * 131;
    for (const el of surfaces) { const t = surfaceTarget(el, env, layer); if (t) targets.push(t); }
    for (const el of fades) { const t = fadeTarget(el, env); if (t) targets.push(t); }
    const m0 = performance.now();
    for (const el of media) { const t = mediaTarget(el, env, layer); if (t) targets.push(t); }
    this.lastSplit.media = performance.now() - m0;
    const x0 = performance.now();
    let k = 0;
    // (NiceVault) the text nodes of each root, by a plain walk: what excludes a subtree is checked
    // once per element on the way down, not with closest() per text node (nor a TreeWalker
    // calling back into script for every node)
    const skipEl = (el: Element) => el.classList.contains('ar-overlay') || el.getAttribute('data-ascii') === 'skip' || el.hasAttribute('data-ascii-skeleton');
    const walk = (el: Element, inSel: boolean) => {
      for (let c = el.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 3) {
          if (!inSel || SKIP_TAGS.has(el.tagName) || !c.nodeValue || !c.nodeValue.trim()) continue;
          const t = textTarget(c as Text, env, seed + k++ * 977, layer);
          if (t) targets.push(t);
        } else if (c.nodeType === 1 && !skipEl(c as Element)) walk(c as Element, inSel || (!!sel && (c as Element).matches(sel)));
      }
    };
    for (const r of roots) {
      if (r.closest('.ar-overlay,[data-ascii="skip"],[data-ascii-skeleton]')) continue;
      walk(r, !sel || !!r.closest(sel));
    }
    this.lastSplit.text = performance.now() - x0;
    // content sitting on a surface starts a beat after the surface
    if (surfaces.length) {
      for (const t of targets) {
        if (t.holdName === S) continue;
        const el = t.holdEl;
        t.insideSurface = surfaces.some((s) => s !== el && s.contains(el));
      }
    }
    return targets;
  }

  /** Cheap pass for instant hides: which elements to hold, without measuring anything. */
  private collectLite(scope: Element, only?: Element[]): Pick<Target, 'key' | 'holdEl' | 'holdName'>[] {
    const out: Pick<Target, 'key' | 'holdEl' | 'holdName'>[] = [];
    if (only) {
      for (const el of only) if (el.isConnected) out.push({ key: el, holdEl: el, holdName: M });
      return out;
    }
    const sel = this.opts.selector;
    const ok = (el: Element) => !el.closest('.ar-overlay,[data-ascii="skip"],[data-ascii-skeleton]') && (!sel || !!el.closest(sel));
    const roots = this.roots(scope);
    const add = (q: string, name: string) => {
      for (const r of roots) {
        const list = Array.from(r.querySelectorAll(q));
        if (r.matches(q)) list.unshift(r);
        for (const el of list) if (ok(el)) out.push({ key: el, holdEl: el, holdName: name });
      }
    };
    add(this.opts.surfaces || '[data-ascii="surface"]', S);
    add(this.opts.fades || '[data-ascii="fade"]', F);
    add(this.opts.media || 'img, canvas, video, [data-ascii="media"]', M);
    for (const r of roots) {
      const walker = document.createTreeWalker(r, NodeFilter.SHOW_TEXT);
      while (walker.nextNode()) {
        const node = walker.currentNode as Text, p = node.parentElement;
        if (!p || !node.nodeValue?.trim() || SKIP_TAGS.has(p.tagName) || !ok(p)) continue;
        out.push({ key: node, holdEl: p, holdName: T });
      }
    }
    return out;
  }

  private makeLayer(env: Env, r = env.region): Layer {
    const d = env.dpr;
    const mk = () => {
      const c = document.createElement('canvas');
      c.width = Math.max(1, Math.round(r.w * d));
      c.height = Math.max(1, Math.round(r.h * d));
      Object.assign(c.style, { left: r.x + 'px', top: r.y + 'px', width: r.w + 'px', height: r.h + 'px' });
      this.overlay.appendChild(c);
      return c;
    };
    const bake = mk(), live = mk();
    return { live, lctx: live.getContext('2d')!, bake, bctx: bake.getContext('2d')!, x: r.x, y: r.y, w: r.w, h: r.h, dpr: d };
  }

  private async run(dir: 1 | -1, o: RunOptions = {}): Promise<void> {
    if (!isBrowser()) return o.onStart?.();
    let started = false;
    const start = () => {
      if (!started) (started = true), o.onStart?.();
    };
    try {
      return await this.runInner(dir, o, start);
    } finally {
      start();
    }
  }

  private async runInner(dir: 1 | -1, o: RunOptions, start: () => void): Promise<void> {
    const scope = o.within || this.root;
    if (dir < 0 && (o.instant || reducedMotion() || isDegraded())) {
      this.lastRects = [];
      const lite = this.collectLite(scope, o.only);
      readHolds(lite);
      for (const t of lite) {
        const prev = owner.get(t.key);
        if (prev) {
          // stop the running animation; its hold carries over as the hidden mark
          const pt = prev.byKey.get(t.key);
          pt?.dispose();
          prev.baked.delete(pt!);
          owner.delete(t.key);
          // (a cross-fade had already let it go: hold it again)
          if (pt?.early) (pt.early = false), hold(t.holdEl, t.holdName);
        } else hold(t.holdEl, t.holdName);
        if (t.holdName !== T) (t.holdEl as HTMLElement).style.setProperty('--ar-o', '0');
        this.markHidden(t.holdEl, t.holdName);
      }
      return;
    }
    // (NiceVault) the layout pending when the run starts (a view just mounted) is the page's cost,
    // not the decode's: done first and counted apart
    const l0 = performance.now();
    void (scope as HTMLElement).offsetWidth;
    const s0 = performance.now();
    const env = this.env(o, scope);
    // positions are measured against the layout as it is now; only a resize after this point makes them stale
    // (a loading swap usually resizes the wrapper right before the reveal starts, and that must not cancel it)
    this.size = this.contentSize();
    let layer: Layer | null = null;
    let area = env.region;
    let canvasMs = 0;
    const getLayer = () => {
      if (!layer) {
        const c0 = performance.now();
        layer = this.makeLayer(env, area);
        canvasMs = performance.now() - c0;
      }
      return layer;
    };
    const targets = this.collect(scope, env, getLayer, o.only);
    // the shared canvas only needs to cover what is animating (smaller canvas = cheaper frames)
    {
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (const t of targets) {
        const r = t.rect;
        if (!r || t.holdName === F) continue;
        x0 = Math.min(x0, r.x); y0 = Math.min(y0, r.y); x1 = Math.max(x1, r.x + r.w); y1 = Math.max(y1, r.y + r.h);
      }
      const R = env.region;
      if (isFinite(x0)) {
        x0 = Math.max(R.x, Math.floor(x0 - 4)); y0 = Math.max(R.y, Math.floor(y0 - 4));
        x1 = Math.min(R.x + R.w, Math.ceil(x1 + 4)); y1 = Math.min(R.y + R.h, Math.ceil(y1 + 4));
        area = { x: x0, y: y0, w: Math.max(1, x1 - x0), h: Math.max(1, y1 - y0) };
      }
    }
    if (dir < 0) this.lastRects = targets.filter((t) => t.rect).map((t) => t.rect!);

    if (dir > 0 && (reducedMotion() || isDegraded())) {
      // short fade instead of the decode: reduced motion, or a device that couldn't keep up
      for (const t of targets) {
        const prev = owner.get(t.key);
        if (prev) {
          const pt = prev.byKey.get(t.key);
          pt?.dispose();
          if (pt) letGo(pt);
          else release(t.holdEl, t.holdName);
          owner.delete(t.key);
        }
      }
      this.clearHidden(scope);
      // (NiceVault) through --nv-o, never an opacity animation (lib/motion.ts: WebKitGTK's stale end frame)
      if ((scope as HTMLElement).animate) fade(scope as HTMLElement, 0, 1, { duration: 220, easing: 'ease-out' });
      return;
    }

    let resolve!: () => void;
    const run: Run = {
      ctrl: this,
      dir,
      start: Infinity,
      targets,
      byKey: new Map(targets.map((t) => [t.key, t])),
      baked: new Set(),
      layer: null,
      lastPaint: -Infinity,
      done: new Promise<void>((r) => (resolve = r)),
      resolve: () => resolve(),
      live: false,
      onStart: start,
      stat: { t: Math.round(s0), layout: Math.round(s0 - l0), dir, root: this.root.className.split(' ')[0] || this.root.tagName, targets: targets.length, items: this.lastItems, setup: 0, collect: 0, media: 0, text: 0, hold: 0, clear: 0, reads: 0, canvas: 0, prepare: 0, ready: 0, end: 0, at: 0, why: 'natural', frames: 0, tickMean: 0, tickMax: 0, tickSum: 0, textMs: 0, mediaMs: 0 },
    };
    run.stat.collect = performance.now() - s0;
    run.stat.media = this.lastSplit.media;
    run.stat.text = this.lastSplit.text;
    const h0 = performance.now();
    readHolds(targets);
    run.stat.reads = performance.now() - h0;
    for (const t of targets) {
      claim(run, t);
      t.init(dir);
    }
    run.layer = layer;
    const h1 = performance.now();
    if (dir > 0) this.clearHidden(scope);
    run.stat.hold = h1 - h0;
    run.stat.canvas = canvasMs;
    run.stat.clear = performance.now() - h1;

    const tm = env.timing;
    const vel = tm.vel / env.scale;
    for (const t of targets) {
      const wave = Math.min(Math.hypot(t.cx - env.origin.x, t.cy - env.origin.y) / vel, tm.maxWave * env.scale);
      t.offset = wave + (t.insideSurface && dir > 0 ? tm.onSurface * env.scale : 0);
    }
    for (const t of targets) run.stat.end = Math.max(run.stat.end, Math.round(t.offset + t.dur));
    run.stat.setup = performance.now() - s0;
    if (dir > 0 && targets.some((t) => t.ready)) {
      const r0 = performance.now();
      await Promise.all(targets.map((t) => t.ready?.()));
      run.stat.ready = Math.round(performance.now() - r0);
    }
    // (prepare — sampling pictures, drawing ink maps — happens frame by frame as the wave reaches
    // each target, within the frame budget: see tick)
    if (this.destroyed || !targets.some((t) => owner.get(t.key) === run)) {
      (layer as Layer | null)?.live.remove();
      (layer as Layer | null)?.bake.remove();
      run.resolve();
      return run.done;
    }
    run.start = PENDING;
    run.live = true;
    runs.add(run);
    this.active(1);
    kick();
    return run.done;
  }

  reveal(o: RunOptions = {}) {
    if (this.opts.lazy !== false && !o.within && isBrowser() && !reducedMotion() && 'IntersectionObserver' in window) return this.lazyReveal(o);
    return this.run(1, o);
  }

  private io: IntersectionObserver | null = null;
  /** Reveal what's on screen now; hide the rest and reveal each section when it scrolls into view. */
  private lazyReveal(o: RunOptions): Promise<void> {
    const kids = (el: Element) => Array.from(el.children).filter((c) => !c.classList.contains('ar-overlay'));
    let scope: Element = this.root;
    while (kids(scope).length === 1) scope = kids(scope)[0];
    const parts = kids(scope);
    const vh = window.innerHeight;
    const near: Element[] = [], far: Element[] = [];
    for (const el of parts) {
      const r = el.getBoundingClientRect();
      (r.top < vh * 1.02 && r.bottom > 0 ? near : far).push(el);
    }
    if (!far.length || !near.length) return this.run(1, o);
    this.io?.disconnect();
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        io.unobserve(e.target);
        const r = e.target.getBoundingClientRect();
        // each section decodes from its own top edge as it arrives
        this.run(1, { ...o, within: e.target, origin: { x: r.left + r.width / 2, y: Math.max(r.top, 0) } });
      }
    }, { rootMargin: '0px 0px -8% 0px' });
    this.io = io;
    // off-screen sections are hidden (text stays in the DOM for readers and search engines) and decode on arrival
    for (const el of far) {
      this.run(-1, { ...o, within: el, instant: true });
      io.observe(el);
    }
    return Promise.all(near.map((el) => this.run(1, { ...o, within: el }))).then(() => {});
  }

  hide(o?: RunOptions) {
    return this.run(-1, o);
  }

  shimmer(on: boolean) {
    if (!isBrowser()) return;
    if (!on) {
      if (this.shimmerLoop && this.shimmerLoop.stopAt === Infinity) this.shimmerLoop.stopAt = performance.now();
      return;
    }
    if (this.shimmerLoop) {
      this.shimmerLoop.stopAt = Infinity;
      return;
    }
    if (reducedMotion()) return;
    const draw = this.mascotDraw();
    // a skeleton already shows the loading state; the plain shimmer would only add noise on top of it
    if (!draw && this.root.querySelector('[data-ascii-skeleton]')) return;
    const dp = getDpr();
    const orc = this.overlay.getBoundingClientRect();
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(orc.width * dp);
    canvas.height = Math.round(orc.height * dp);
    Object.assign(canvas.style, { left: '0px', top: '0px', width: orc.width + 'px', height: orc.height + 'px' });
    this.overlay.appendChild(canvas);
    const ctx = canvas.getContext('2d')!;
    if (draw) {
      const loop = this.mascotLoop(canvas, ctx, draw, dp, orc);
      this.shimmerLoop = loop;
      loops.add(loop);
      kick();
      return;
    }
    const preset = resolvePreset(this.opts.preset);
    const ramp = preset.ramp.slice(1, Math.max(3, Math.ceil(preset.ramp.length * 0.6)));
    const font = cssVar(this.root, '--ascii-font') || DEFAULT_FONT;
    const accent = cssVar(this.root, '--ascii-accent');
    const color = (preset.accentGlyphs || preset.cursor) && accent ? accent : cssVar(this.root, '--ascii-glyph') || getComputedStyle(this.root).color;
    const rects = (this.lastRects.length ? this.lastRects : [{ x: 0, y: 0, w: orc.width, h: orc.height }]).map((r) => ({
      x: r.x * dp, y: r.y * dp, w: r.w * dp, h: r.h * dp,
    }));
    const ch = 11 * dp, cw = ch * 0.62;
    const t0 = performance.now();
    let lastDraw = -Infinity;
    const loop = {
      stopAt: Infinity,
      canvas,
      render: (now: number) => {
        const lt = now - t0;
        const fin = clamp(lt / 260);
        const fout = loop.stopAt === Infinity ? 1 : 1 - clamp((now - loop.stopAt) / 260);
        if (fout <= 0) {
          canvas.remove();
          if (this.shimmerLoop === loop) this.shimmerLoop = null;
          return false;
        }
        if (lt - lastDraw < 33 && fin >= 1 && fout >= 1) return true; // ~30 fps is plenty for a shimmer
        lastDraw = lt;
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        ctx.globalAlpha = 0.6 * fin * fout;
        ctx.font = `${Math.round(ch)}px ${font}`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillStyle = color;
        const tick = Math.floor(lt / 70);
        for (const R of rects) {
          const cols = Math.max(1, Math.floor(R.w / cw)), rows = Math.max(1, Math.floor(R.h / ch));
          const x0 = R.x + (R.w - cols * cw) / 2;
          for (let gy = 0; gy < rows; gy++)
            for (let gx = 0; gx < cols; gx++) {
              const w = 0.5 + 0.5 * Math.sin(gx * 0.5 - lt * 0.007 + gy * 1.7 + R.y * 0.01);
              let idx = Math.floor(w * ramp.length * 0.999);
              if (hash((gx + R.x) | 0, gy, tick) < 0.2) idx = Math.floor(hash(gx, (gy + R.y) | 0, tick) * ramp.length);
              ctx.fillText(ramp[idx], x0 + (gx + 0.5) * cw, R.y + (gy + 0.5) * ch + (R.h - rows * ch) / 2);
            }
        }
        ctx.globalAlpha = 1;
        return true;
      },
    };
    this.shimmerLoop = loop;
    loops.add(loop);
    kick();
  }

  private mascotCache: { key: unknown; draw: MascotDraw } | null = null;
  private mascotDraw(): MascotDraw | null {
    const m = this.opts.mascot;
    if (!m) return null;
    if (typeof m === 'function') return m;
    if (m in mascots) return mascots[m as MascotName];
    if (this.mascotCache?.key !== m) this.mascotCache = { key: m, draw: imageMascot(m) };
    return this.mascotCache.draw;
  }

  /** Animated mascot rendered through the glyph grid where the hidden content was. */
  private mascotLoop(canvas: HTMLCanvasElement, ctx: CanvasRenderingContext2D, draw: MascotDraw, dp: number, orc: DOMRect) {
    // area: the union of what was hidden, padded, at least 170px tall, kept inside the element
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const r of this.lastRects) { x0 = Math.min(x0, r.x); y0 = Math.min(y0, r.y); x1 = Math.max(x1, r.x + r.w); y1 = Math.max(y1, r.y + r.h); }
    if (!isFinite(x0)) { x0 = 0; y0 = 0; x1 = orc.width; y1 = orc.height; }
    x0 -= 12; y0 -= 12; x1 += 12; y1 += 12;
    const minH = Math.min(170, orc.height);
    if (y1 - y0 < minH) { const c = (y0 + y1) / 2; y0 = c - minH / 2; y1 = c + minH / 2; }
    x0 = Math.max(0, x0); y0 = Math.max(0, y0); x1 = Math.min(orc.width, x1); y1 = Math.min(orc.height, y1);
    const preset = resolvePreset(this.opts.preset);
    const ramp = preset.ramp;
    const L = ramp.length;
    const font = cssVar(this.root, '--ascii-font') || DEFAULT_FONT;
    const color = cssVar(this.root, '--ascii-mascot') || getComputedStyle(this.root).color;
    const ch = 7 * dp, cw = ch * 0.62;
    const cols = Math.max(4, Math.floor(((x1 - x0) * dp) / cw)), rows = Math.max(4, Math.floor(((y1 - y0) * dp) / ch));
    const ox = x0 * dp + ((x1 - x0) * dp - cols * cw) / 2, oy = y0 * dp + ((y1 - y0) * dp - rows * ch) / 2;
    // the mascot is drawn at 2 samples per cell, then read back as glyph densities
    const off = document.createElement('canvas');
    const OW = cols * 2, OH = Math.round(rows * 2 * (ch / cw));
    off.width = OW; off.height = OH;
    const o = off.getContext('2d', { willReadFrequently: true })!;
    const sy = OH / rows;
    const t0 = performance.now();
    let lastDraw = -Infinity;
    const loop = {
      stopAt: Infinity,
      canvas,
      render: (now: number) => {
        const lt = now - t0;
        const fin = clamp(lt / 320);
        const gone = loop.stopAt === Infinity ? 0 : clamp((now - loop.stopAt) / 450);
        if (gone >= 1) {
          canvas.remove();
          if (this.shimmerLoop === loop) this.shimmerLoop = null;
          return false;
        }
        if (lt - lastDraw < 33) return true; // mascots animate at ~30 fps
        lastDraw = lt;
        o.clearRect(0, 0, OW, OH);
        o.save();
        draw(o, OW, OH, lt / 1000, color);
        o.restore();
        const d = o.getImageData(0, 0, OW, OH).data;
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        ctx.font = `${Math.round(ch)}px ${font}`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        const tick = Math.floor(lt / 80);
        let lastFill = '';
        for (let gy = 0; gy < rows; gy++) {
          const py = Math.min(OH - 1, Math.floor((gy + 0.5) * sy));
          for (let gx = 0; gx < cols; gx++) {
            if (hash(gx, gy, 9) > fin || hash(gx, gy, 13) < gone) continue;
            const px = gx * 2 + 1;
            let a = 0, r = 0, g = 0, b = 0;
            for (let s = 0; s < 4; s++) {
              const i = (Math.max(0, py - (s >> 1)) * OW + Math.max(0, px - (s & 1))) * 4;
              const al = d[i + 3] / 255;
              a += al; r += d[i] * al; g += d[i + 1] * al; b += d[i + 2] * al;
            }
            if (a < 0.25) continue;
            r /= a; g /= a; b /= a; a /= 4;
            let lv = a;
            if (hash(gx, gy, tick) < 0.12) lv += (hash(gx, gy, tick + 5) - 0.5) * 0.4;
            const idx = Math.max(1, Math.min(L - 1, Math.round(clamp(lv) * (L - 1))));
            // quantize colors a little so most cells reuse the previous fillStyle
            const fill = `rgb(${r & 0xf8},${g & 0xf8},${b & 0xf8})`;
            if (fill !== lastFill) { ctx.fillStyle = fill; lastFill = fill; }
            ctx.fillText(ramp[idx], ox + (gx + 0.5) * cw, oy + (gy + 0.5) * ch);
          }
        }
        return true;
      },
    };
    return loop;
  }

  destroy() {
    this.destroyed = true;
    this.io?.disconnect();
    this.io = null;
    this.ro?.disconnect();
    this.ro = null;
    for (const run of Array.from(runs)) {
      if (run.ctrl !== this) continue;
      runs.delete(run);
      logStat(run, 'destroy');
      for (const t of run.targets) {
        if (owner.get(t.key) !== run) continue;
        owner.delete(t.key);
        t.dispose();
        letGo(t);
      }
      run.layer?.live.remove();
      run.layer?.bake.remove();
      run.resolve();
    }
    this.activeRuns = 0;
    for (const [el, names] of this.hidden) for (const n of names) release(el, n);
    this.hidden.clear();
    if (this.shimmerLoop) {
      loops.delete(this.shimmerLoop);
      this.shimmerLoop.canvas.remove();
      this.shimmerLoop = null;
    }
    if (this.ownOverlay) this.overlay.remove();
    else this.overlay.replaceChildren();
  }
}

/** Attach the effect to any element. Works without React. */
export function createAsciiReveal(root: HTMLElement, options: AsciiRevealOptions = {}): AsciiRevealController {
  return new Controller(root, options);
}
