<!--
  A canvas for printed line work. It sizes its backing store to the device pixel
  grid (crisp hairlines at 1x and 2x), and redraws when its size, the theme, the
  accent, the pixel ratio or the fonts change. Purely decorative: aria-hidden.
  On a phone the sheet is engraved on a shared scratch canvas instead, kept as an
  encoded image and shown as this element's background (see SHEET_BYTES).
-->
<script lang="ts" module>
  import { isMobile } from '../lib/platform';
  export interface InkEnv {
    dpr: number;
    /** resolve a CSS colour expression in this canvas' context */
    ink: (expr: string) => string;
    /** numeric custom property in this canvas' context */
    num: (prop: string, fallback?: number) => number;
    el: HTMLCanvasElement;
  }
  export type Draw = (ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) => void;

  // Printed pixels kept per `cache` key (the caller's word for everything its draw depends on
  // besides size, pixel ratio and the inks it resolves): a sheet opened again (Impostazioni's
  // ink chips, Sblocca's seal and ghost) is copied back instead of engraved again. An entry
  // remembers the inks and numbers its draw read, and is used only while they resolve the same
  // in the new canvas' context (theme, accent, local tokens), and, if it set text, while no web
  // font has arrived since.
  interface Print {
    inks: [string, string][];
    nums: [string, number, number][];
    text: boolean;
    fontEpoch: number;
    px: HTMLCanvasElement;
  }
  const prints = new Map<string, Print>();
  const TEXT_CALLS = ['fillText', 'strokeText', 'measureText'] as const;
  let printBytes = 0;
  // A phone's web content process has a hard memory limit (iOS kills it, the page reloads):
  // 2026-10-25, every Sblocca on an iPhone killed it while its canvases at 3x and their prints
  // were alive next to Benvenuto's (device bisect: ink at 1x or none survived). Canvas backing
  // stores stay resident (and the compositor keeps a copy); an image's decoded pixels are
  // purgeable and shared per URL, its encoded bytes live outside the page. So there every sheet
  // is engraved at the full pixel ratio, one at a time, on one scratch canvas, encoded (PNG) and
  // shown as the element's background; the scratch gives its pixels back as soon as the queue
  // is empty, and the element itself never takes a context (no backing store at all).
  const PRINT_BYTES = 48 << 20;
  export const AS_IMAGE = isMobile;
  /** encoded sheets kept per cache key on a phone (bytes of PNG, not pixels) */
  const SHEET_BYTES = 24 << 20;
  /** a sheet's pixels on a phone: a page-sized one (Sblocca's frame and fibres) prints at 2x, not 3x */
  const SHEET_PX = 1 << 20;
  function sheetDpr(dpr: number, w: number, h: number): number {
    for (const d of [dpr, 3, 2, 1.5]) if (d <= dpr && w * h * d * d <= SHEET_PX) return d;
    return Math.min(dpr, 1);
  }
  const bytes = (c: HTMLCanvasElement) => c.width * c.height * 4;
  function release(c: HTMLCanvasElement) {
    c.width = 0;
    c.height = 0;
  }
  function drop(key: string) {
    const e = prints.get(key);
    if (!e) return;
    printBytes -= bytes(e.px);
    prints.delete(key);
    release(e.px);
  }
  function keep(key: string, p: Print) {
    drop(key);
    if (bytes(p.px) > PRINT_BYTES / 2) return release(p.px);
    prints.set(key, p);
    printBytes += bytes(p.px);
    // least recently used first
    for (const k of prints.keys()) {
      if (printBytes <= PRINT_BYTES) break;
      drop(k);
    }
  }

  interface Sheet {
    inks: [string, string][];
    nums: [string, number, number][];
    text: boolean;
    fontEpoch: number;
    url: string;
    size: number;
    /** elements showing it (or about to): its URL is revoked only once none is and it left the cache */
    refs: number;
    cached: boolean;
  }
  const sheets = new Map<string, Sheet>();
  let sheetBytes = 0;
  function unref(s: Sheet) {
    if (--s.refs <= 0 && !s.cached) URL.revokeObjectURL(s.url);
  }
  function forget(key: string) {
    const s = sheets.get(key);
    if (!s) return;
    sheets.delete(key);
    sheetBytes -= s.size;
    s.cached = false;
    if (s.refs <= 0) URL.revokeObjectURL(s.url);
  }
  function file(key: string, s: Sheet) {
    forget(key);
    if (s.size > SHEET_BYTES / 4) return;
    sheets.set(key, s);
    s.cached = true;
    sheetBytes += s.size;
    // least recently used first; sheets on screen stay
    for (const [k, e] of sheets) {
      if (sheetBytes <= SHEET_BYTES) break;
      if (e.refs <= 0) forget(k);
    }
  }
  /** Drops every kept sheet (on lock): those on screen keep their URL until they leave. */
  export function forgetSheets() {
    for (const k of [...sheets.keys()]) forget(k);
  }

  // One engraving at a time: the owner of the scratch canvas, and the sheets waiting for it.
  let scratch: HTMLCanvasElement | null = null;
  let owner: object | null = null;
  const waiting: { who: object; go: () => boolean }[] = [];
  function scratchFor(bw: number, bh: number): HTMLCanvasElement {
    const s = (scratch ??= document.createElement('canvas'));
    s.width = bw;
    s.height = bh;
    return s;
  }
  function next() {
    owner = null;
    while (waiting.length) {
      const w = waiting.shift()!;
      owner = w.who;
      if (w.go()) return;
      owner = null;
    }
    if (scratch) release(scratch);
    scratch = null;
  }
  function wait(who: object, go: () => boolean) {
    if (!waiting.some((w) => w.who === who)) waiting.push({ who, go });
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { ink, inkOf, installInk, numberOf } from './ink.svelte';

  let { draw, class: cls = '', cache }: { draw: Draw; class?: string; cache?: string } = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let w = $state(0);
  let h = $state(0);
  /** bumps when this sheet's turn on the scratch canvas comes (phones) */
  let turn = $state(0);
  const me = {};
  let alive = true;
  let encoding = false;
  let gen = 0;
  let shown: Sheet | null = null;
  let pending: Sheet | null = null;

  onMount(() => {
    installInk();
    const c = canvas!;
    const ro = new ResizeObserver(() => {
      const r = c.getBoundingClientRect();
      // layout size (untransformed): rotation/scale animations must not trigger redraws
      w = c.offsetWidth || r.width;
      h = c.offsetHeight || r.height;
    });
    ro.observe(c);
    return () => {
      alive = false;
      gen++;
      ro.disconnect();
      release(c);
      if (owner === me && !encoding) next();
      if (pending) unref(pending);
      if (shown) unref(shown);
      pending = shown = null;
    };
  });

  /** Engraves `draw` on `ctx` (a bw×bh backing store for w×h CSS px); what it read, for the caches. */
  function engrave(ctx: CanvasRenderingContext2D, c: HTMLCanvasElement, bw: number, bh: number, dpr: number) {
    ctx.setTransform(bw / w, 0, 0, bh / h, 0, 0);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    const inks: [string, string][] = [];
    const nums: [string, number, number][] = [];
    // text set on this canvas (it must redraw when the fonts arrive): seen through the calls
    let text = false;
    const own = ctx as unknown as Record<string, unknown>;
    // own properties defined on this context (the prototypes may be frozen)
    for (const f of TEXT_CALLS) {
      const orig = CanvasRenderingContext2D.prototype[f] as (...a: unknown[]) => unknown;
      Object.defineProperty(ctx, f, {
        configurable: true,
        writable: true,
        value(this: CanvasRenderingContext2D, ...a: unknown[]) {
          text = true;
          return orig.apply(this, a);
        },
      });
    }
    try {
      draw(ctx, w, h, {
        dpr,
        el: c,
        ink: (expr) => {
          const v = inkOf(c, expr);
          inks.push([expr, v]);
          return v;
        },
        num: (prop, fallback = 0) => {
          const v = numberOf(c, prop, fallback);
          nums.push([prop, fallback, v]);
          return v;
        },
      });
    } finally {
      for (const f of TEXT_CALLS) delete own[f];
    }
    if (text) void ink.fontEpoch;
    return { inks, nums, text };
  }

  const fresh = (c: HTMLCanvasElement, e: { text: boolean; fontEpoch: number; inks: [string, string][]; nums: [string, number, number][] }) =>
    (!e.text || e.fontEpoch === ink.fontEpoch) && e.inks.every(([x, v]) => inkOf(c, x) === v) && e.nums.every(([p, f, v]) => numberOf(c, p, f) === v);

  /** Shows `s` as the element's background once its pixels are decoded (the old one stays till then). */
  function show(c: HTMLCanvasElement, s: Sheet, px: string) {
    if (s === shown) {
      c.style.backgroundImage = `url("${s.url}")`;
      c.dataset.sheet = px;
      return;
    }
    if (s === pending) return;
    s.refs++;
    if (pending) unref(pending);
    pending = s;
    const im = new Image();
    im.src = s.url;
    im.decode()
      .catch(() => {})
      .then(() => {
        if (pending !== s) return;
        pending = null;
        c.style.backgroundImage = `url("${s.url}")`;
        c.dataset.inked = 'true';
        c.dataset.sheet = px;
        if (shown) unref(shown);
        shown = s;
      });
  }

  $effect(() => {
    void ink.epoch;
    void turn;
    const c = canvas;
    if (!AS_IMAGE || !c) return;
    // a phone: no backing store on the element, its intrinsic size its layout size (as at 1x)
    const yieldTurn = () => owner === me && !encoding && next();
    if (w < 1 || h < 1) return void yieldTurn();
    const dpr = sheetDpr(ink.dpr, w, h);
    const lw = Math.max(1, Math.round(w));
    const lh = Math.max(1, Math.round(h));
    if (c.width !== lw) c.width = lw;
    if (c.height !== lh) c.height = lh;
    const bw = Math.max(1, Math.round(w * dpr));
    const bh = Math.max(1, Math.round(h * dpr));
    // a sheet engraved for another box is not stretched over this one while the new one comes
    if (c.dataset.sheet && c.dataset.sheet !== `${bw}x${bh}`) c.style.backgroundImage = '';
    const key = cache === undefined ? null : `${cache}|${bw}x${bh}|${w}x${h}`;
    const hit = key === null ? undefined : sheets.get(key);
    if (hit && fresh(c, hit)) {
      // most recently used last
      sheets.delete(key!);
      sheets.set(key!, hit);
      gen++;
      show(c, hit, `${bw}x${bh}`);
      return void yieldTurn();
    }
    if (owner !== null && (owner !== me || encoding)) {
      wait(me, () => alive && (turn++, true));
      return;
    }
    owner = me;
    const g = ++gen;
    const s = scratchFor(bw, bh);
    const ctx = s.getContext('2d');
    if (!ctx) return void next();
    const read = engrave(ctx, c, bw, bh, dpr);
    const fontEpoch = ink.fontEpoch;
    encoding = true;
    s.toBlob((blob) => {
      encoding = false;
      next();
      if (!blob || !alive || g !== gen) return;
      const sheet: Sheet = { ...read, fontEpoch, url: URL.createObjectURL(blob), size: blob.size, refs: 0, cached: false };
      if (key !== null) file(key, sheet);
      show(c, sheet, `${bw}x${bh}`);
      // an uncached sheet nobody shows any more goes at once
      if (sheet.refs === 0 && !sheet.cached) URL.revokeObjectURL(sheet.url);
    }, 'image/png');
  });

  $effect(() => {
    void ink.epoch;
    const c = canvas;
    if (AS_IMAGE || !c || w < 1 || h < 1) return;
    const dpr = ink.dpr;
    const bw = Math.max(1, Math.round(w * dpr));
    const bh = Math.max(1, Math.round(h * dpr));
    if (c.width !== bw) c.width = bw;
    if (c.height !== bh) c.height = bh;
    const ctx = c.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, bw, bh);
    const key = cache === undefined ? null : `${cache}|${bw}x${bh}|${w}x${h}`;
    const hit = key === null ? undefined : prints.get(key);
    if (hit && fresh(c, hit)) {
      // most recently used last
      prints.delete(key!);
      prints.set(key!, hit);
      ctx.drawImage(hit.px, 0, 0);
      return;
    }
    const read = engrave(ctx, c, bw, bh, dpr);
    if (key !== null) {
      const px = document.createElement('canvas');
      px.width = bw;
      px.height = bh;
      px.getContext('2d')?.drawImage(c, 0, 0);
      keep(key, { ...read, fontEpoch: ink.fontEpoch, px });
    }
  });
</script>

<canvas bind:this={canvas} class="ink-canvas {cls}" aria-hidden="true"></canvas>

<style>
  .ink-canvas {
    display: block;
    pointer-events: none;
    /* a phone's sheet, shown as an image (AS_IMAGE) */
    background: no-repeat 0 0 / 100% 100%;
  }
</style>
