<script lang="ts">
  import { imageSrc, initialOf, viewOf, type EmblemIconId, type EmblemView } from '../lib/emblem';
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { toneMap, type ToneMap } from './engrave';
  import { EMBLEM_GLYPHS, glyphCentre, glyphPath } from './glyphs';
  import { circle, petals, ring } from './lathe';
  import { reliefOf } from './print';

  let { emblem, name }: { emblem: EmblemView | null | undefined; name: string } = $props();

  const v = $derived(viewOf(emblem));
  let src: HTMLSpanElement | undefined = $state();
  let tone = $state<ToneMap | null>(null);
  $effect(() => {
    const view = v;
    tone = null;
    if (view.kind !== 'image') return;
    let live = true;
    void toneMap(imageSrc(view.data_base64)).then((t) => {
      if (live) tone = t;
    });
    return () => {
      live = false;
    };
  });

  function layer(env: InkEnv, w: number, h: number, paint: (c: CanvasRenderingContext2D) => void): HTMLCanvasElement {
    const c = document.createElement('canvas');
    c.width = Math.max(1, Math.round(w * env.dpr));
    c.height = Math.max(1, Math.round(h * env.dpr));
    const x = c.getContext('2d')!;
    x.setTransform(c.width / w, 0, 0, c.height / h, 0, 0);
    x.lineCap = 'round';
    x.lineJoin = 'round';
    paint(x);
    return c;
  }

  function soften(src: HTMLCanvasElement, k: number): HTMLCanvasElement {
    let cur = src;
    for (let i = 0; i < 2; i++) {
      const small = document.createElement('canvas');
      small.width = Math.max(1, Math.round(cur.width / k));
      small.height = Math.max(1, Math.round(cur.height / k));
      const s = small.getContext('2d')!;
      s.imageSmoothingQuality = 'high';
      s.drawImage(cur, 0, 0, small.width, small.height);
      const big = document.createElement('canvas');
      big.width = src.width;
      big.height = src.height;
      const b = big.getContext('2d')!;
      b.imageSmoothingQuality = 'high';
      b.drawImage(small, 0, 0, big.width, big.height);
      cur = big;
    }
    return cur;
  }

  function tinted(m: HTMLCanvasElement, color: string): HTMLCanvasElement {
    const c = document.createElement('canvas');
    c.width = m.width;
    c.height = m.height;
    const t = c.getContext('2d')!;
    t.drawImage(m, 0, 0);
    t.globalCompositeOperation = 'source-in';
    t.fillStyle = color;
    t.fillRect(0, 0, c.width, c.height);
    return c;
  }

  function pressed(ctx: CanvasRenderingContext2D, height: HTMLCanvasElement, env: InkEnv, w: number, h: number, wall: number) {
    const W = height.width;
    const H = height.height;
    const src = height.getContext('2d')!.getImageData(0, 0, W, H).data;
    const a = new Float32Array(W * H);
    for (let i = 0; i < a.length; i++) a[i] = src[i * 4 + 3] / 255;
    const lit = new ImageData(W, H);
    const shade = new ImageData(W, H);
    const k = wall * 1.3;
    for (let y = 1; y < H - 1; y++) {
      for (let x = 1; x < W - 1; x++) {
        const i = y * W + x;
        const s = -(a[i + 1] - a[i - 1] + a[i + W] - a[i - W]) * k;
        if (s > 0) lit.data[i * 4 + 3] = Math.min(255, s * 255);
        else shade.data[i * 4 + 3] = Math.min(255, -s * 255);
      }
    }
    const put = (img: ImageData, color: string) => {
      const c = document.createElement('canvas');
      c.width = W;
      c.height = H;
      c.getContext('2d')!.putImageData(img, 0, 0);
      ctx.drawImage(tinted(c, color), 0, 0, w, h);
    };
    const alpha = env.num('--wm-alpha', 1);
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.drawImage(tinted(height, env.ink('var(--wm-body)')), 0, 0, w, h);
    put(shade, env.ink('var(--wm-shade)'));
    put(lit, env.ink('var(--wm-light)'));
    ctx.restore();
  }

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const view = v;
    const deboss = reliefOf(env) < 0;
    const S = Math.min(w, h);
    const cx = w / 2;
    const cy = h / 2;
    let motif: HTMLCanvasElement | null = null;
    if (view.kind === 'initial') {
      const fs = S * 0.92;
      motif = layer(env, w, h, (c) => {
        c.font = `600 ${fs}px "Newsreader", Georgia, serif`;
        c.textAlign = 'center';
        c.textBaseline = 'alphabetic';
        const letter = initialOf(name);
        const mt = c.measureText(letter);
        const asc = mt.actualBoundingBoxAscent || fs * 0.7;
        const desc = mt.actualBoundingBoxDescent || 0;
        c.fillStyle = '#000';
        const dx = ((mt.actualBoundingBoxLeft || 0) - (mt.actualBoundingBoxRight || 0)) / 2;
        c.fillText(letter, cx + (mt.actualBoundingBoxRight ? dx : 0), cy + (asc - desc) / 2);
      });
    } else if (view.kind === 'icon') {
      const svg = src?.querySelector('svg');
      if (!svg) return;
      const geo = glyphPath(svg);
      const k = (S * 0.82) / 24;
      const gc = glyphCentre(svg);
      motif = layer(env, w, h, (c) => {
        c.translate(cx - gc.x * k, cy - gc.y * k);
        c.scale(k, k);
        c.lineWidth = 3.4;
        c.strokeStyle = '#000';
        c.stroke(geo);
      });
    } else if (view.kind === 'image') {
      const t = tone;
      if (!t) return;
      const r = S * 0.47;
      const ovalMask = (alphaOf: (d: number) => number, k: number) => {
        const img = new ImageData(t.w, t.h);
        for (let i = 0; i < t.w * t.h; i++) img.data[i * 4 + 3] = Math.round(alphaOf(t.dark[i]) * 255);
        const small = document.createElement('canvas');
        small.width = t.w;
        small.height = t.h;
        small.getContext('2d')!.putImageData(img, 0, 0);
        const m = layer(env, w, h, (c) => {
          c.save();
          c.translate(cx, cy);
          c.scale(0.84, 1);
          const g = c.createRadialGradient(0, 0, r * 0.5, 0, 0, r);
          g.addColorStop(0, 'rgba(0,0,0,1)');
          g.addColorStop(1, 'rgba(0,0,0,0)');
          c.fillStyle = g;
          c.beginPath();
          c.arc(0, 0, r, 0, Math.PI * 2);
          c.fill();
          c.restore();
          c.globalCompositeOperation = 'source-in';
          c.drawImage(small, cx - r, cy - r, r * 2, r * 2);
        });
        return soften(m, k);
      };
      if (deboss) {
        const k = Math.max(2, S / 90);
        pressed(ctx, ovalMask((d) => d * 0.85, k), env, w, h, k);
        return;
      }
      const oval = (alphaOf: (d: number) => number, color: string) => tinted(ovalMask(alphaOf, Math.max(2, S / 70)), color);
      const alpha = env.num('--wm-alpha', 1);
      ctx.save();
      ctx.globalAlpha = alpha;
      ctx.drawImage(oval((d) => d * 0.9, env.ink(reliefOf(env) < 0 ? 'var(--wm-body)' : 'var(--wm-shade)')), 0, 0, w, h);
      ctx.globalAlpha = alpha * 0.55;
      ctx.drawImage(oval((d) => 1 - d, env.ink('var(--wm-light)')), 0, 0, w, h);
      ctx.restore();
      return;
    } else {
      motif = layer(env, w, h, (c) => {
        const R = S * 0.46;
        const p = new Path2D();
        circle(p, cx, cy, R);
        petals(p, cx, cy, R, { r0: 0.25, r1: 0.95, lines: 6, k: 10, lean: 1 });
        ring(p, cx, cy, R, { rm: 0.2, lines: 3, k: 10, a: 0.05 });
        c.lineWidth = S * 0.025;
        c.strokeStyle = '#000';
        c.stroke(p);
      });
    }
    if (!motif) return;
    if (deboss) {
      const k = Math.max(2, S / 60);
      pressed(ctx, soften(motif, k), env, w, h, k);
      return;
    }
    const soft = soften(motif, Math.max(3, S / 34));
    const d = Math.max(1, S / 120);
    const light = tinted(soft, env.ink('var(--wm-light)'));
    const shade = tinted(soft, env.ink('var(--wm-shade)'));
    const body = tinted(soft, env.ink('var(--wm-body)'));
    const alpha = env.num('--wm-alpha', 1);
    const r = reliefOf(env);
    ctx.save();
    ctx.globalAlpha = alpha * 0.55;
    ctx.drawImage(body, 0, 0, w, h);
    const rim = (src: HTMLCanvasElement, dx: number, dy: number) => {
      const c = document.createElement('canvas');
      c.width = src.width;
      c.height = src.height;
      const x = c.getContext('2d')!;
      const s = c.width / w;
      x.drawImage(src, dx * s, dy * s);
      x.globalCompositeOperation = 'destination-out';
      x.drawImage(soft, -dx * s, -dy * s);
      return c;
    };
    ctx.globalAlpha = alpha;
    ctx.drawImage(rim(shade, d * r, d * 1.2 * r), 0, 0, w, h);
    ctx.drawImage(rim(light, -d * r, -d * 1.2 * r), 0, 0, w, h);
    ctx.restore();
  }
</script>

<span class="watermark" aria-hidden="true">
  <InkCanvas class="wm-ink" {draw} />
  {#if v.kind === 'icon'}
    {@const Icon = EMBLEM_GLYPHS[v.id as EmblemIconId]}
    <span class="src" bind:this={src}><Icon size={24} aria-hidden="true" /></span>
  {/if}
</span>

<style>
  .watermark {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
  }
  .watermark > :global(.wm-ink) {
    width: 100%;
    height: 100%;
  }
  .src {
    position: absolute;
    left: 0;
    top: 0;
    width: 24px;
    height: 24px;
    overflow: hidden;
    visibility: hidden;
    pointer-events: none;
  }
</style>
