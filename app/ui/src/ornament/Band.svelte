<!--
  A running band: one guilloché run across its surface. Document chrome (a page head, a dialog
  head), never behind text. Printed in the ink on paper; pressed blind into anthracite (tone on
  tone), a touch of the ink in its groove. It runs edge to edge of the surface it is printed on,
  through the content's padding: the container says how far with --bleed-start / --bleed-end
  (its own inline padding); text keeps its margins, the print does not.
-->
<script lang="ts">
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { hair } from './ink.svelte';
  import { run } from './lathe';
  import { reliefOf, strokeInk } from './print';

  let { height = 9 }: { height?: number } = $props();

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const p = new Path2D();
    const b = h - 3;
    run(p, 0, h / 2, w, h / 2, b, 2, Math.max(9, b * 2.4));
    if (reliefOf(env) < 0) {
      strokeInk(ctx, p, env, hair(env.dpr) * 0.95, { finish: 'blind' });
      strokeInk(ctx, p, env, hair(env.dpr) * 0.6, { finish: 'flat', color: env.ink('color-mix(in srgb, var(--engrave) 34%, transparent)') });
    } else strokeInk(ctx, p, env, hair(env.dpr) * 0.8, { color: env.ink('color-mix(in oklab, var(--engrave) 58%, var(--paper))') });
  }
</script>

<span class="band" style:--h="{height}px" aria-hidden="true"><InkCanvas class="band-ink" {draw} cache="band" /></span>

<style>
  .band {
    display: block;
    height: var(--h);
    margin-inline: calc(-1 * var(--bleed-start, 0px)) calc(-1 * var(--bleed-end, 0px));
  }
  .band > :global(.band-ink) {
    width: 100%;
    height: 100%;
  }
</style>
