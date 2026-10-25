// The glyph decode: content resolving out of a grid of machine-readable glyphs, the
// NiceVault signature. Thin Svelte actions over the vendored ascii-reveal core
// (src/vendor/ascii-reveal, MIT): presentation only. The content stays in the DOM, in
// place, the whole time (screen readers, tests and selection see the real text); the
// glyphs are drawn on a canvas above it and never take a pointer event.
//
// When it plays (and only then):
//   - the first time Benvenuto or Sblocca is shown in a session (on start), its document
//     elements decode in sequence (Sblocca not when a key brought it there);
//   - right after the unlock ceremony the file view resolves;
//   - the first time a folder is shown in a session (pointer, or Enter on it), its files decode
//     in a quick stagger (the whole folder ~300 ms);
//   - thumbnails decode the first time each is shown in a session, quick-look media as their
//     bytes arrive (~350 ms);
// Only what is on screen when it starts: a list or grid decodes the items in view (all of them);
// the others, and items that arrive later (scrolled to), show as they are. Thumbnails that arrive
// off screen, or while the view is being scrolled, show as they are too.
//   - context and sort menus compose (~150 ms);
//   - loading states are glyph skeletons.
// Enter is not a "keyboard action" here: it is how a password is submitted and a folder opened.
// Never on other keys (arrows, Space, Backspace...), repeated paths, re-renders, sorting,
// selection or rename.
// Every decision is logged in window.__nvReveal (frame-capture checks in the real app read it).
// prefers-reduced-motion: none of it plays (skeletons stand still).
import {
  adoptPicture,
  animateSkeletons,
  asciiSkeletonHTML,
  createAsciiReveal,
  pieceShown,
  SKELETON_CSS,
  type AsciiRevealController,
  type Origin,
  type Preset,
  type SkeletonSpec,
} from '../vendor/ascii-reveal';

/** The machine-readable alphabet of the document's MRZ, condensing through a light ramp. */
export const MRZ: Preset = {
  ramp: ' .:-=+<#',
  scramble: 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789<',
  mode: 'ramp',
  color: 'accent',
  cells: [18, 12, 9, 7, 5],
  tick: 36,
  accentGlyphs: true,
};

const browser = typeof window !== 'undefined';

/** Reviewing the motion frame by frame: window.__nvRevealSlow = 8 plays everything 8x slower. */
function pace(speed: number): number {
  const slow = browser ? Number((window as unknown as { __nvRevealSlow?: number }).__nvRevealSlow) || 1 : 1;
  return speed / Math.max(1, slow);
}

export function motionAllowed(): boolean {
  return browser && typeof matchMedia === 'function' && !matchMedia('(prefers-reduced-motion: reduce)').matches;
}

// --- what asked for the thing now on screen: a key or the pointer ------------------
let lastKey = -1e9;
let lastPointer = -1e9;
if (browser) {
  window.addEventListener(
    'keydown',
    (e) => {
      // Enter submits and opens: what it brings on screen is shown like a click's (also after
      // the arrows that walked to it)
      lastKey = e.key === 'Enter' ? -1e9 : performance.now();
    },
    { capture: true, passive: true },
  );
  window.addEventListener('pointerdown', () => (lastPointer = performance.now()), { capture: true, passive: true });
  window.addEventListener('scroll', () => (lastScroll = performance.now()), { capture: true, passive: true });
}
let lastScroll = -1e9;
/** A view was scrolled within `ms`: pictures arriving now just appear. */
function scrolling(ms = 200): boolean {
  return performance.now() - lastScroll < ms;
}
/** The most recent input was a key (other than Enter), within `ms`: the motion stays out of the way. */
export function fromKeyboard(ms = 1000): boolean {
  return lastKey > lastPointer && performance.now() - lastKey < ms;
}

// --- once per session ---------------------------------------------------------------
const seen = new Set<string>();
/** True the first time `key` is asked for in this session (and marks it). */
export function firstTime(key: string): boolean {
  if (seen.has(key)) return false;
  seen.add(key);
  return true;
}

