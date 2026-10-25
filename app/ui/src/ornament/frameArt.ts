// The frames an emblem can be set in, cut like the rosette (ornament/lathe.ts): deterministic
// line work built in code, three plates per frame. The fixed plate carries the frame's own
// figure (the wreath, the cord, the key, the seal's edge, the rules); the two lathe plates carry
// fine engraved rings that turn while a key is derived and lock into register (Rosette.svelte,
// ceremony.ts), so every repeat on them divides 360 / 18 degrees and a ring at rest after any
// whole number of register steps looks exactly as it did before.
// Pure geometry: no DOM, no randomness.
import { circle, loops, petals, rays } from './lathe';
import type { FrameId } from './frames';
import type { RosetteKind } from './Rosette.svelte';

const TAU = Math.PI * 2;

/** One plate: lines stroked at `weight` x the hairline, and (laurel) shapes filled. */
export interface Plate {
  lines: Path2D;
  weight: number;
  /** finer line work on the same plate */
  fine?: Path2D;
  fineWeight?: number;
  fill?: Path2D;
}

export interface FrameArt {
  fixed: Plate;
  a?: Plate;
  b?: Plate;
}

interface Geo {
  cx: number;
  cy: number;
  R: number;
  /** the clear centre (emblem), fraction of R */
  hole: number;
  kind: RosetteKind;
}

/** How much detail the size can carry: 0 (a swatch, ~30 px) .. 1 (Sblocca's seal). */
function detail(R: number): number {
  return Math.max(0, Math.min(1, (R - 14) / 110));
}

const at = (g: Geo, r: number, a: number): [number, number] => [g.cx + g.R * r * Math.cos(a), g.cy + g.R * r * Math.sin(a)];

/** A polyline in polar coordinates (radius fraction, angle). */
function polar(p: Path2D, g: Geo, pts: [number, number][], close = false): void {
  pts.forEach(([r, a], i) => {
    const [x, y] = at(g, r, a);
    if (i === 0) p.moveTo(x, y);
    else p.lineTo(x, y);
  });
  if (close) p.closePath();
}

/** A tangential run at radius r from angle a0 to a1, sampled finely enough to read as an arc. */
function arcRun(g: Geo, r: number, a0: number, a1: number): [number, number][] {
  const n = Math.max(2, Math.ceil((Math.abs(a1 - a0) * g.R * r) / 2));
  return Array.from({ length: n + 1 }, (_, i) => [r, a0 + ((a1 - a0) * i) / n] as [number, number]);
}

/**
 * The fine rings on the two lathe plates, between the clear centre and `inner` (where the frame's
 * figure begins), as the rosette cuts them: petals on lathe B next to the emblem, a looped lace
 * on lathe A outside them. Every repeat divides 20 degrees.
 */
function lathes(g: Geo, inner: number): { a: Plate; b: Plate } {
  const d = detail(g.R);
  const lo = g.hole + 0.03;
  const hi = inner - 0.025;
  const mid = lo + (hi - lo) * 0.55;
  const half = ((hi - mid) / 2) * 0.86;
  const a = new Path2D();
  loops(a, g.cx, g.cy, g.R, { rc: (mid + hi) / 2, a: half / 1.1, k: d > 0.35 ? 36 : 18, lines: d > 0.35 ? 2 : 1, squash: 1.1 });
  const b = new Path2D();
  petals(b, g.cx, g.cy, g.R, { r0: lo, r1: mid, lines: d > 0.35 ? 3 : 2, k: 18, lean: 1 });
  return { a: { lines: a, weight: 0.85 }, b: { lines: b, weight: 0.8 } };
}

// ---- laurel ----------------------------------------------------------------------------

/** One leaf: a pointed oval from its base (on the stem) along `dir`, `len` long, `w` wide; its midrib, and its shade (fine lines along the lower half). */
function leaf(out: Path2D, rib: Path2D, shade: Path2D | null, x: number, y: number, dir: number, len: number, w: number): void {
  const ux = Math.cos(dir);
  const uy = Math.sin(dir);
  const nx = -uy;
  const ny = ux;
  const tx = x + ux * len;
  const ty = y + uy * len;
  // the widest point a little before the middle, like a bay leaf
  const mx = x + ux * len * 0.42;
  const my = y + uy * len * 0.42;
  out.moveTo(x, y);
  out.quadraticCurveTo(mx + nx * w * 1.05, my + ny * w * 1.05, tx, ty);
  out.quadraticCurveTo(mx - nx * w * 1.05, my - ny * w * 1.05, x, y);
  out.closePath();
  rib.moveTo(x + ux * len * 0.06, y + uy * len * 0.06);
  rib.lineTo(x + ux * len * 0.84, y + uy * len * 0.84);
  if (!shade) return;
  // two short hatches in the half away from the light
  for (const k of [0.3, 0.55]) {
    const ox = nx * w * k;
    const oy = ny * w * k;
    shade.moveTo(x + ux * len * (0.2 + k * 0.1) - ox, y + uy * len * (0.2 + k * 0.1) - oy);
    shade.lineTo(x + ux * len * (0.72 - k * 0.2) - ox, y + uy * len * (0.72 - k * 0.2) - oy);
  }
}

