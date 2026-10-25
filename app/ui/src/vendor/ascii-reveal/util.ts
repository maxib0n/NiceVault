export const clamp = (x: number, a = 0, b = 1) => (x < a ? a : x > b ? b : x);
export const ease = (x: number) => 1 - Math.pow(1 - clamp(x), 3);
export const easeIO = (x: number) => {
  x = clamp(x);
  return x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2;
};

/** Small integer hash in [0, 1). Deterministic, so glyph flicker is stable per frame. */
export function hash(a: number, b: number, c: number): number {
  let h =
    Math.imul(a | 0, 0x27d4eb2d) ^
    Math.imul(((b | 0) + 0x9e3779b9) | 0, 0x85ebca6b) ^
    Math.imul(((c | 0) + 0x632be5ab) | 0, 0xc2b2ae35);
  h ^= h >>> 15;
  h = Math.imul(h, 0x2c1b3c6d);
  h ^= h >>> 12;
  h = Math.imul(h, 0x297a2d39);
  h ^= h >>> 15;
  return (h >>> 0) / 4294967296;
}

export const isBrowser = () => typeof window !== 'undefined' && typeof document !== 'undefined';
export const dpr = () => Math.min((isBrowser() && window.devicePixelRatio) || 1, 2);
export const reducedMotion = () =>
  isBrowser() && !!window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

/** Parse rgb(), rgba() and color(srgb …) strings into [r, g, b, a] (0–255, alpha 0–1). */
export function parseColor(s: string): [number, number, number, number] | null {
  if (!s) return null;
  s = s.trim();
  if (s[0] === '#') {
    let h = s.slice(1);
    if (h.length === 3 || h.length === 4) h = h.split('').map((x) => x + x).join('');
    const n = parseInt(h.slice(0, 6), 16);
    const a = h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1;
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, a];
  }
  const m = s.match(/^(rgba?|color)\(([^)]*)\)/i);
  if (!m) return null;
  const parts = m[2].replace(/srgb/i, '').split(/[\s,/]+/).filter(Boolean).map(parseFloat);
  if (m[1].toLowerCase() === 'color') {
    return [parts[0] * 255, parts[1] * 255, parts[2] * 255, parts[3] ?? 1];
  }
  return [parts[0], parts[1], parts[2], parts[3] ?? 1];
}

export const toRgba = (c: [number, number, number, number], a = c[3]) =>
  `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`;

/**
 * (NiceVault) The advance of each grapheme of a text laid out on one line: `advance(ch, first)` per
 * character (`first`: it starts a word), plus `letterSpacing`. Unless `pre`, white space collapses
 * as CSS does: none at either end of the line, one space for a run of it (a space's advance is
 * asked as ' ').
 */
export function lineAdvances(segs: string[], advance: (ch: string, first: boolean) => number, letterSpacing: number, pre: boolean): number[] {
  const isWs = (s: string) => /^\s+$/.test(s);
  let lastInk = segs.length - 1;
  while (!pre && lastInk >= 0 && isWs(segs[lastInk])) lastInk--;
  const out: number[] = [];
  let first = true, seenInk = false, prevWs = false;
  segs.forEach((seg, i) => {
    const ws = isWs(seg);
    out.push(ws && !pre && (!seenInk || prevWs || i > lastInk) ? 0 : advance(ws ? ' ' : seg, first) + letterSpacing);
    first = ws;
    prevWs = ws;
    if (!ws) seenInk = true;
  });
  return out;
}