export interface RevealLogEntry {
  t: number;
  kind: 'decode' | 'media' | 'compose';
  key: string;
  /** played, or why not */
  what: 'play' | 'done' | 'reduced' | 'keyboard' | 'cached' | 'small' | 'late' | 'offscreen' | 'scrolling' | 'merged' | 'shown' | 'large';
}
/** Decisions of this session, newest last (capped): window.__nvReveal. */
export function revealLog(kind: RevealLogEntry['kind'], key: string, what: RevealLogEntry['what']): void {
  if (!browser) return;
  const w = window as unknown as { __nvReveal?: RevealLogEntry[] };
  const log = (w.__nvReveal ??= []);
  log.push({ t: Math.round(performance.now()), kind, key, what });
  if (log.length > 200) log.splice(0, log.length - 200);
}

/** Fonts measured before glyphs are placed (bundled, so this is quick; capped anyway). */
function fontsReady(cap = 400): Promise<void> {
  if (!browser || !document.fonts || document.fonts.status === 'loaded') return Promise.resolve();
  return Promise.race([document.fonts.ready.then(() => undefined), new Promise<void>((r) => setTimeout(r, cap))]);
}

/** On a view that is about to decode, until its first glyph frame (styles/base.css). */
const PENDING = 'data-nv-pending';
/** Failsafes: content waited for this long is shown plainly (and not decoded after) ... */
const WAIT_CAP = 2500;
/** ... and a decode that hasn't drawn its first frame this long after it was asked for. */
const START_CAP = 800;

export interface DecodeOptions {
  /** session key: plays once per key */
  key: string;
  /** another key this view stands for (marked seen too) */
  also?: string;
  /** play when this turns true (e.g. the listing has loaded) */
  ready?: boolean;
  /** 1 = the library's "fast" (data readable in ~400 ms); higher is quicker */
  speed?: number;
  /** wait this long (content held back meanwhile), e.g. while the page turns */
  delay?: number;
  origin?: Origin;
  /** only content inside elements matching this */
  selector?: string;
  /** elements that fade in on the wave (default [data-ascii="fade"]) */
  fades?: string;
  /** elements that assemble from a dot field (default [data-ascii="surface"]) */
  surfaces?: string;
  /** elements rendered through the glyph grid (default: only [data-ascii="media"]) */
  media?: string;
  /** skip it when a key other than Enter asked for this view (Backspace up a folder) */
  pointerOnly?: boolean;
  /** the repeated items (rows, tiles): only those on screen decode (engine `items`) */
  items?: string;
}

/**
 * use:decode={{ key, ready, ... }} — the subtree decodes from glyphs once per session
 * per key, when `ready`. Holds the content back from the moment it plays until the
 * wave reaches it; never delays input (nothing it covers stops a click or a key).
 */
