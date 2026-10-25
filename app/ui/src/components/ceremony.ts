// The unlock ceremony and its quick reverse. Presentation only: Svelte transitions on the
// two screen roots (they share one grid cell; Sblocca lies above the open volume), each
// deciding at the instant the screens swap whether to play. No 3D, nothing turns over.
//   open  (Sblocca out → vault in, only right after a successful unlock), ~750 ms:
//         1. the rosette's two lathes, turning since the key went in, run on into register
//            and stop (~300 ms); the emblem rises in relief;
//         2. the Sblocca document breaks into the MRZ glyph grid in the volume's ink (the
//            glyph decode run in reverse) and fades away...
//         3. ...while, underneath, the open volume composes from glyphs in place: the
//            sidebar here, the file view through its own decode (FileView.svelte, keyed on
//            the ceremony). Both are held from their first frame, so there is never a frame
//            of their plain content before the glyphs, and never an empty frame: the
//            document only fades once the volume's surfaces are there below it.
//   close (vault out → Sblocca in, when a lock sends us back to Sblocca), ~300 ms: the same
//         in reverse, quicker: the volume breaks into glyphs as Sblocca fades in over it and
//         its document composes.
// Input is never held: the screen going away takes no pointer from the first frame.
// Everything else is instant. prefers-reduced-motion: a short crossfade.
// Cost: only transform and opacity move, as Web Animations on the elements that move (the two
// lathe canvases, the emblem and its pre-rendered shadow, the screen roots); nothing is set per
// frame on a screen root, so no frame restyles the whole tree.
import { fade as fadeVia } from '../lib/motion';
import type { TransitionConfig } from 'svelte/transition';
import { REGISTER_STEP } from '../ornament/Rosette.svelte';
import { MRZ } from '../lib/reveal';
import { app } from '../lib/stores/app.svelte';
import { vaults } from '../lib/stores/vaults.svelte';
import { createAsciiReveal, type AsciiRevealController } from '../vendor/ascii-reveal';

const OPEN_MS = 750;
const CLOSE_MS = 300;
const FADE_MS = 160;
/** when, into the open ceremony, the volume underneath starts composing (FileView reads it) */
export const COMPOSE_AT = 330;
/** frame-by-frame review: window.__nvRevealSlow = 8 plays the ceremony (and the decode) 8x slower */
const slow = () => (typeof window === 'undefined' ? 1 : Math.max(1, Number((window as unknown as { __nvRevealSlow?: number }).__nvRevealSlow) || 1));
const NONE: TransitionConfig = { duration: 0 };

let openedAt = -1e9;
let closedAt = -1e9;
const fresh = (at: number) => performance.now() - at < 160;

/** The open ceremony in progress, if any: its id and age in ms (the file view resolves inside it). */
export function ceremony(): { id: number; age: number; slow: number } | null {
  return openedAt < 0 ? null : { id: Math.round(openedAt), age: performance.now() - openedAt, slow: slow() };
}
const reduced = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

function fade(node: HTMLElement): TransitionConfig {
  return {
    duration: FADE_MS,
    tick: (t) => {
      node.style.opacity = t >= 1 ? '' : String(t);
    },
  };
}

/** The screen going away takes no input: the one arriving underneath gets the pointer at once. */
function letGo(node: HTMLElement) {
  node.style.pointerEvents = 'none';
  node.inert = true;
}

/** the repeated rows of the pages that break into glyphs: only those on screen do */
const ITEMS = '.file-item, [data-testid="pm-entry"]';

/** Glyphs over `el`: held at once (`instant`), or broken into the grid / composed out of it. */
function glyphs(el: Element | null, speed: number, opts: { media?: string; items?: string } = {}): AsciiRevealController | null {
  if (!(el instanceof HTMLElement)) return null;
  return createAsciiReveal(el, { preset: MRZ, speed, origin: 'top-left', lazy: false, fallback: true, media: opts.media ?? '[data-ascii="media"]', items: opts.items });
}

/** easeOutBack (overshoot 1.2) as a cubic Bézier: the lathes settle on their index */
const BACK = 'cubic-bezier(0.34, 1.42, 0.64, 1)';
const EASE_OUT = 'cubic-bezier(0.33, 1, 0.68, 1)';
const EASE_IN = 'cubic-bezier(0.11, 0, 0.5, 0)';