function laurel(g: Geo): FrameArt {
  const d = detail(g.R);
  const out = new Path2D();
  const ribs = new Path2D();
  const shade = d > 0.55 ? new Path2D() : null;
  const stem = new Path2D();
  const rStem = 0.81;
  const leafLen = 0.2 - 0.025 * d;
  const leafW = leafLen * 0.36;
  // two branches whose stems cross at the bottom, open at the top
  const pairs = Math.round(8 + 11 * d);
  const start = Math.PI / 2 - 0.13; // just past the crossing (bottom = +90 degrees, y down)
  const end = Math.PI * 1.5 - 0.32; // short of the top
  for (const side of [1, -1]) {
    // the left branch runs clockwise from the bottom, the right one mirrors it
    const ang = (t: number) => (side > 0 ? start + (end - start) * t : Math.PI - (start + (end - start) * t));
    polar(stem, g, arcRun(g, rStem, ang(0), ang(1)));
    const fwdOf = (a: number) => a + (side > 0 ? Math.PI / 2 : -Math.PI / 2);
    for (let i = 0; i < pairs; i++) {
      for (const o of [1, -1]) {
        // the outer and inner leaves alternate along the branch, like a real sprig
        const t = (i + (o > 0 ? 0.55 : 0.95)) / (pairs + 0.6);
        if (t > 0.97) continue;
        const a = ang(t);
        const [x, y] = at(g, rStem, a);
        const k = 1 - 0.3 * t; // the leaves shorten towards the tip
        leaf(out, ribs, shade, x, y, fwdOf(a) - 0.5 * o * side, g.R * leafLen * k, g.R * leafW * k);
      }
    }
    // the sprig ends in a leaf on the branch's line
    const a1 = ang(1);
    const [tx, ty] = at(g, rStem, a1);
    leaf(out, ribs, shade, tx, ty, fwdOf(a1), g.R * leafLen * 0.62, g.R * leafW * 0.62);
  }
  // where the stems cross: a short binding, wound twice
  const [kx, ky] = at(g, rStem, Math.PI / 2);
  const bw = g.R * 0.05;
  const bh = g.R * 0.022;
  for (const dy of [-bh * 0.55, bh * 0.55]) {
    out.moveTo(kx - bw, ky + dy - bh / 2);
    out.lineTo(kx + bw, ky + dy - bh / 2);
    out.lineTo(kx + bw, ky + dy + bh / 2);
    out.lineTo(kx - bw, ky + dy + bh / 2);
    out.closePath();
  }
  const fixed = new Path2D();
  fixed.addPath(stem);
  fixed.addPath(out);
  const fine = new Path2D();
  if (d > 0.25) fine.addPath(ribs);
  if (shade) fine.addPath(shade);
  circle(fine, g.cx, g.cy, g.R * 0.645);
  circle(fixed, g.cx, g.cy, g.R * g.hole);
  const l = lathes(g, 0.645);
  return { fixed: { lines: fixed, weight: 1, fine, fineWeight: 0.75 }, a: l.a, b: l.b };
}

// ---- rope (a twisted cord) ---------------------------------------------------------------

function rope(g: Geo): FrameArt {
  const d = detail(g.R);
  const rIn = 0.8;
  const rOut = 0.985;
  const lines = new Path2D();
  const fine = new Path2D();
  circle(lines, g.cx, g.cy, g.R * rOut);
  circle(lines, g.cx, g.cy, g.R * rIn);
  // each strand crosses the band on a slant, an S between the two rules
  const n = 18 * Math.max(2, Math.round((g.R * 0.9) / 18 / (2.3 + 1.2 * (1 - d))));
  const slant = (TAU / n) * 2.3;
  for (let i = 0; i < n; i++) {
    const a = (i / n) * TAU;
    const pts: [number, number][] = [];
    for (let j = 0; j <= 10; j++) {
      const t = j / 10;
      // eased across the band: the strand rolls over the cord's crown
      const e = t * t * (3 - 2 * t);
      pts.push([rIn + (rOut - rIn) * t, a + slant * e]);
    }
    polar(lines, g, pts);
    if (d > 0.3) {
      // the shade in the groove under each strand
      polar(
        fine,
        g,
        pts.map(([r, aa]) => [r, aa + (TAU / n) * 0.32] as [number, number]),
      );
    }
  }
  // a beaded rule inside the cord
  const beads = d > 0.2 ? 72 : 36;
  for (let i = 0; i < beads; i++) {
    const [x, y] = at(g, 0.745, (i / beads) * TAU);
    circle(fine, x, y, g.R * (0.009 + 0.004 * (1 - d)));
  }
  circle(lines, g.cx, g.cy, g.R * g.hole);
  const l = lathes(g, 0.72);
  return { fixed: { lines, weight: 1, fine, fineWeight: 0.7 }, a: l.a, b: l.b };
}

// ---- meander (the Greek key) ---------------------------------------------------------------