export function decode(node: HTMLElement, opts: DecodeOptions) {
  let ctrl: AsciiRevealController | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let alive = true;

  // Pending: from the moment this view is going to decode until the decode draws its first
  // frame, nothing of it is shown (opacity 0 on its children: no layout shift, the text stays
  // in the accessibility tree). Never a frame of the plain content before its glyphs.
  let failsafe: ReturnType<typeof setTimeout> | undefined;
  let pending = 0;
  const unpend = () => {
    clearTimeout(failsafe);
    pending++;
    node.removeAttribute(PENDING);
  };
  const pend = (ms: number, o: DecodeOptions) => {
    clearTimeout(failsafe);
    const mine = ++pending;
    node.setAttribute(PENDING, '');
    // counted in frames actually drawn: a window not painting yet (app start) shows nothing anyway
    failsafe = setTimeout(() => requestAnimationFrame(() => {
      if (mine !== pending) return;
      // the decode didn't start in time: show the content as it is, and don't decode it later
      // (a decode after the plain content would be the flash this is here to prevent)
      if (o.ready === false && !seen.has(o.key)) {
        firstTime(o.key);
        if (o.also) firstTime(o.also);
        revealLog('decode', o.key, 'late');
      }
      ctrl?.destroy();
      ctrl = null;
      unpend();
    }), ms);
  };
  const willPlay = (o: DecodeOptions) => !seen.has(o.key) && motionAllowed() && !(o.pointerOnly && fromKeyboard());

  const play = (o: DecodeOptions) => {
    if (seen.has(o.key)) return;
    if (o.ready === false) {
      // waiting for the content (the volumes found, the listing): held from the first paint
      if (willPlay(o) && !node.hasAttribute(PENDING)) pend(WAIT_CAP, o);
      return;
    }
    firstTime(o.key);
    if (o.also) firstTime(o.also);
    // a newer view replaces one still decoding (fast navigation)
    clearTimeout(timer);
    ctrl?.destroy();
    ctrl = null;
    if (!motionAllowed()) return unpend(), revealLog('decode', o.key, 'reduced');
    if (o.pointerOnly && fromKeyboard()) return unpend(), revealLog('decode', o.key, 'keyboard');
    revealLog('decode', o.key, 'play');
    pend(START_CAP + (o.delay ?? 0), o);
    ctrl = createAsciiReveal(node, {
      preset: MRZ,
      speed: pace(o.speed ?? 1),
      origin: o.origin ?? 'top-left',
      lazy: false,
      selector: o.selector,
      fades: o.fades,
      surfaces: o.surfaces,
      media: o.media ?? '[data-ascii="media"]',
      items: o.items,
    });
    const c = ctrl;
    // held back at once, decoded after the wait (and once the fonts are measured)
    void c.hide({ instant: true });
    timer = setTimeout(() => {
      void fontsReady().then(() => {
        if (!alive || ctrl !== c) return;
        void c.reveal({ onStart: unpend }).then(() => {
          revealLog('decode', o.key, 'done');
          if (ctrl === c) {
            c.destroy();
            ctrl = null;
          }
        });
      });
    }, o.delay ?? 0);
  };

  play(opts);
  return {
    update(o: DecodeOptions) {
      play(o);
    },
    destroy() {
      alive = false;
      clearTimeout(timer);
      ctrl?.destroy();
      ctrl = null;
      unpend();
    },
  };
}

/** Shows an <img> only once it is decoded (off the main thread), so a large picture never
 *  stalls a frame while it is painted for the first time. */
function whenDecoded(node: HTMLImageElement | HTMLVideoElement, then: () => void): void {
  if (node instanceof HTMLImageElement && typeof node.decode === 'function') node.decode().then(then, then);
  else then();
}

interface MediaWait {
  node: HTMLImageElement | HTMLVideoElement;
  key: string;
  speed: number;
  min: number;
  show(): void;
  ctrl: AsciiRevealController | null;
}
/** Pixels past which a picture is an original, not a thumbnail (thumbs.rs: 1024 px, fit 2560 px). */
export const LARGE_PX = 8_000_000;
/** Pictures whose bytes arrived since the last frame: decoded together, under one controller per host. */
let arrivals: MediaWait[] = [];
let arrivalsRaf = 0;

