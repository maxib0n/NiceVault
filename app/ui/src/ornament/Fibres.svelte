<!--
  Security fibres embedded in the paper: a few visible red and blue fibres in
  daylight, sparse, and kept off the areas that carry text (`quiet`, fractions of
  the box). Suede has none (--fibre-alpha 0): nothing is drawn.
-->
<script lang="ts">
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { fibres } from './lathe';

  let {
    seed = 7,
    density = 1,
    quiet = [],
  }: { seed?: number; density?: number; quiet?: { x: number; y: number; w: number; h: number }[] } = $props();

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const alpha = env.num('--fibre-alpha', 0.4);
    if (alpha <= 0) return;
    const inks = [env.ink('var(--fibre-1)'), env.ink('var(--fibre-2)'), env.ink('var(--fibre-3)')];
    const count = Math.round(((w * h) / 9000) * density);
    const list = fibres(w, h, count, seed);
    ctx.save();
    ctx.globalAlpha = alpha;
    for (let i = 0; i < list.length; i++) {
      const f = list[i];
      const [x0, y0] = f.pts[0];
      // no fibre lands on the text: the quiet areas (grown by a fibre's reach) stay clean
      const m = 12;
      const inQuiet = quiet.some((q) => x0 >= q.x * w - m && x0 <= (q.x + q.w) * w + m && y0 >= q.y * h - m && y0 <= (q.y + q.h) * h + m);
      if (inQuiet) continue;
      ctx.beginPath();
      ctx.moveTo(f.pts[0][0], f.pts[0][1]);
      for (let j = 1; j < f.pts.length; j++) ctx.lineTo(f.pts[j][0], f.pts[j][1]);
      ctx.lineWidth = f.w;
      ctx.strokeStyle = inks[f.tone];
      ctx.stroke();
    }
    ctx.restore();
  }
</script>

<InkCanvas class="fibres-ink" {draw} />

<style>
  :global(.fibres-ink) {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
