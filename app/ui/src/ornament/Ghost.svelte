<script lang="ts">
  import { imageSrc, initialOf, viewOf, type EmblemIconId, type EmblemView } from '../lib/emblem';
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { lineScreen, toneMap, type ToneMap } from './engrave';
  import { EMBLEM_GLYPHS, glyphCentre, glyphPath } from './glyphs';
  import { hair } from './ink.svelte';
  import { circle, hashString, petals } from './lathe';

  let { emblem, name, legend }: { emblem: EmblemView | null | undefined; name: string; legend: string } = $props();

  const v = $derived(viewOf(emblem));
  let src: HTMLSpanElement | undefined = $state();
  let photo = $state<ToneMap | null>(null);
  $effect(() => {
    const view = v;
    photo = null;
    if (view.kind !== 'image') return;
    let live = true;
    void toneMap(imageSrc(view.data_base64)).then((t) => {
      if (live) photo = t;
    });
    return () => {
      live = false;
    };
  });

  const N = 96;

  const markKey = $derived(v.kind === 'icon' ? `i:${v.id}` : v.kind === 'image' ? `p:${hashString(v.data_base64)}` : v.kind === 'initial' ? `l:${initialOf(name)}` : v.kind);

  function compose(t: ToneMap, rx: number, ry: number, side: number): ToneMap {
    const M = 128;
    const box = ry * 2;
    const dark = new Float32Array(M * M);
    const smooth = (a: number, b: number, x: number) => {
      const k = Math.min(1, Math.max(0, (x - a) / (b - a)));
      return k * k * (3 - 2 * k);
    };
    for (let j = 0; j < M; j++) {
      for (let i = 0; i < M; i++) {
        const dx = ((i + 0.5) / M - 0.5) * box;
        const dy = ((j + 0.5) / M - 0.5) * box;
        const u = dx / side + 0.5;
        const v = dy / side + 0.5;
        if (u < 0 || u > 1 || v < 0 || v > 1) continue;
        const e = Math.hypot(dx / rx, dy / ry);
        const c = Math.hypot(u - 0.5, v - 0.5) * 2;
        const fade = (1 - smooth(0.48, 1, c)) * (1 - smooth(0.66, 0.94, e));
        if (fade <= 0) continue;
        const x = Math.min(t.w - 1, Math.max(0, Math.round(u * (t.w - 1))));
        const y = Math.min(t.h - 1, Math.max(0, Math.round(v * (t.h - 1))));
        dark[j * M + i] = t.dark[y * t.w + x] * fade * 0.78;
      }
    }
    return { w: M, h: M, dark };
  }
  function markTone(paint: (c: CanvasRenderingContext2D) => void): ToneMap | null {
    const c = document.createElement('canvas');
    c.width = c.height = N;
    const x = c.getContext('2d', { willReadFrequently: true });
    if (!x) return null;
    x.fillStyle = '#000';
    x.strokeStyle = '#000';
    x.lineCap = 'round';
    x.lineJoin = 'round';
    paint(x);
    const px = x.getImageData(0, 0, N, N).data;
    const dark = new Float32Array(N * N);
    for (let i = 0; i < N * N; i++) dark[i] = px[i * 4 + 3] / 255;
    return { w: N, h: N, dark };
  }

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const view = v;
    let tone: ToneMap | null = null;
    if (view.kind === 'image') tone = photo;
    else if (view.kind === 'initial') {
      tone = markTone((c) => {
        const fs = N * 0.86;
        c.font = `600 ${fs}px "Newsreader", Georgia, serif`;
        c.textAlign = 'center';
        c.textBaseline = 'alphabetic';
        const letter = initialOf(name);
        const mt = c.measureText(letter);
        const asc = mt.actualBoundingBoxAscent || fs * 0.7;
        const desc = mt.actualBoundingBoxDescent || 0;
        const dx = mt.actualBoundingBoxRight ? ((mt.actualBoundingBoxLeft || 0) - mt.actualBoundingBoxRight) / 2 : 0;
        c.fillText(letter, N / 2 + dx, N / 2 + (asc - desc) / 2);
      });
    } else if (view.kind === 'icon') {
      const svg = src?.querySelector('svg');
      if (!svg) return;
      const geo = glyphPath(svg);
      const gc = glyphCentre(svg);
      const k = (N * 0.78) / 24;
      tone = markTone((c) => {
        c.translate(N / 2 - gc.x * k, N / 2 - gc.y * k);
        c.scale(k, k);
        c.lineWidth = 3.2;
        c.stroke(geo);
      });
    } else {
      tone = markTone((c) => {
        const p = new Path2D();
        petals(p, N / 2, N / 2, N * 0.44, { r0: 0.2, r1: 0.95, lines: 4, k: 9, lean: 1 });
        circle(p, N / 2, N / 2, N * 0.44);
        c.lineWidth = N * 0.035;
        c.stroke(p);
      });
    }
    if (!tone) return;

    const hw = hair(env.dpr);
    const rx = Math.min(w * 0.42, (h - 16) / 2.6);
    const ry = rx * 1.24;
    const cx = w / 2;
    const cy = h / 2;
    const box = ry * 2;
    const lines = Math.max(20, Math.round(box / (env.dpr >= 2 ? 2.3 : 2.8)));
    const field = compose(tone, rx, ry, rx * 1.42);
    const inkLine = env.ink('color-mix(in srgb, var(--engrave) 40%, transparent)');

    const layer = document.createElement('canvas');
    layer.width = Math.max(1, Math.round(w * env.dpr));
    layer.height = Math.max(1, Math.round(h * env.dpr));
    const l = layer.getContext('2d')!;
    l.setTransform(layer.width / w, 0, 0, layer.height / h, 0, 0);
    const p = new Path2D();
    lineScreen(p, field, cx - box / 2, cy - box / 2, box, lines, hw * 0.5);
    l.fillStyle = inkLine;
    l.fill(p);
    l.globalCompositeOperation = 'destination-in';
    l.save();
    l.translate(cx, cy);
    l.scale(1, ry / rx);
    const g = l.createRadialGradient(0, 0, rx * 0.86, 0, 0, rx);
    g.addColorStop(0, 'rgba(0,0,0,1)');
    g.addColorStop(1, 'rgba(0,0,0,0)');
    l.fillStyle = g;
    l.beginPath();
    l.arc(0, 0, rx, 0, Math.PI * 2);
    l.fill();
    l.restore();
    ctx.drawImage(layer, 0, 0, w, h);

    ctx.save();
    ctx.strokeStyle = env.ink('color-mix(in srgb, var(--engrave) 52%, transparent)');
    ctx.lineWidth = hw * 0.7;
    ctx.beginPath();
    ctx.ellipse(cx, cy, rx + 1.5, ry + 1.5, 0, 0, Math.PI * 2);
    ctx.stroke();
    const fs = env.dpr >= 2 ? 3.4 : 3.8;
    ctx.font = `500 ${fs}px "IBM Plex Mono", monospace`;
    ctx.fillStyle = env.ink('color-mix(in srgb, var(--engrave) 62%, transparent)');
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    const R = rx + 5;
    const RY = ry + 5;
    const perim = Math.PI * (3 * (R + RY) - Math.sqrt((3 * R + RY) * (R + 3 * RY)));
    const unit = ctx.measureText('M').width || fs * 0.6;
    const reps = Math.max(1, Math.round(perim / (unit * Math.max(1, legend.length))));
    const count = reps * legend.length;
    const adv = perim / count;
    const text = legend.repeat(reps);
    let t = -Math.PI / 2;
    for (let i = 0; i < count; i++) {
      const x = cx + R * Math.cos(t);
      const y = cy + RY * Math.sin(t);
      const dx = -R * Math.sin(t);
      const dy = RY * Math.cos(t);
      const speed = Math.hypot(dx, dy) || 1;
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(Math.atan2(dy, dx));
      ctx.fillText(text[i], 0, 0);
      ctx.restore();
      t += adv / speed;
    }
    ctx.restore();
  }
</script>

<span class="ghost" aria-hidden="true">
  <InkCanvas class="ghost-ink" {draw} cache="ghost|{markKey}|{photo ? 1 : 0}|{src ? 1 : 0}|{legend}" />
  {#if v.kind === 'icon'}
    {@const Icon = EMBLEM_GLYPHS[v.id as EmblemIconId]}
    <span class="src" bind:this={src}><Icon size={24} aria-hidden="true" /></span>
  {/if}
</span>

<style>
  .ghost {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
  }
  .ghost > :global(.ghost-ink) {
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