function flushArrivals() {
  arrivalsRaf = 0;
  const batch = arrivals;
  arrivals = [];
  // reads first (one layout for the whole batch), then the writes
  const vw = window.innerWidth, vh = window.innerHeight;
  const busy = scrolling();
  type Seen = { host: HTMLElement | null; what: RevealLogEntry['what'] | 'decode' };
  const seen = batch.map((w): Seen | null => {
    if (!w.node.isConnected) return null;
    const r = w.node.getBoundingClientRect();
    const host = w.node.closest('[data-ascii-host]') as HTMLElement | null;
    const hr = host?.getBoundingClientRect();
    const top = Math.max(0, hr?.top ?? 0), bottom = Math.min(vh, hr?.bottom ?? vh);
    const onScreen = r.bottom > top && r.top < bottom && r.right > 0 && r.left < vw;
    const h = host ?? w.node.parentElement;
    if (!h) return { host: null, what: 'decode' };
    if (Math.min(r.width, r.height) < w.min) return { host: h, what: 'small' };
    // an original the backend could not make small (thumbs.rs: a format it cannot read, e.g. a
    // 12 MP HEIC on a desktop): never drawn into the decode's canvas (a full-size decode on the
    // main thread; WKWebView left grid tiles black), it just shows
    if (w.node instanceof HTMLImageElement && w.node.naturalWidth * w.node.naturalHeight > LARGE_PX) return { host: h, what: 'large' };
    if (!onScreen) return { host: h, what: 'offscreen' };
    if (busy) return { host: h, what: 'scrolling' };
    // inside a piece the folder's decode is resolving (or about to): the picture is part of that
    // decode — its glyphs are drawn from the piece again, now with the picture — never a second one
    // (adopting reads the piece's layout again: done here, among the reads)
    if (adoptPicture(w.node)) return { host: h, what: 'merged' };
    // its piece was decoded already (the picture came after): it just fades in, the item's one
    // decode was that one
    if (pieceShown(w.node, h)) return { host: h, what: 'shown' };
    return { host: h, what: 'decode' };
  });
  const groups = new Map<HTMLElement, MediaWait[]>();
  batch.forEach((w, i) => {
    const s = seen[i];
    if (!s || !s.host) return w.show();
    if (s.what !== 'decode') {
      revealLog('media', w.key, s.what);
      w.show();
      if (s.what === 'shown') fadeIn(w.node);
      return;
    }
    let g = groups.get(s.host);
    if (!g) groups.set(s.host, (g = []));
    g.push(w);
  });
  for (const [host, g] of groups) {
    const c = createAsciiReveal(host, { preset: MRZ, speed: pace(g[0].speed), origin: 'top-left', lazy: false, media: 'img, video' });
    const only = g.map((w) => w.node);
    // the engine holds the pictures (synchronously) before our veils drop, then decodes them:
    // never a frame where a picture shows, blanks and comes back
    void c.hide({ instant: true, only });
    for (const w of g) {
      revealLog('media', w.key, 'play');
      w.ctrl = c;
      w.show();
    }
    void c.reveal({ only }).then(() => {
      c.destroy();
      for (const w of g) if (w.ctrl === c) w.ctrl = null;
    });
  }
}

/** A short compositor fade (a picture arriving in a piece already decoded). */
function fadeIn(node: HTMLElement) {
  node.animate?.([{ opacity: 0 }, { opacity: 1 }], { duration: 220 / pace(1), easing: 'ease-out' });
}

/**
 * use:decodeMedia — an <img> or <video> decodes from glyphs in the volume's ink as its
 * bytes arrive. Not when it was already there (cache) unless `key` asks for its first
 * showing in this session (quick look: the file is opened, its bytes decrypted for it);
 * never after a key press, never off screen or while the view scrolls. Pictures arriving in
 * the same frame decode together (one controller and one canvas layer per host:
 * the nearest [data-ascii-host], else the parent). Nothing is measured until then.
 * An <img> is shown only once decoded (img.decode(), off the main thread).
 */
