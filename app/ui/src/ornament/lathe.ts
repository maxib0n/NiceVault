export type Pt = [number, number];

export function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function hashString(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

const TAU = Math.PI * 2;

export interface RingSpec {
  rm: number;
  lines: number;
  k: number;
  a: number;
  k2?: number;
  a2?: number;
  p2?: number;
  dir?: 1 | -1;
}

export function ring(path: Path2D, cx: number, cy: number, R: number, s: RingSpec): void {
  const { rm, lines, k, a, k2 = 0, a2 = 0, p2 = 0, dir = 1 } = s;
  const samples = Math.min(2400, Math.max(360, Math.ceil(Math.max(k * 14, k2 * 8, R * 2.2))));
  for (let i = 0; i < lines; i++) {
    const phi = (dir * (i / lines) * TAU) / k;
    for (let j = 0; j <= samples; j++) {
      const t = (j / samples) * TAU;
      const r = R * (rm + a * Math.sin(k * t + k * phi) + a2 * Math.sin(k2 * t + k2 * phi + p2));
      const x = cx + r * Math.cos(t);
      const y = cy + r * Math.sin(t);
      if (j === 0) path.moveTo(x, y);
      else path.lineTo(x, y);
    }
    path.closePath();
  }
}

export interface PetalSpec {
  r0: number;
  r1: number;
  lines: number;
  k: number;
  lean: number;
}

export function petals(path: Path2D, cx: number, cy: number, R: number, s: PetalSpec): void {
  const { r0, r1, lines, k, lean } = s;
  const samples = Math.min(3000, Math.max(480, k * 24));
  const rm = (r0 + r1) / 2;
  const amp = (r1 - r0) / 2;
  for (let i = 0; i < lines; i++) {
    const phi = (i / lines) * (TAU / k);
    for (let j = 0; j <= samples; j++) {
      const t = (j / samples) * TAU;
      const u = k * (t + phi);
      const r = R * (rm + amp * Math.cos(u));
      const th = t + lean * Math.sin(u) / k;
      const x = cx + r * Math.cos(th);
      const y = cy + r * Math.sin(th);
      if (j === 0) path.moveTo(x, y);
      else path.lineTo(x, y);
    }
    path.closePath();
  }
}

export interface LoopSpec {
  rc: number;
  a: number;
  k: number;
  lines: number;
  squash?: number;
}

export function loops(path: Path2D, cx: number, cy: number, R: number, s: LoopSpec): void {
  const { rc, a, k, lines, squash = 1 } = s;
  const samples = Math.min(4000, Math.max(600, k * 36));
  for (let i = 0; i < lines; i++) {
    const phi = (i / lines) * TAU;
    for (let j = 0; j <= samples; j++) {
      const t = (j / samples) * TAU;
      const u = k * t + phi;
      const rr = R * (rc + a * squash * Math.cos(u));
      const tt = R * a * Math.sin(u);
      const x = cx + rr * Math.cos(t) - tt * Math.sin(t);
      const y = cy + rr * Math.sin(t) + tt * Math.cos(t);
      if (j === 0) path.moveTo(x, y);
      else path.lineTo(x, y);
    }
    path.closePath();
  }
}

export function rays(path: Path2D, cx: number, cy: number, R: number, r0: number, r1: number, n: number): void {
  for (let i = 0; i < n; i++) {
    const t = (i / n) * TAU;
    path.moveTo(cx + R * r0 * Math.cos(t), cy + R * r0 * Math.sin(t));
    path.lineTo(cx + R * r1 * Math.cos(t), cy + R * r1 * Math.sin(t));
  }
}

export function circle(path: Path2D, cx: number, cy: number, r: number): void {
  path.moveTo(cx + r, cy);
  path.arc(cx, cy, r, 0, TAU);
  path.closePath();
}

export function run(path: Path2D, x0: number, y0: number, x1: number, y1: number, h: number, lines: number, per: number, rules = true): void {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len = Math.hypot(dx, dy);
  if (len < 1) return;
  const ux = dx / len;
  const uy = dy / len;
  const nx = -uy;
  const ny = ux;
  const step = Math.max(0.75, per / 16);
  const n = Math.ceil(len / step);
  for (const dir of [1, -1]) {
    for (let i = 0; i < lines; i++) {
      const ph = (i / lines) * TAU;
      for (let j = 0; j <= n; j++) {
        const s = Math.min(len, j * step);
        const o = dir * (h / 2) * 0.86 * Math.sin((s / per) * TAU + ph);
        const x = x0 + ux * s + nx * o;
        const y = y0 + uy * s + ny * o;
        if (j === 0) path.moveTo(x, y);
        else path.lineTo(x, y);
      }
    }
  }
  if (rules) {
    for (const side of [-1, 1]) {
      path.moveTo(x0 + nx * side * (h / 2 + 1.2), y0 + ny * side * (h / 2 + 1.2));
      path.lineTo(x1 + nx * side * (h / 2 + 1.2), y1 + ny * side * (h / 2 + 1.2));
    }
  }
}

export function waves(path: Path2D, x: number, y: number, w: number, h: number, gap: number, seed: number, amp = 6): void {
  const r = prng(seed);
  const f1 = 40 + r() * 30;
  const f2 = 15 + r() * 12;
  const f3 = 260 + r() * 140;
  const p = r() * 10;
  const rows = Math.ceil(h / gap) + 6;
  for (let i = -3; i < rows; i++) {
    const y0 = y + i * gap;
    for (let xx = 0; xx <= w; xx += 3) {
      const yy = y0 + amp * Math.sin(xx / f1 + i * 0.19 + p) + amp * 0.55 * Math.sin(xx / f2 - i * 0.33) * Math.sin(xx / f3 + p);
      if (xx === 0) path.moveTo(x + xx, yy);
      else path.lineTo(x + xx, yy);
    }
  }
}

export interface Fibre {
  pts: Pt[];
  tone: 0 | 1 | 2;
  w: number;
}

export function fibres(w: number, h: number, count: number, seed: number): Fibre[] {
  const r = prng(seed);
  const out: Fibre[] = [];
  for (let i = 0; i < count; i++) {
    let x = r() * w;
    let y = r() * h;
    let a = r() * TAU;
    const len = 3 + r() * 7;
    const bend = (r() - 0.5) * 1.2;
    const steps = 6;
    const pts: Pt[] = [[x, y]];
    for (let j = 0; j < steps; j++) {
      a += bend / steps * 2;
      x += (Math.cos(a) * len) / steps;
      y += (Math.sin(a) * len) / steps;
      pts.push([x, y]);
    }
    out.push({ pts, tone: (i % 3) as 0 | 1 | 2, w: 0.42 + r() * 0.3 });
  }
  return out;
}

export function checkDigit(s: string): string {
  const w = [7, 3, 1];
  let sum = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    const v = c === '<' ? 0 : /[0-9]/.test(c) ? c.charCodeAt(0) - 48 : /[A-Z]/.test(c) ? c.charCodeAt(0) - 55 : 0;
    sum += v * w[i % 3];
  }
  return String(sum % 10);
}

export function mrzText(s: string): string {
  return s
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toUpperCase()
    .replace(/[^A-Z0-9]+/g, '<')
    .replace(/^<+|<+$/g, '');
}

export function pad(s: string, n: number): string {
  return (s + '<'.repeat(n)).slice(0, n);
}
