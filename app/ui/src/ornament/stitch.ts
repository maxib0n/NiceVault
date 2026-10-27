export type Pt = [number, number];
export type Seg = [number, number, number, number];

interface Line {
  kind: 'line';
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  len: number;
}
interface Arc {
  kind: 'arc';
  cx: number;
  cy: number;
  r: number;
  a0: number;
  da: number;
  len: number;
}
type Piece = Line | Arc;

export class Track {
  readonly pieces: Piece[] = [];
  readonly closed: boolean;
  length = 0;

  constructor(closed: boolean) {
    this.closed = closed;
  }

  line(x0: number, y0: number, x1: number, y1: number): this {
    const len = Math.hypot(x1 - x0, y1 - y0);
    if (len > 1e-6) this.pieces.push({ kind: 'line', x0, y0, x1, y1, len });
    this.length += len;
    return this;
  }

  arc(cx: number, cy: number, r: number, a0: number, da: number): this {
    const len = Math.abs(da) * r;
    if (len > 1e-6) this.pieces.push({ kind: 'arc', cx, cy, r, a0, da, len });
    this.length += len;
    return this;
  }

  at(s: number): [number, number, number, number] {
    const L = this.length;
    if (this.closed) s = ((s % L) + L) % L;
    else s = Math.min(L, Math.max(0, s));
    for (const p of this.pieces) {
      if (s <= p.len || p === this.pieces[this.pieces.length - 1]) {
        const t = p.len > 0 ? Math.min(1, s / p.len) : 0;
        if (p.kind === 'line') {
          return [p.x0 + (p.x1 - p.x0) * t, p.y0 + (p.y1 - p.y0) * t, (p.x1 - p.x0) / p.len, (p.y1 - p.y0) / p.len];
        }
        const a = p.a0 + p.da * t;
        const dir = Math.sign(p.da) || 1;
        return [p.cx + p.r * Math.cos(a), p.cy + p.r * Math.sin(a), -Math.sin(a) * dir, Math.cos(a) * dir];
      }
      s -= p.len;
    }
    return [0, 0, 1, 0];
  }

  svg(): string {
    const f = (v: number) => (Math.round(v * 100) / 100).toString();
    let d = '';
    let first = true;
    for (const p of this.pieces) {
      if (p.kind === 'line') {
        d += `${first ? 'M' : 'L'}${f(p.x0)} ${f(p.y0)}L${f(p.x1)} ${f(p.y1)}`;
      } else {
        const sx = p.cx + p.r * Math.cos(p.a0);
        const sy = p.cy + p.r * Math.sin(p.a0);
        const ex = p.cx + p.r * Math.cos(p.a0 + p.da);
        const ey = p.cy + p.r * Math.sin(p.a0 + p.da);
        d += `${first ? 'M' : 'L'}${f(sx)} ${f(sy)}A${f(p.r)} ${f(p.r)} 0 ${Math.abs(p.da) > Math.PI ? 1 : 0} ${p.da > 0 ? 1 : 0} ${f(ex)} ${f(ey)}`;
      }
      first = false;
    }
    return this.closed && d ? d + 'Z' : d;
  }

  trace(path: Path2D | CanvasRenderingContext2D): void {
    let first = true;
    for (const p of this.pieces) {
      if (p.kind === 'line') {
        if (first) path.moveTo(p.x0, p.y0);
        path.lineTo(p.x1, p.y1);
      } else {
        const sx = p.cx + p.r * Math.cos(p.a0);
        const sy = p.cy + p.r * Math.sin(p.a0);
        if (first) path.moveTo(sx, sy);
        else path.lineTo(sx, sy);
        path.arc(p.cx, p.cy, p.r, p.a0, p.a0 + p.da, p.da < 0);
      }
      first = false;
    }
    if (this.closed) path.closePath();
  }
}

export type Radii = number | [number, number, number, number];

export function roundRect(x: number, y: number, w: number, h: number, radii: Radii): Track {
  const r = typeof radii === 'number' ? [radii, radii, radii, radii] : radii;
  const k = Math.min(1, w / Math.max(1e-6, Math.max(r[0] + r[1], r[3] + r[2])), h / Math.max(1e-6, Math.max(r[0] + r[3], r[1] + r[2])));
  const [tl, tr, br, bl] = r.map((v) => Math.max(0, v * k));
  const H = Math.PI / 2;
  return new Track(true)
    .line(x + tl, y, x + w - tr, y)
    .arc(x + w - tr, y + tr, tr, -H, H)
    .line(x + w, y + tr, x + w, y + h - br)
    .arc(x + w - br, y + h - br, br, 0, H)
    .line(x + w - br, y + h, x + bl, y + h)
    .arc(x + bl, y + h - bl, bl, H, H)
    .line(x, y + h - bl, x, y + tl)
    .arc(x + tl, y + tl, tl, Math.PI, H);
}

export function seam(x0: number, y0: number, x1: number, y1: number): Track {
  return new Track(false).line(x0, y0, x1, y1);
}

export interface StitchSpec {
  len: number;
  gap: number;
  slant?: number;
  margin?: number;
  start?: number;
}

export function stitches(track: Track, spec: StitchSpec): Seg[] {
  const L = track.length;
  const pitch0 = spec.len + spec.gap;
  if (L < pitch0 * 0.75) return [];
  const k = spec.len / pitch0;
  let n: number;
  let pitch: number;
  let s0: number;
  if (track.closed) {
    n = Math.max(3, Math.round(L / pitch0));
    pitch = L / n;
    const first = track.pieces[0];
    const mid = spec.start ?? (first ? first.len / 2 : 0);
    s0 = mid - (pitch * k) / 2;
  } else {
    const margin = spec.margin ?? spec.gap;
    const room = L - 2 * margin;
    n = Math.max(1, Math.floor((room + spec.gap) / pitch0));
    pitch = pitch0;
    s0 = margin + (room - (n * pitch - spec.gap)) / 2;
  }
  const len = track.closed ? pitch * k : spec.len;
  const slant = spec.slant ?? 0;
  const cs = Math.cos(slant);
  const sn = Math.sin(slant);
  const out: Seg[] = [];
  for (let i = 0; i < n; i++) {
    const a = s0 + i * pitch;
    const [x0, y0] = track.at(a);
    const [x1, y1] = track.at(a + len);
    if (!slant) {
      out.push([x0, y0, x1, y1]);
      continue;
    }
    const mx = (x0 + x1) / 2;
    const my = (y0 + y1) / 2;
    const hx = (x1 - x0) / 2;
    const hy = (y1 - y0) / 2;
    const rx = hx * cs - hy * sn;
    const ry = hx * sn + hy * cs;
    out.push([mx - rx, my - ry, mx + rx, my + ry]);
  }
  return out;
}

export function stitchPath(segs: Seg[], dx = 0, dy = 0, shrink = 0): string {
  const f = (v: number) => (Math.round(v * 100) / 100).toString();
  let d = '';
  for (const [x0, y0, x1, y1] of segs) {
    const ax = x0 + (x1 - x0) * shrink + dx;
    const ay = y0 + (y1 - y0) * shrink + dy;
    const bx = x1 - (x1 - x0) * shrink + dx;
    const by = y1 - (y1 - y0) * shrink + dy;
    d += `M${f(ax)} ${f(ay)}L${f(bx)} ${f(by)}`;
  }
  return d;
}
