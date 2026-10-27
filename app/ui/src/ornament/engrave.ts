export interface ToneMap {
  w: number;
  h: number;
  dark: Float32Array;
}

const cache = new Map<string, Promise<ToneMap | null>>();
const N = 128;

export function toneMap(src: string): Promise<ToneMap | null> {
  let p = cache.get(src);
  if (!p) {
    p = load(src);
    cache.set(src, p);
    if (cache.size > 24) cache.delete(cache.keys().next().value as string);
  }
  return p;
}

async function load(src: string): Promise<ToneMap | null> {
  try {
    const img = new Image();
    img.src = src;
    await img.decode();
    const c = document.createElement('canvas');
    c.width = c.height = N;
    const ctx = c.getContext('2d', { willReadFrequently: true });
    if (!ctx) return null;
    const side = Math.min(img.naturalWidth, img.naturalHeight);
    ctx.drawImage(img, (img.naturalWidth - side) / 2, (img.naturalHeight - side) / 2, side, side, 0, 0, N, N);
    const px = ctx.getImageData(0, 0, N, N).data;
    const lum = new Float32Array(N * N);
    for (let i = 0; i < N * N; i++) {
      const r = px[i * 4] / 255;
      const g = px[i * 4 + 1] / 255;
      const b = px[i * 4 + 2] / 255;
      lum[i] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    }
    const sorted = Float32Array.from(lum).sort();
    const lo = sorted[Math.floor(N * N * 0.02)];
    const hi = sorted[Math.floor(N * N * 0.98)];
    const span = Math.max(0.08, hi - lo);
    const dark = new Float32Array(N * N);
    for (let i = 0; i < N * N; i++) {
      const v = Math.min(1, Math.max(0, (lum[i] - lo) / span));
      dark[i] = Math.pow(1 - v, 1.15);
    }
    return { w: N, h: N, dark };
  } catch {
    return null;
  }
}

function sample(t: ToneMap, x: number, y: number): number {
  const fx = Math.min(t.w - 1.001, Math.max(0, x * (t.w - 1)));
  const fy = Math.min(t.h - 1.001, Math.max(0, y * (t.h - 1)));
  const x0 = Math.floor(fx);
  const y0 = Math.floor(fy);
  const ax = fx - x0;
  const ay = fy - y0;
  const i = y0 * t.w + x0;
  const d = t.dark;
  return (d[i] * (1 - ax) + d[i + 1] * ax) * (1 - ay) + (d[i + t.w] * (1 - ax) + d[i + t.w + 1] * ax) * ay;
}

export function lineScreen(path: Path2D, t: ToneMap, x: number, y: number, size: number, lines: number, minHair: number): void {
  const gap = size / lines;
  const maxHalf = gap * 0.47;
  const minHalf = Math.min(maxHalf, minHair / 2);
  const step = Math.max(0.6, size / 160);
  const n = Math.ceil(size / step);
  const top: [number, number][] = [];
  const bot: [number, number][] = [];
  for (let j = 0; j < lines; j++) {
    top.length = 0;
    bot.length = 0;
    const yc = y + (j + 0.5) * gap;
    for (let i = 0; i <= n; i++) {
      const xx = x + Math.min(size, i * step);
      const u = (xx - x) / size;
      const yw = yc + gap * 0.22 * Math.sin(u * 9.5 + j * 0.55);
      const v = Math.min(1, Math.max(0, (yw - y) / size));
      const d = sample(t, u, v);
      const half = minHalf + (maxHalf - minHalf) * d;
      top.push([xx, yw - half]);
      bot.push([xx, yw + half]);
    }
    path.moveTo(top[0][0], top[0][1]);
    for (let i = 1; i < top.length; i++) path.lineTo(top[i][0], top[i][1]);
    for (let i = bot.length - 1; i >= 0; i--) path.lineTo(bot[i][0], bot[i][1]);
    path.closePath();
  }
}
