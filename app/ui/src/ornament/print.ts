import type { InkEnv } from './InkCanvas.svelte';

export type Finish = 'intaglio' | 'blind' | 'foil' | 'flat';

export function reliefOf(env: InkEnv): number {
  return env.num('--ink-relief', 1) < 0 ? -1 : 1;
}

export function impress(
  ctx: CanvasRenderingContext2D,
  env: InkEnv,
  finish: Finish,
  paint: (ctx: CanvasRenderingContext2D) => void,
  color?: string,
): void {
  const pass = (style: string | CanvasGradient, dx: number, dy: number) => {
    ctx.save();
    ctx.translate(dx, dy);
    ctx.strokeStyle = style;
    ctx.fillStyle = style;
    paint(ctx);
    ctx.restore();
  };
  if (finish === 'foil') {
    pass('rgba(0,0,0,0.45)', -0.5, -0.6);
    pass('rgba(255,255,255,0.13)', 0.45, 0.55);
    pass(foilGradient(ctx, env), 0, 0);
    return;
  }
  const r = reliefOf(env);
  if (finish === 'intaglio' || finish === 'blind') {
    if (r > 0) {
      pass(env.ink('var(--relief-hi)'), -0.42, -0.42);
      pass(env.ink('color-mix(in srgb, color-mix(in oklab, var(--engrave) 50%, #000) 36%, transparent)'), 0.48, 0.62);
    } else {
      pass(env.ink('var(--relief-lo)'), -0.45, -0.55);
      pass(env.ink('var(--relief-hi)'), 0.5, 0.62);
    }
    if (finish === 'blind') {
      pass(env.ink(r > 0 ? 'var(--relief-hi)' : 'var(--press-floor)'), 0, 0);
      return;
    }
  }
  const ink = color ?? env.ink('var(--engrave)');
  ctx.save();
  ctx.strokeStyle = ink;
  ctx.fillStyle = ink;
  paint(ctx);
  ctx.restore();
}

export function strokeInk(
  ctx: CanvasRenderingContext2D,
  path: Path2D,
  env: InkEnv,
  width: number,
  opts: { finish?: Finish; color?: string } = {},
): void {
  ctx.lineWidth = width;
  impress(ctx, env, opts.finish ?? 'intaglio', (c) => c.stroke(path), opts.color);
}

export function foilGradient(ctx: CanvasRenderingContext2D, env: InkEnv): CanvasGradient {
  const w = env.el.offsetWidth || 100;
  const h = env.el.offsetHeight || 100;
  const g = ctx.createLinearGradient(0, 0, w * 0.4, h);
  g.addColorStop(0, env.ink('var(--foil-hi)'));
  g.addColorStop(0.3, env.ink('var(--foil)'));
  g.addColorStop(0.55, env.ink('var(--foil-mid)'));
  g.addColorStop(0.72, env.ink('var(--foil)'));
  g.addColorStop(1, env.ink('var(--foil-lo)'));
  return g;
}

export function mask(env: InkEnv, w: number, h: number, build: (m: CanvasRenderingContext2D) => void): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(w * env.dpr));
  c.height = Math.max(1, Math.round(h * env.dpr));
  const m = c.getContext('2d')!;
  m.setTransform(c.width / w, 0, 0, c.height / h, 0, 0);
  m.lineCap = 'round';
  m.lineJoin = 'round';
  m.strokeStyle = '#000';
  m.fillStyle = '#000';
  build(m);
  return c;
}

function tint(m: HTMLCanvasElement, style: string | CanvasGradient, w: number, h: number): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = m.width;
  c.height = m.height;
  const t = c.getContext('2d')!;
  t.drawImage(m, 0, 0);
  t.globalCompositeOperation = 'source-in';
  t.setTransform(c.width / w, 0, 0, c.height / h, 0, 0);
  t.fillStyle = style;
  t.fillRect(0, 0, w, h);
  return c;
}

export function release(c: HTMLCanvasElement): void {
  c.width = 0;
  c.height = 0;
}

export function printMask(ctx: CanvasRenderingContext2D, m: HTMLCanvasElement, env: InkEnv, w: number, h: number, finish: Finish, color?: string): void {
  printMaskOnce(ctx, m, env, w, h, finish, color);
  release(m);
}

