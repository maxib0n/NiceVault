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
export const COMPOSE_AT = 330;
const slow = () => (typeof window === 'undefined' ? 1 : Math.max(1, Number((window as unknown as { __nvRevealSlow?: number }).__nvRevealSlow) || 1));
const NONE: TransitionConfig = { duration: 0 };

let openedAt = -1e9;
let closedAt = -1e9;
const fresh = (at: number) => performance.now() - at < 160;

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

function letGo(node: HTMLElement) {
  node.style.pointerEvents = 'none';
  node.inert = true;
}

const ITEMS = '.file-item, [data-testid="pm-entry"]';

function glyphs(el: Element | null, speed: number, opts: { media?: string; items?: string } = {}): AsciiRevealController | null {
  if (!(el instanceof HTMLElement)) return null;
  return createAsciiReveal(el, { preset: MRZ, speed, origin: 'top-left', lazy: false, fallback: true, media: opts.media ?? '[data-ascii="media"]', items: opts.items });
}

const BACK = 'cubic-bezier(0.34, 1.42, 0.64, 1)';
const EASE_OUT = 'cubic-bezier(0.33, 1, 0.68, 1)';
const EASE_IN = 'cubic-bezier(0.11, 0, 0.5, 0)';

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
  for (const el of lathes) {
    const k = el.classList.contains('lathe-b') ? -1 : 1;
    anims.push(el.animate([{ transform: `rotate(${from * k}deg)` }, { transform: `rotate(${to * k}deg)` }], { duration: ms * 0.4, easing: BACK, fill: 'forwards' }));
  }
  const emblem = node.querySelector<HTMLElement>('.seal .emblem');
  const shadow = node.querySelector<HTMLElement>('.seal .rise-shadow');
  const rise = { delay: ms * 0.22, duration: ms * 0.23, easing: EASE_OUT, fill: 'forwards' as const };
  if (emblem) anims.push(emblem.animate([{ transform: 'none' }, { transform: 'translateY(-1px) scale(1.035)' }], rise));
  if (shadow) anims.push(fadeVia(shadow, 0, 1, rise));
  anims.push(fadeVia(node, 1, 0, { delay: ms * 0.5, duration: ms * 0.5, easing: EASE_IN, fill: 'forwards' }));
  node.dataset.ceremony = 'open';
  let encode: AsciiRevealController | null = null;
  const doc = node.querySelector('.document');
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

export function interiorRise(node: HTMLElement): TransitionConfig {
  if (!fresh(openedAt)) return NONE;
  if (reduced()) return fade(node);
  const s = slow();
  node.dataset.compose = 'wait';
  const release = () => delete node.dataset.compose;
  setTimeout(() => {
    if (!node.isConnected) return;
    const parts = ['.sidebar', '.folder-head, .details, .passwords'].map((q) => glyphs(node.querySelector(q), 1.5 / s));
    for (const c of parts) void c?.hide({ instant: true });
    release();
    for (const c of parts) void c?.reveal().then(() => c.destroy());
  }, COMPOSE_AT * s);
  setTimeout(release, (OPEN_MS + 400) * s);
  node.dataset.ceremony = 'open';
  return {
    duration: OPEN_MS * s,
    tick: (t) => {
      if (t >= 1) delete node.dataset.ceremony;
    },
  };
}

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
