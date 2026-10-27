<script lang="ts">
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { hair } from './ink.svelte';
  import { waves } from './lathe';

  let {
    around,
    seed = 3,
    fade = 0.72,
    measure,
  }: { around?: HTMLElement; seed?: number; fade?: number; measure?: unknown } = $props();

  function offsetIn(el: HTMLElement, root: HTMLElement): { x: number; y: number } {
    let x = 0;
    let y = 0;
    let n: HTMLElement | null = el;
    while (n && n !== root) {
      x += n.offsetLeft;
      y += n.offsetTop;
      n = n.offsetParent as HTMLElement | null;
    }
    if (n !== root) {
      const r = offsetIn(root, document.body);
      const e = offsetIn(el, document.body);
      return { x: e.x - r.x, y: e.y - r.y };
    }
    return { x, y };
  }

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    void measure;
    const hw = hair(env.dpr);
    const p = new Path2D();
    waves(p, -8, 0, w + 16, h, Math.max(3.2, w / 92), seed, Math.max(3.5, w / 64));
    const g = ctx.createLinearGradient(0, 0, w, h);
    g.addColorStop(0, env.ink('color-mix(in srgb, var(--engrave-2) 72%, transparent)'));
    g.addColorStop(0.5, env.ink('color-mix(in srgb, var(--engrave-2) 56%, transparent)'));
    g.addColorStop(1, env.ink('color-mix(in srgb, var(--iris) 62%, transparent)'));
    ctx.save();
    ctx.lineWidth = hw * 0.7;
    ctx.strokeStyle = g;
    ctx.stroke(p);
    ctx.restore();
    ctx.save();
    ctx.globalCompositeOperation = 'destination-in';
    const o = around ? offsetIn(around, env.el) : { x: w / 2, y: h / 2 };
    const fx = around ? o.x + around.offsetWidth / 2 : w / 2;
    const fy = around ? o.y + around.offsetHeight / 2 : h * 0.4;
    const reach = Math.max(w, h * fade) * 0.62;
    const f = ctx.createRadialGradient(fx, fy, reach * 0.45, fx, fy, reach);
    f.addColorStop(0, 'rgba(0,0,0,1)');
    f.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.fillStyle = f;
    ctx.fillRect(0, 0, w, h);
    ctx.restore();
    if (around) {
      const o = offsetIn(around, env.el);
      const cx = o.x + around.offsetWidth / 2;
      const cy = o.y + around.offsetHeight / 2;
      const r = around.offsetWidth / 2 + Math.max(6, w * 0.03);
      ctx.save();
      ctx.globalCompositeOperation = 'destination-out';
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
      ctx.save();
      ctx.lineWidth = hw * 0.7;
      ctx.strokeStyle = env.ink('color-mix(in srgb, var(--engrave-2) 70%, transparent)');
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }
  }
</script>

<InkCanvas class="tint-ink" {draw} />

<style>
  :global(.tint-ink) {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