function meander(g: Geo): FrameArt {
  const d = detail(g.R);
  const rIn = 0.79;
  const rOut = 0.985;
  const lines = new Path2D();
  const fine = new Path2D();
  circle(lines, g.cx, g.cy, g.R * rOut);
  circle(lines, g.cx, g.cy, g.R * rIn);
  const n = d > 0.5 ? 36 : 18;
  const band = rOut - rIn;
  // one key per unit: a squared spiral rising from the inner rule (u along, v across the band)
  const KEY: [number, number][] = [
    [0.12, 0.08],
    [0.12, 0.86],
    [0.84, 0.86],
    [0.84, 0.3],
    [0.4, 0.3],
    [0.4, 0.62],
    [0.6, 0.62],
  ];
  const inner = 0.1;
  for (let i = 0; i < n; i++) {
    const a0 = (i / n) * TAU;
    const da = TAU / n;
    const pts: [number, number][] = [];
    // the baseline from the previous key to this one, then the key
    pts.push(...arcRun(g, rIn + band * inner, a0 - da * 0.12, a0 + da * KEY[0][0]));
    for (let k = 1; k < KEY.length; k++) {
      const [u0, v0] = KEY[k - 1];
      const [u1, v1] = KEY[k];
      if (v0 === v1) pts.push(...arcRun(g, rIn + band * v0, a0 + da * u0, a0 + da * u1).slice(1));
      else pts.push([rIn + band * v1, a0 + da * u1]);
    }
    polar(lines, g, pts);
  }
  // a fine double rule on each side of the band
  circle(fine, g.cx, g.cy, g.R * (rOut - 0.02));
  circle(fine, g.cx, g.cy, g.R * (rIn - 0.025));
  circle(lines, g.cx, g.cy, g.R * g.hole);
  const l = lathes(g, rIn - 0.025);
  return { fixed: { lines, weight: 1, fine, fineWeight: 0.65 }, a: l.a, b: l.b };
}

// ---- starburst (a notarial seal) -----------------------------------------------------------

function starburst(g: Geo): FrameArt {
  const d = detail(g.R);
  const lines = new Path2D();
  const fine = new Path2D();
  // the seal's serrated edge, like a foil seal cut by a die
  const teeth = d > 0.4 ? 54 : 36;
  const edge: [number, number][] = [];
  for (let i = 0; i < teeth * 2; i++) edge.push([i % 2 ? 0.915 : 0.99, (i / (teeth * 2)) * TAU]);
  polar(lines, g, edge, true);
  circle(lines, g.cx, g.cy, g.R * 0.86);
  circle(fine, g.cx, g.cy, g.R * 0.83);
  circle(lines, g.cx, g.cy, g.R * g.hole);
  // the burst: fine rays between the rules turn on lathe A
  const a = new Path2D();
  rays(a, g.cx, g.cy, g.R, g.hole + 0.06, 0.8, d > 0.4 ? 144 : 72);
  circle(a, g.cx, g.cy, g.R * (g.hole + 0.05));
  const b = new Path2D();
  petals(b, g.cx, g.cy, g.R, { r0: g.hole + 0.03, r1: g.hole + 0.16, lines: 2, k: 18, lean: 1 });
  return { fixed: { lines, weight: 1, fine, fineWeight: 0.7 }, a: { lines: a, weight: 0.55 }, b: { lines: b, weight: 0.75 } };
}

// ---- hairline (plain rules, a pearl ring) ---------------------------------------------------

function hairline(g: Geo): FrameArt {
  const d = detail(g.R);
  const lines = new Path2D();
  const fine = new Path2D();
  circle(lines, g.cx, g.cy, g.R * 0.985);
  circle(fine, g.cx, g.cy, g.R * 0.95);
  circle(lines, g.cx, g.cy, g.R * g.hole);
  circle(fine, g.cx, g.cy, g.R * (g.hole + 0.035));
  // a ring of pearls on lathe A, fine ticks on lathe B
  const a = new Path2D();
  const pearls = d > 0.3 ? 72 : 36;
  for (let i = 0; i < pearls; i++) {
    const [x, y] = at(g, 0.86, (i / pearls) * TAU);
    circle(a, x, y, g.R * (0.011 + 0.006 * (1 - d)));
  }
  const b = new Path2D();
  rays(b, g.cx, g.cy, g.R, 0.7, 0.73, d > 0.3 ? 144 : 72);
  return { fixed: { lines, weight: 1, fine, fineWeight: 0.7 }, a: { lines: a, weight: 0.8 }, b: { lines: b, weight: 0.6 } };
}

/** The plates of `frame` (not the guilloché rosette, which Rosette.svelte cuts itself). */
export function frameArt(frame: Exclude<FrameId, 'guilloche'>, kind: RosetteKind, cx: number, cy: number, R: number, hole: number): FrameArt {
  const g: Geo = { cx, cy, R, hole, kind };
  switch (frame) {
    case 'laurel':
      return laurel(g);
    case 'rope':
      return rope(g);
    case 'meander':
      return meander(g);
    case 'starburst':
      return starburst(g);
    case 'hairline':
      return hairline(g);
  }
}
