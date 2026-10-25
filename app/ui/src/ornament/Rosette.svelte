<!--
  A guilloché rosette, cut by two lathes. The fixed rings print on one plate;
  lathe A (the petals) and lathe B (the inner lace) print on their own plates
  so the unlock can turn them in opposite directions and bring them back into
  register (CSS: --lathe, in degrees, on any ancestor). The centre is a clear
  cartouche for the emblem (children).
  `frame` (a volume's emblem only, ornament/frames.ts): the guilloché rosette (default), or another
  frame from ornament/frameArt.ts: its own figure on the fixed plate (a laurel wreath, a twisted
  cord, a Greek key, a notarial seal's edge, plain rules), fine rings on the two lathes.
-->
<script lang="ts" module>
  export type RosetteKind = 'seal' | 'cover' | 'stamp' | 'mark' | 'swatch';
  /** Degrees between register positions of both lathes (their lobes repeat every 360/18). */
  export const REGISTER_STEP = 20;
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { circle, loops, petals, rays, ring } from './lathe';
  import { hair } from './ink.svelte';
  import { strokeInk, type Finish } from './print';
  import { DEFAULT_FRAME, frameOf } from './frames';
  import { frameArt, type FrameArt } from './frameArt';

  let {
    size,
    kind = 'seal',
    finish = 'intaglio',
    frame = DEFAULT_FRAME,
    children,
  }: { size: number; kind?: RosetteKind; finish?: Finish; frame?: string | null; children?: Snippet } = $props();

  // proportions of the cartouche (clear centre) per kind, as a fraction of the radius
  const HOLE: Record<RosetteKind, number> = { seal: 0.355, cover: 0.4, stamp: 0.42, mark: 0.3, swatch: 0.34 };
  const hole = $derived(HOLE[kind]);

  function geom(w: number, h: number) {
    return { cx: w / 2, cy: h / 2, R: Math.min(w, h) / 2 - 1 };
  }

  // the volume's emblems wear their frame; brand marks, stamps and ink swatches stay guilloché
  const fid = $derived(kind === 'mark' || kind === 'stamp' ? 'guilloche' : frameOf(frame));
  let artKey = '';
  let art: FrameArt | null = null;
  /** the frame's plates for this size (built once, shared by the three plates' draws) */
  function artFor(w: number, h: number): FrameArt | null {
    const id = fid;
    if (id === 'guilloche') return null;
    const key = `${id}|${kind}|${w}|${h}|${hole}`;
    if (key !== artKey) {
      const { cx, cy, R } = geom(w, h);
      art = frameArt(id, kind, cx, cy, R, hole);
      artKey = key;
    }
    return art;
  }

  function drawStatic(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const { cx, cy, R } = geom(w, h);
    const hw = hair(env.dpr);
    const fa = artFor(w, h);
    if (fa) {
      const k = kind === 'swatch' ? 1.05 : 1.1;
      strokeInk(ctx, fa.fixed.lines, env, hw * k * fa.fixed.weight, { finish });
      if (fa.fixed.fine) strokeInk(ctx, fa.fixed.fine, env, hw * 0.85 * (fa.fixed.fineWeight ?? 0.8), { finish });
      return;
    }
    const p = new Path2D();
    const fine = new Path2D();
    if (kind === 'seal') {
      circle(p, cx, cy, R * 0.996);
      circle(fine, cx, cy, R * 0.981);
      ring(fine, cx, cy, R, { rm: 0.951, lines: 3, k: 84, a: 0.016 });
      circle(fine, cx, cy, R * 0.921);
      rays(fine, cx, cy, R, 0.876, 0.914, Math.round(R * 1.55));
      circle(p, cx, cy, R * 0.872);
      circle(fine, cx, cy, R * 0.598);
      circle(fine, cx, cy, R * 0.586);
      circle(p, cx, cy, R * 0.388);
      ring(fine, cx, cy, R, { rm: 0.374, lines: 2, k: 64, a: 0.0055 });
      circle(fine, cx, cy, R * hole);
    } else if (kind === 'cover') {
      circle(p, cx, cy, R * 0.99);
      ring(fine, cx, cy, R, { rm: 0.94, lines: 2, k: 48, a: 0.03 });
      circle(p, cx, cy, R * 0.885);
      circle(fine, cx, cy, R * 0.62);
      circle(p, cx, cy, R * 0.45);
      circle(fine, cx, cy, R * hole);
    } else if (kind === 'stamp') {
      circle(p, cx, cy, R * 0.99);
      ring(p, cx, cy, R, { rm: 0.93, lines: 2, k: 40, a: 0.035 });
      circle(p, cx, cy, R * 0.86);
      circle(p, cx, cy, R * hole + 1.5);
      circle(fine, cx, cy, R * hole - 1);
    } else if (kind === 'swatch') {
      circle(p, cx, cy, R * 0.99);
      ring(fine, cx, cy, R, { rm: 0.9, lines: 2, k: 28, a: 0.05 });
      circle(p, cx, cy, R * hole);
    } else {
      circle(p, cx, cy, R * 0.99);
      circle(p, cx, cy, R * hole);
    }
    strokeInk(ctx, p, env, hw * (kind === 'stamp' ? 1.6 : kind === 'mark' ? 1.15 : 1.1), { finish });
    strokeInk(ctx, fine, env, hw * (kind === 'stamp' ? 1.2 : 0.85), { finish });
  }

  function drawA(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const { cx, cy, R } = geom(w, h);
    const hw = hair(env.dpr);
    const fa = artFor(w, h);
    if (fa) {
      if (fa.a) strokeInk(ctx, fa.a.lines, env, hw * fa.a.weight, { finish });
      return;
    }
    const p = new Path2D();
    if (kind === 'seal') {
      loops(p, cx, cy, R, { rc: 0.733, a: 0.072, k: 30, lines: 3, squash: 1.12 });
    } else if (kind === 'cover') {
      loops(p, cx, cy, R, { rc: 0.75, a: 0.085, k: 20, lines: 2, squash: 1.1 });
    } else if (kind === 'stamp') {
      petals(p, cx, cy, R, { r0: hole + 0.06, r1: 0.84, lines: 6, k: 12, lean: 1 });
    } else if (kind === 'swatch') {
      petals(p, cx, cy, R, { r0: hole + 0.05, r1: 0.84, lines: 4, k: 10, lean: 0.9 });
    } else {
      petals(p, cx, cy, R, { r0: hole + 0.06, r1: 0.94, lines: 3, k: 8, lean: 0.8 });
    }
    const k = kind === 'stamp' ? 1.3 : kind === 'cover' ? 1.1 : kind === 'mark' ? 1.1 : 0.9;
    strokeInk(ctx, p, env, hw * k, { finish });
  }

  function drawB(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const { cx, cy, R } = geom(w, h);
    const hw = hair(env.dpr);
    const fa = artFor(w, h);
    if (fa) {
      if (fa.b) strokeInk(ctx, fa.b.lines, env, hw * fa.b.weight, { finish });
      return;
    }
    const p = new Path2D();
    if (kind === 'seal') {
      petals(p, cx, cy, R, { r0: 0.405, r1: 0.572, lines: 5, k: 15, lean: 1.2 });
    } else if (kind === 'cover') {
      petals(p, cx, cy, R, { r0: 0.465, r1: 0.6, lines: 3, k: 12, lean: 1 });
    } else {
      return;
    }
    strokeInk(ctx, p, env, hw * (kind === 'cover' ? 1.1 : 0.88), { finish });
  }
</script>

<span class="rosette" data-kind={kind} data-frame={fid} style:--size="{size}px" style:--hole={hole}>
  <InkCanvas class="plate" draw={drawStatic} cache="rosette|{kind}|{fid}|{finish}|s" />
  <InkCanvas class="plate lathe-a" draw={drawA} cache="rosette|{kind}|{fid}|{finish}|a" />
  <InkCanvas class="plate lathe-b" draw={drawB} cache="rosette|{kind}|{fid}|{finish}|b" />
  {#if children}<span class="cartouche">{@render children()}</span>{/if}
</span>

<style>
  .rosette {
    position: relative;
    display: inline-grid;
    flex: none;
    width: var(--size);
    height: var(--size);
  }
  .rosette > :global(.plate) {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .rosette > :global(.lathe-a) {
    transform: rotate(calc(var(--lathe, 0) * 1deg));
  }
  .rosette > :global(.lathe-b) {
    transform: rotate(calc(var(--lathe, 0) * -1deg));
  }
  .cartouche {
    position: absolute;
    left: 50%;
    top: 50%;
    width: calc(var(--size) * var(--hole) - 2px);
    height: calc(var(--size) * var(--hole) - 2px);
    transform: translate(-50%, -50%);
  }
  /* the mark is centred on the rosette even when it is larger than the clear centre. Two
     classes deep: the mark's own `position: relative` (Emblem.svelte, same specificity, later
     in the cascade) otherwise won, and its left: 50% then pushed it off centre by
     (cartouche - mark) / 2 */
  .rosette > .cartouche > :global(*) {
    position: absolute;
    left: 50%;
    top: 50%;
    /* the individual property: a mark's own transform (Sblocca's rise) still composes */
    translate: -50% -50%;
  }
</style>