export function decodeMedia(node: HTMLImageElement | HTMLVideoElement, opts: { speed?: number; min?: number; key?: string } = {}) {
  const key = opts.key ?? '';
  let alive = true;
  const plain = (what: RevealLogEntry['what']) => {
    revealLog('media', key, what);
    // still never painted half-decoded on the main thread
    if (node instanceof HTMLImageElement && !(node.complete && node.naturalWidth)) {
      node.style.opacity = '0';
      const show = () => alive && node.style.removeProperty('opacity');
      whenDecoded(node, show);
      setTimeout(show, 4000);
    }
    return {
      destroy() {
        alive = false;
        node.style.removeProperty('opacity');
      },
    };
  };
  if (!motionAllowed()) return plain('reduced');
  if (fromKeyboard()) return plain('keyboard');
  const ready = node instanceof HTMLImageElement ? node.complete && node.naturalWidth > 0 : node.readyState >= 2;
  const first = opts.key ? firstTime(opts.key) : false;
  if (ready && !first) return plain('cached');
  if (opts.key && !first) return plain('cached');
  const w: MediaWait = {
    node,
    key,
    speed: opts.speed ?? 1.15,
    min: opts.min ?? 48,
    show: () => {
      clearTimeout(fallback);
      node.style.removeProperty('opacity');
    },
    ctrl: null,
  };
  node.style.opacity = '0';
  const fallback = setTimeout(w.show, 4000);
  const arrived = () => {
    if (!alive || !node.isConnected) return w.show();
    arrivals.push(w);
    arrivalsRaf ||= requestAnimationFrame(flushArrivals);
  };
  const failed = () => w.show();
  const ev = node instanceof HTMLImageElement ? 'load' : 'loadeddata';
  const onEv = () => whenDecoded(node, arrived);
  // already here: decode on the next frame, once the surface it sits on is laid out
  if (ready) onEv();
  else {
    node.addEventListener(ev, onEv, { once: true });
    node.addEventListener('error', failed, { once: true });
  }
  return {
    destroy() {
      alive = false;
      clearTimeout(fallback);
      node.removeEventListener(ev, onEv);
      node.removeEventListener('error', failed);
      arrivals = arrivals.filter((x) => x !== w);
      w.ctrl = null;
      node.style.removeProperty('opacity');
    },
  };
}

/**
 * use:compose={ready} — a menu's items compose from glyphs as it opens (~150 ms), once
 * it has been placed. Not when it was opened from the keyboard.
 */
export function compose(node: HTMLElement, ready: boolean) {
  const keyboard = fromKeyboard(400);
  let ctrl: AsciiRevealController | null = null;
  let played = false;
  const play = (r: boolean) => {
    if (!r || played) return;
    played = true;
    if (!motionAllowed()) return revealLog('compose', 'menu', 'reduced');
    if (keyboard) return revealLog('compose', 'menu', 'keyboard');
    revealLog('compose', 'menu', 'play');
    ctrl = createAsciiReveal(node, { preset: MRZ, speed: pace(2.7), origin: 'top-left', lazy: false, media: '[data-ascii="media"]' });
    const c = ctrl;
    void c.reveal().then(() => {
      c.destroy();
      if (ctrl === c) ctrl = null;
    });
  };
  play(ready);
  return {
    update: play,
    destroy() {
      ctrl?.destroy();
      ctrl = null;
    },
  };
}

// --- skeletons: the loading state as a still field of glyphs with a scan running through it

let skeletonCss = false;
function ensureSkeletonCss() {
  if (skeletonCss || !browser) return;
  skeletonCss = true;
  const el = document.createElement('style');
  el.setAttribute('data-ascii-skeleton', '');
  el.textContent = SKELETON_CSS;
  document.head.appendChild(el);
}

/**
 * use:skeleton={{ lines: 3 }} — fills an empty, aria-hidden element with a glyph skeleton,
 * but only once the wait is long enough to be seen (`after`, default 280 ms): a quick load
 * never draws it, and the element holds no glyph text until then.
 */
export function skeleton(node: HTMLElement, spec: SkeletonSpec & { after?: number } = {}) {
  let stop = () => {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  const render = (s: SkeletonSpec & { after?: number }) => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      ensureSkeletonCss();
      stop();
      node.innerHTML = asciiSkeletonHTML({ variant: 'scan', ...s, css: false, animate: motionAllowed() && s.animate !== false });
      stop = animateSkeletons(node);
    }, s.after ?? 280);
  };
  render(spec);
  return {
    update: render,
    destroy() {
      clearTimeout(timer);
      stop();
      node.innerHTML = '';
    },
  };
}