function printMaskOnce(ctx: CanvasRenderingContext2D, m: HTMLCanvasElement, env: InkEnv, w: number, h: number, finish: Finish, color?: string): void {
  const put = (c: HTMLCanvasElement, dx: number, dy: number) => {
    ctx.drawImage(c, dx, dy, w, h);
    release(c);
  };
  if (finish === 'foil') {
    put(tint(m, 'rgba(0,0,0,0.5)', w, h), -0.55, -0.65);
    put(tint(m, 'rgba(255,255,255,0.14)', w, h), 0.5, 0.6);
    const g = ctx.createLinearGradient(0, 0, w * 0.45, h);
    g.addColorStop(0, env.ink('var(--foil-hi)'));
    g.addColorStop(0.32, env.ink('var(--foil)'));
    g.addColorStop(0.58, env.ink('var(--foil-mid)'));
    g.addColorStop(0.76, env.ink('var(--foil)'));
    g.addColorStop(1, env.ink('var(--foil-lo)'));
    put(tint(m, g, w, h), 0, 0);
    return;
  }
  const r = reliefOf(env);
  if (finish === 'intaglio' || finish === 'blind') {
    if (r > 0) {
      put(tint(m, env.ink('var(--relief-hi)'), w, h), -0.45, -0.45);
      put(tint(m, env.ink('color-mix(in srgb, color-mix(in oklab, var(--engrave) 50%, #000) 38%, transparent)'), w, h), 0.5, 0.65);
    } else {
      put(tint(m, env.ink('var(--relief-lo)'), w, h), -0.5, -0.6);
      put(tint(m, env.ink('var(--relief-hi)'), w, h), 0.5, 0.65);
    }
    if (finish === 'blind') {
      put(tint(m, env.ink(r > 0 ? 'var(--relief-hi)' : 'var(--press-floor)'), w, h), 0, 0);
      return;
    }
  }
  const ink = color ?? env.ink('var(--engrave)');
  put(tint(m, ink, w, h), 0, 0);
}

export function stitchRow(
  ctx: CanvasRenderingContext2D,
  env: InkEnv,
  pts: [number, number][],
  opts: { closed?: boolean; len?: number; gap?: number; width?: number; color?: string; slant?: number } = {},
): void {
  const alpha = env.num('--stitch-alpha', 0);
  if (alpha <= 0 || pts.length < 2) return;
  const len = opts.len ?? 5;
  const gap = opts.gap ?? 3;
  const wid = opts.width ?? 1.35;
  const thread = opts.color ?? env.ink('var(--stitch)');
  const segs: [number, number, number, number][] = [];
  const path = opts.closed ? [...pts, pts[0]] : pts;
  let carry = gap / 2;
  for (let i = 0; i < path.length - 1; i++) {
    const [x0, y0] = path[i];
    const [x1, y1] = path[i + 1];
    const L = Math.hypot(x1 - x0, y1 - y0);
    if (L < 1) continue;
    const ux = (x1 - x0) / L;
    const uy = (y1 - y0) / L;
    let t = carry;
    while (t + len <= L) {
      segs.push([x0 + ux * t, y0 + uy * t, x0 + ux * (t + len), y0 + uy * (t + len)]);
      t += len + gap;
    }
    carry = Math.max(0, t - L);
  }
  const slant = opts.slant ?? 0.22;
  const cs = Math.cos(slant);
  const sn = Math.sin(slant);
  const line = (style: string, w: number, dx: number, dy: number, shrink = 0) => {
    ctx.strokeStyle = style;
    ctx.lineWidth = w;
    ctx.beginPath();
    for (const [a, b, c, d] of segs) {
      const mx = (a + c) / 2;
      const my = (b + d) / 2;
      const hx = ((c - a) / 2) * (1 - shrink * 2);
      const hy = ((d - b) / 2) * (1 - shrink * 2);
      const rx = hx * cs - hy * sn;
      const ry = hx * sn + hy * cs;
      ctx.moveTo(mx - rx + dx, my - ry + dy);
      ctx.lineTo(mx + rx + dx, my + ry + dy);
    }
    ctx.stroke();
  };
  ctx.save();
  ctx.globalAlpha = alpha;
  ctx.lineCap = 'round';
  ctx.beginPath();
  ctx.moveTo(path[0][0], path[0][1]);
  for (const [x, y] of path.slice(1)) ctx.lineTo(x, y);
  ctx.lineWidth = wid * 1.6;
  ctx.strokeStyle = env.ink('var(--relief-lo)');
  ctx.globalAlpha = alpha * 0.35;
  ctx.stroke();
  ctx.globalAlpha = alpha;
  line('rgba(0,0,0,0.6)', wid, 0.5, 0.7);
  line(thread, wid, 0, 0);
  line('rgba(255,255,255,0.26)', wid * 0.4, -0.3, -0.34, 0.16);
  ctx.restore();
}
