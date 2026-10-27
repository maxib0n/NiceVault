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

function pace(speed: number): number {
  const slow = browser ? Number((window as unknown as { __nvRevealSlow?: number }).__nvRevealSlow) || 1 : 1;
  return speed / Math.max(1, slow);
}

export function motionAllowed(): boolean {
  return browser && typeof matchMedia === 'function' && !matchMedia('(prefers-reduced-motion: reduce)').matches;
}

let lastKey = -1e9;
let lastPointer = -1e9;
if (browser) {
  window.addEventListener(
    'keydown',
    (e) => {
      lastKey = e.key === 'Enter' ? -1e9 : performance.now();
    },
    { capture: true, passive: true },
  );
  window.addEventListener('pointerdown', () => (lastPointer = performance.now()), { capture: true, passive: true });
  window.addEventListener('scroll', () => (lastScroll = performance.now()), { capture: true, passive: true });
}
let lastScroll = -1e9;
function scrolling(ms = 200): boolean {
  return performance.now() - lastScroll < ms;
}
export function fromKeyboard(ms = 1000): boolean {
  return lastKey > lastPointer && performance.now() - lastKey < ms;
}

const seen = new Set<string>();
export function firstTime(key: string): boolean {
  if (seen.has(key)) return false;
  seen.add(key);
  return true;
}

export interface RevealLogEntry {
  t: number;
  kind: 'decode' | 'media' | 'compose';
  key: string;
  what: 'play' | 'done' | 'reduced' | 'keyboard' | 'cached' | 'small' | 'late' | 'offscreen' | 'scrolling' | 'merged' | 'shown' | 'large';
}
export function revealLog(kind: RevealLogEntry['kind'], key: string, what: RevealLogEntry['what']): void {
  if (!browser) return;
  const w = window as unknown as { __nvReveal?: RevealLogEntry[] };
  const log = (w.__nvReveal ??= []);
  log.push({ t: Math.round(performance.now()), kind, key, what });
  if (log.length > 200) log.splice(0, log.length - 200);
}

function fontsReady(cap = 400): Promise<void> {
  if (!browser || !document.fonts || document.fonts.status === 'loaded') return Promise.resolve();
  return Promise.race([document.fonts.ready.then(() => undefined), new Promise<void>((r) => setTimeout(r, cap))]);
}

const PENDING = 'data-nv-pending';
const WAIT_CAP = 2500;
const START_CAP = 800;

export interface DecodeOptions {
  key: string;
  also?: string;
  ready?: boolean;
  speed?: number;
  delay?: number;
  origin?: Origin;
  selector?: string;
  fades?: string;
  surfaces?: string;
  media?: string;
  pointerOnly?: boolean;
  items?: string;
}

export function decode(node: HTMLElement, opts: DecodeOptions) {
  let ctrl: AsciiRevealController | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let alive = true;

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
    failsafe = setTimeout(() => requestAnimationFrame(() => {
      if (mine !== pending) return;
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
      if (willPlay(o) && !node.hasAttribute(PENDING)) pend(WAIT_CAP, o);
      return;
    }
    firstTime(o.key);
    if (o.also) firstTime(o.also);
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
export const LARGE_PX = 8_000_000;
let arrivals: MediaWait[] = [];
let arrivalsRaf = 0;

function flushArrivals() {
  arrivalsRaf = 0;
  const batch = arrivals;
  arrivals = [];
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
    if (w.node instanceof HTMLImageElement && w.node.naturalWidth * w.node.naturalHeight > LARGE_PX) return { host: h, what: 'large' };
    if (!onScreen) return { host: h, what: 'offscreen' };
    if (busy) return { host: h, what: 'scrolling' };
    if (adoptPicture(w.node)) return { host: h, what: 'merged' };
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

function fadeIn(node: HTMLElement) {
  node.animate?.([{ opacity: 0 }, { opacity: 1 }], { duration: 220 / pace(1), easing: 'ease-out' });
}

export function decodeMedia(node: HTMLImageElement | HTMLVideoElement, opts: { speed?: number; min?: number; key?: string } = {}) {
  const key = opts.key ?? '';
  let alive = true;
  const plain = (what: RevealLogEntry['what']) => {
    revealLog('media', key, what);
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

let skeletonCss = false;
function ensureSkeletonCss() {
  if (skeletonCss || !browser) return;
  skeletonCss = true;
  const el = document.createElement('style');
  el.setAttribute('data-ascii-skeleton', '');
  el.textContent = SKELETON_CSS;
  document.head.appendChild(el);
}

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