/** Sblocca root, out: lathes into register, then the document breaks into glyphs and fades. */
export function lidOpen(node: HTMLElement): TransitionConfig {
  if (vaults.unlock.phase !== 'success') return NONE;
  openedAt = performance.now();
  letGo(node);
  if (reduced()) return fade(node);
  const s = slow();
  const ms = OPEN_MS * s;
  const lathes = Array.from(node.querySelectorAll<HTMLElement>('.seal .lathe-a, .seal .lathe-b'));
  const from = Number.parseFloat(lathes[0]?.style.getPropertyValue('--lathe') ?? '') || 0;
  const to = Math.ceil((from + REGISTER_STEP * 0.6) / REGISTER_STEP) * REGISTER_STEP;
  const anims: Animation[] = [];
  // 1. the lathes run on into register (0 → 40 %)
  for (const el of lathes) {
    const k = el.classList.contains('lathe-b') ? -1 : 1;
    anims.push(el.animate([{ transform: `rotate(${from * k}deg)` }, { transform: `rotate(${to * k}deg)` }], { duration: ms * 0.4, easing: BACK, fill: 'forwards' }));
  }
  // the emblem rises in relief (22 → 45 %), its shadow (drawn once) fading in under it
  const emblem = node.querySelector<HTMLElement>('.seal .emblem');
  const shadow = node.querySelector<HTMLElement>('.seal .rise-shadow');
  const rise = { delay: ms * 0.22, duration: ms * 0.23, easing: EASE_OUT, fill: 'forwards' as const };
  if (emblem) anims.push(emblem.animate([{ transform: 'none' }, { transform: 'translateY(-1px) scale(1.035)' }], rise));
  if (shadow) anims.push(fadeVia(shadow, 0, 1, rise));
  // 3. the whole of Sblocca (board, document, its glyphs) fades over the composing volume (50 → 100 %)
  anims.push(fadeVia(node, 1, 0, { delay: ms * 0.5, duration: ms * 0.5, easing: EASE_IN, fill: 'forwards' }));
  node.dataset.ceremony = 'open';
  let encode: AsciiRevealController | null = null;
  const doc = node.querySelector('.document');
  // the document breaks into glyphs once the lathes have locked (~300 ms in)
  const timer = setTimeout(() => {
    encode = glyphs(doc, 2.1 / s, { media: '.seal canvas, .wm-window canvas' });
    void encode?.hide();
  }, 280 * s);
  return {
    duration: ms,
    tick: (t) => {
      if (t <= 0) {
        clearTimeout(timer);
        encode?.destroy();
        for (const a of anims) a.cancel();
      }
    },
  };
}

/** Vault root, in: the sidebar composes from glyphs (the file view composes through its own decode). */
export function interiorRise(node: HTMLElement): TransitionConfig {
  if (!fresh(openedAt)) return NONE;
  if (reduced()) return fade(node);
  const s = slow();
  // the cover and the page's head (the files compose through their own decode): held from the
  // first frame (VaultScreen's [data-compose] style; the page may mount a moment later), then
  // handed to the glyphs in one task, so no frame shows their plain text first
  node.dataset.compose = 'wait';
  const release = () => delete node.dataset.compose;
  setTimeout(() => {
    if (!node.isConnected) return;
    const parts = ['.sidebar', '.folder-head, .details, .passwords'].map((q) => glyphs(node.querySelector(q), 1.5 / s));
    for (const c of parts) void c?.hide({ instant: true });
    release();
    for (const c of parts) void c?.reveal().then(() => c.destroy());
  }, COMPOSE_AT * s);
  // failsafe: never left held
  setTimeout(release, (OPEN_MS + 400) * s);
  node.dataset.ceremony = 'open';
  return {
    duration: OPEN_MS * s,
    tick: (t) => {
      if (t >= 1) delete node.dataset.ceremony;
    },
  };
}

/** Vault root, out: the volume breaks into glyphs under the arriving Sblocca. */
export function interiorSink(node: HTMLElement): TransitionConfig {
  if (app.screen !== 'unlock') return NONE;
  closedAt = performance.now();
  letGo(node);
  if (reduced()) return fade(node);
  const s = slow();
  const page = node.querySelector('[role=listbox], .details, .passwords');
  const out = [glyphs(node.querySelector('.sidebar'), 3 / s), glyphs(page, 3 / s, { items: page?.matches('[role=listbox]') ? ITEMS : undefined })];
  for (const c of out) void c?.hide();
  node.dataset.ceremony = 'close';
  return {
    duration: CLOSE_MS * s,
    tick: (t) => {
      if (t <= 0) for (const c of out) c?.destroy();
    },
  };
}

/** Sblocca root, in: fades in over the volume while its document composes from glyphs. */
export function lidClose(node: HTMLElement): TransitionConfig {
  if (!fresh(closedAt)) return NONE;
  if (reduced()) return fade(node);
  const s = slow();
  const doc = glyphs(node.querySelector('.document'), 2.6 / s, { media: '.seal canvas, .wm-window canvas' });
  void doc?.hide({ instant: true });
  setTimeout(() => {
    if (!node.isConnected) return doc?.destroy();
    void doc?.reveal().then(() => doc.destroy());
  }, 90 * s);
  node.dataset.ceremony = 'close';
  const fadeIn = fadeVia(node, 0, 1, { duration: CLOSE_MS * 0.7 * s, easing: EASE_OUT });
  return {
    duration: CLOSE_MS * s,
    tick: (t) => {
      if (t >= 1) delete node.dataset.ceremony;
      if (t <= 0) fadeIn.cancel();
    },
  };
}
