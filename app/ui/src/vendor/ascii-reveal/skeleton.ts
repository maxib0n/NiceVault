/**
 * ASCII skeleton markup: glyph rows that paint on first render with zero JavaScript.
 * Patterns are deterministic, so server and client output match exactly (no hydration mismatch).
 *
 * Each row is a faint base pattern with a denser "wave" pattern stacked on top. A CSS mask sweeps a
 * band across the wave layer, so a scan of dense glyphs travels through the placeholder, like a
 * classic shimmer. Two wave patterns alternate in steps, so glyphs scramble inside the band.
 */
import { hash } from './util';

export type SkeletonVariant = 'dither' | 'scan' | 'braille' | 'terminal';

interface VariantSpec {
  base: string;
  wave: string;
  /** Share of empty cells. */
  gap: number;
  /** >1 favours the lighter glyphs of the ramp. */
  bias: number;
}

export const SKELETON_VARIANTS: Record<SkeletonVariant, VariantSpec> = {
  dither: { base: '░░░░▒', wave: '▒▓▓█▓▒', gap: 0, bias: 0.9 },
  scan: { base: '··:·:-', wave: '+=*#%@&$', gap: 0.08, bias: 1.6 },
  braille: { base: '⠢⠔⠑⠊⠡⠌⠒⠤⠕⠪', wave: '⣿⣷⣯⣟⡿⢿⣻⣽⣾⣶', gap: 0.02, bias: 1 },
  terminal: { base: '01', wave: '01', gap: 0.12, bias: 1 },
};

/** @deprecated kept for v0.3 compatibility; use SKELETON_VARIANTS. */
export const SKELETON_RAMP = SKELETON_VARIANTS.scan.base;

/*
 * How it stays cheap (measured, see scripts/skeleton-bench):
 * - The markup is a still glyph pattern: plain HTML + CSS, paints on the first frame with no JavaScript.
 * - The shimmer (a soft band sweeping each row, glyphs flickering inside it) is drawn by JavaScript on one small
 *   canvas per skeleton at ~30 fps. Each row's glyphs are rasterised once; a frame only copies the band.
 *   A CSS mask animation looks the same but makes the browser repaint every row on the main thread every frame
 *   (~3x the CPU on a 12-card page).
 * - Off-screen skeletons, hidden tabs and reduced motion don't animate at all.
 * - The CSS ships once per page (React 19 hoists <style href precedence>; or pass css: false and add SKELETON_CSS once).
 */
export const SKELETON_CSS = `
:where(.ars){position:relative;display:block;font-family:var(--ascii-font,ui-monospace,"SF Mono",Menlo,Consolas,monospace);color:var(--ascii-skeleton,currentColor);user-select:none;pointer-events:none;--ars-base:.3;--ars-wave:.9;--ars-band:45%;--ars-speed:1.8s}
.ars-line,.ars-block{position:relative;display:block;overflow:hidden;white-space:pre;line-height:1.3;contain:inline-size layout paint style}
.ars-line+.ars-line{margin-top:.55em}
.ars-block{line-height:1;border-radius:var(--ars-radius,8px);background:color-mix(in srgb,currentColor 6%,transparent)}
.ars-base{display:block;opacity:var(--ars-base)}
.ars>canvas{position:absolute;left:0;top:0;pointer-events:none}
.ars-dither{--ars-base:.22;--ars-wave:.75;--ars-band:55%}
.ars-dither .ars-line{line-height:1;letter-spacing:-.02em}
.ars-braille{--ars-base:.35;--ars-wave:1;--ars-band:40%}
.ars-scan{--ars-base:.28;--ars-wave:.95;--ars-band:38%}
.ars-terminal{--ars-base:.16;--ars-wave:1;--ars-band:35%;--ars-speed:1.5s;--ars-wave-color:var(--ascii-accent,currentColor)}`;

/** Glyphs per row: enough for ~900 px of text line or ~800 px of block at the default sizes. */
export const LINE_LEN = 120, BLOCK_LEN = 96;

/** A deterministic row of glyphs. */
export function glyphRow(seed: number, len: number, ramp = SKELETON_VARIANTS.scan.base, gap = 0.08, bias = 1.6): string {
  let out = '';
  for (let i = 0; i < len; i++) {
    out += hash(i, seed, 17) < gap ? ' ' : ramp[Math.floor(Math.pow(hash(i, seed, 29), bias) * ramp.length)];
  }
  return out;
}

/** The still glyph text for one skeleton row group (what the markup renders). */
export function skeletonBase(variant: SkeletonVariant, seed: number, rows: number, len: number): string {
  const v = SKELETON_VARIANTS[variant] || SKELETON_VARIANTS.dither;
  return Array.from({ length: rows }, (_, r) => glyphRow(seed + r, len, v.base, v.gap, v.bias)).join('\n');
}

/** Base text and the two alternating wave texts for one skeleton row group. */
export function skeletonLayers(variant: SkeletonVariant, seed: number, rows: number, len: number): [string, string, string] {
  const v = SKELETON_VARIANTS[variant] || SKELETON_VARIANTS.dither;
  const make = (ramp: string, off: number) =>
    Array.from({ length: rows }, (_, r) => glyphRow(seed + r + off, len, ramp, v.gap, v.bias)).join('\n');
  return [make(v.base, 0), make(v.wave, 300), make(v.wave, 600)];
}

export interface SkeletonSpec {
  /** Glyph style. Default `dither`. */
  variant?: SkeletonVariant;
  /** Number of text lines. Default 3. */
  lines?: number;
  /** Line widths (CSS). Defaults to a natural paragraph shape. */
  widths?: (string | number)[];
  /** Rows of glyphs for a block (image/avatar) skeleton. Overrides `lines`. */
  rows?: number;
  /** Seed for the glyph pattern. */
  seed?: number;
  /** Include the stylesheet. Default true; set false when you've added `SKELETON_CSS` to the page once. */
  css?: boolean;
  /** Shimmer. Default true. Set false for long lists: a still glyph pattern paints as fast as a gray box. */
  animate?: boolean;
}

export const SKELETON_WIDTHS = ['100%', '94%', '72%', '88%', '60%'];
const css = (v: string | number) => (typeof v === 'number' ? `${v}px` : v);
/** Stagger so lines sweep as one diagonal band. */
export const skeletonDelay = (i: number) => `${(i * 0.09).toFixed(2)}s`;

/** HTML string for non-React use: `el.innerHTML = asciiSkeletonHTML({ lines: 3 })`, then `animateSkeletons()` to start the shimmer. */
export function asciiSkeletonHTML(spec: SkeletonSpec = {}): string {
  const { variant = 'dither', lines = 3, widths = SKELETON_WIDTHS, rows, seed = 1, css: withCss = true, animate = true } = spec;
  const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const group = (i: number, cls: string, style: string, n: number) => {
    const b = skeletonBase(variant, seed * 97 + i * 13, n, rows ? BLOCK_LEN : LINE_LEN);
    return `<span class="${cls}" style="${style}"><span class="ars-base">${esc(b)}</span></span>`;
  };
  const body = rows
    ? group(0, 'ars-block', '', rows)
    : Array.from({ length: lines }, (_, i) => group(i, 'ars-line', `width:${css(widths[i % widths.length])};`, 1)).join('');
  return `${withCss ? `<style>${SKELETON_CSS}</style>` : ''}<span class="ars ars-${variant}" data-ascii-skeleton${animate ? ` data-ars-animate="${seed}"` : ''} aria-hidden="true">${body}</span>`;
}
