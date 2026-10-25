<!--
  The document's frame: a guilloché run on each side, inset from the cut edge by a
  clean margin, joined at the corners by small rosette medallions; inside it, a
  microtext rule (the "line" is text, legible only under a loupe). On paper it is
  printed in the ink; on anthracite the run is pressed in blind (tone on tone) and the
  medallions are inked in their recess. Fills its positioned parent.
  The card's edge is saddle-stitched by the page (docStitch, sewn over the foil stripe): its
  needle line turns each corner round the medallion, concentric with it, an even gap all round.
-->
<script lang="ts" module>
  import { roundRect, type Track } from './stitch';

  /** The frame's measures for a document `w` px wide (one source for the print and the stitching). */
  export function frameGeom(w: number) {
    const m = Math.round(Math.max(12, w * 0.0135));
    const b = Math.max(5, w * 0.0068);
    const rc = b;
    const ring = rc + 1.2;
    const c = m + b / 2; // centre line of the band, and of the corner medallions
    // the stitching: inset from the cut edge, its corners round each medallion with a clear gap
    const s = Math.max(3.5, Math.min(Math.max(4.5, m * 0.42), c - ring - 2.6));
    return { m, b, rc, ring, c, s, corner: c - s };
  }

  /** The needle line round the document's edge (Stitch track=...). */
  export function docStitch(w: number, h: number): Track {
    const g = frameGeom(w);
    return roundRect(g.s, g.s, w - 2 * g.s, h - 2 * g.s, g.corner);
  }

  /** Stitch pitch on a document `w` px wide. */
  export function docPitch(w: number): { len: number; gap: number } {
    return { len: Math.max(4.6, w * 0.0039), gap: Math.max(2.4, w * 0.002) };
  }
</script>

<script lang="ts">
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { hair } from './ink.svelte';
  import { circle, petals, run } from './lathe';
  import { reliefOf, strokeInk } from './print';

  let { micro = 'NICEVAULT · VOLUME CIFRATO · ' }: { micro?: string } = $props();

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const hw = hair(env.dpr);
    const pressed = reliefOf(env) < 0;
    const { m, b, rc, ring, c } = frameGeom(w);
    const per = Math.max(9, b * 2.6);
    const runs = new Path2D();
    const lines = 2;
    const band = env.ink('color-mix(in oklab, var(--engrave) 82%, var(--paper))');
    const gapEnd = ring + 0.6;
    run(runs, c + gapEnd, c, w - c - gapEnd, c, b, lines, per);
    run(runs, c + gapEnd, h - c, w - c - gapEnd, h - c, b, lines, per);
    run(runs, c, c + gapEnd, c, h - c - gapEnd, b, lines, per);
    run(runs, w - c, c + gapEnd, w - c, h - c - gapEnd, b, lines, per);
    strokeInk(ctx, runs, env, hw * (pressed ? 1 : 0.8), pressed ? { finish: 'blind' } : { color: band });

    const med = new Path2D();
    for (const [x, y] of [
      [c, c],
      [w - c, c],
      [c, h - c],
      [w - c, h - c],
    ]) {
      circle(med, x, y, ring);
      petals(med, x, y, rc, { r0: 0.22, r1: 0.92, lines: 3, k: 6, lean: 1 });
    }
    strokeInk(ctx, med, env, hw * 0.85, { color: pressed ? env.ink('var(--engrave)') : band });

    // microtext rule just inside the band
    const inset = m + b + Math.max(4, b * 0.5);
    const fs = env.dpr >= 2 ? 3.4 : 3.8;
    ctx.save();
    ctx.font = `500 ${fs}px "IBM Plex Mono", monospace`;
    ctx.textBaseline = 'middle';
    ctx.fillStyle = env.ink(pressed ? 'color-mix(in srgb, var(--engrave) 46%, transparent)' : 'color-mix(in srgb, var(--engrave) 62%, transparent)');
    const unit = ctx.measureText(micro).width || 60;
    const side = (x: number, y: number, len: number, rot: number) => {
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(rot);
      ctx.beginPath();
      ctx.rect(0, -fs, len, fs * 2);
      ctx.clip();
      for (let s = 0; s < len; s += unit) ctx.fillText(micro, s, 0);
      ctx.restore();
    };
    side(inset, inset, w - inset * 2, 0);
    side(w - inset, inset, h - inset * 2, Math.PI / 2);
    side(w - inset, h - inset, w - inset * 2, Math.PI);
    side(inset, h - inset, h - inset * 2, -Math.PI / 2);
    ctx.restore();
  }
</script>

<InkCanvas class="frame-ink" {draw} cache="frame|{micro}" />

<style>
  :global(.frame-ink) {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
