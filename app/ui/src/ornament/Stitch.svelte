<!--
  Saddle stitching sewn round a card, a cover or along a seam (ornament/stitch.ts): an even
  pitch with a whole number of stitches, following the corners' radius, so no stitch is ever
  cut; each one a length of thread with its drop shadow and the light along its top edge, set
  in the faint groove the pricking iron pressed. Fills its positioned parent (absolute, inset 0);
  shown only where the theme stitches (--stitch-alpha). Drawn once per size: one SVG, no canvas.
    shape="box"   a closed track `inset` from the parent's edges, corners of `radius`
    shape="seam"  a straight seam down the middle of the parent (a strip as tall as the seam)
    track={(w, h) => Track}  any other needle line (the document's, round its medallions)
  Without `radius`, each corner of the box is concentric with the parent's own rounded corner
  (its border-radius less the inset): the stitching and the cut edge always curve together.
-->
<script lang="ts" module>
  // one observer for every stitched element on screen
  const watchers = new WeakMap<Element, (w: number, h: number) => void>();
  let ro: ResizeObserver | null = null;
  function observe(el: Element, cb: (w: number, h: number) => void): () => void {
    ro ??= new ResizeObserver((entries) => {
      for (const e of entries) {
        const box = e.contentBoxSize?.[0];
        const w = box ? box.inlineSize : e.contentRect.width;
        const h = box ? box.blockSize : e.contentRect.height;
        watchers.get(e.target)?.(w, h);
      }
    });
    watchers.set(el, cb);
    ro.observe(el);
    return () => {
      watchers.delete(el);
      ro?.unobserve(el);
    };
  }

  // the geometry per (spec, size): a page opened again (Impostazioni, Sblocca, the board's
  // passports) draws its stitching from here instead of walking the track once more
  export interface Geo {
    groove: string;
    shadow: string;
    thread: string;
    light: string;
  }
  const geoCache = new Map<string, Geo>();
  const fnIds = new WeakMap<object, number>();
  let fnNext = 1;
  export function fnId(f: object | undefined): number {
    if (!f) return 0;
    let id = fnIds.get(f);
    if (!id) fnIds.set(f, (id = fnNext++));
    return id;
  }
  export function cachedGeo(key: string, make: () => Geo): Geo {
    let g = geoCache.get(key);
    if (g) {
      // most recently used last
      geoCache.delete(key);
    } else {
      g = make();
      if (geoCache.size >= 96) geoCache.delete(geoCache.keys().next().value!);
    }
    geoCache.set(key, g);
    return g;
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { roundRect, seam, stitchPath, stitches, type Radii, type Track } from './stitch';

  let {
    shape = 'box',
    inset = 6,
    radius,
    len = 4,
    gap = 2.6,
    width = 1.35,
    slant = 0.2,
    thread = 'var(--stitch)',
    track,
    pitch,
  }: {
    shape?: 'box' | 'seam';
    /** px from each edge: one value or [top, right, bottom, left] */
    inset?: number | [number, number, number, number];
    /** the corners' radius; default: concentric with the parent's border-radius */
    radius?: Radii;
    len?: number;
    gap?: number;
    width?: number;
    slant?: number;
    /** the thread's colour (a CSS colour expression) */
    thread?: string;
    track?: (w: number, h: number) => Track;
    /** the stitch length and gap for this size (instead of len / gap) */
    pitch?: (w: number, h: number) => { len: number; gap: number };
  } = $props();

  let el: SVGSVGElement | undefined = $state();
  let w = $state(0);
  let h = $state(0);
  /** the parent's corner radii (tl, tr, br, bl), read with its size */
  let outer = $state<[number, number, number, number]>([0, 0, 0, 0]);

  onMount(() =>
    observe(el!, (nw, nh) => {
      w = Math.round(nw * 2) / 2;
      h = Math.round(nh * 2) / 2;
      const p = el?.parentElement;
      if (radius !== undefined || !p) return;
      const cs = getComputedStyle(p);
      const r = [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius].map((v) => Math.round((parseFloat(v) || 0) * 2) / 2);
      if (r.some((v, i) => v !== outer[i])) outer = r as [number, number, number, number];
    }),
  );

  /** each corner: the given radius, or the parent's less the insets that meet there */
  function radii(it: number, ir: number, ib: number, il: number): Radii {
    if (radius !== undefined) return radius;
    const c = (o: number, a: number, b: number) => Math.max(0, o - (a + b) / 2);
    return [c(outer[0], it, il), c(outer[1], it, ir), c(outer[2], ib, ir), c(outer[3], ib, il)];
  }

  const geo = $derived.by(() => {
    if (w < 8 || h < 8) return null;
    const key = `${shape}|${JSON.stringify(inset)}|${JSON.stringify(radius ?? outer)}|${len}|${gap}|${slant}|${fnId(track)}|${fnId(pitch)}|${w}|${h}`;
    return cachedGeo(key, () => {
      let t: Track;
      if (track) t = track(w, h);
      else if (shape === 'seam') {
        const m = typeof inset === 'number' ? inset : inset[0];
        t = seam(w / 2, m, w / 2, h - m);
      } else {
        const [it, ir, ib, il] = typeof inset === 'number' ? [inset, inset, inset, inset] : inset;
        t = roundRect(il, it, w - il - ir, h - it - ib, radii(it, ir, ib, il));
      }
      const p = pitch ? pitch(w, h) : { len, gap };
      const segs = stitches(t, { ...p, slant, margin: shape === 'seam' ? 0 : undefined });
      return {
        groove: t.svg(),
        shadow: stitchPath(segs, 0.5, 0.7),
        thread: stitchPath(segs),
        light: stitchPath(segs, -0.3, -0.34, 0.16),
      };
    });
  });
</script>

<svg bind:this={el} class="stitch" data-shape={shape} aria-hidden="true" style:--thread={thread} style:--sw="{width}px">
  {#if geo}
    <path class="groove" d={geo.groove} />
    <path class="shadow" d={geo.shadow} />
    <path class="thread" d={geo.thread} />
    <path class="light" d={geo.light} />
  {/if}
</svg>

<style>
  .stitch {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
    pointer-events: none;
    opacity: var(--stitch-alpha);
    fill: none;
    stroke-linecap: round;
  }
  /* the needle line pressed into the material */
  .groove {
    stroke: var(--relief-lo);
    stroke-width: calc(var(--sw) * 1.6);
    opacity: 0.35;
  }
  .shadow {
    stroke: rgb(0 0 0 / 0.6);
    stroke-width: var(--sw);
  }
  .thread {
    stroke: var(--thread);
    stroke-width: var(--sw);
  }
  /* the light along each stitch's top edge */
  .light {
    stroke: rgb(255 255 255 / 0.26);
    stroke-width: calc(var(--sw) * 0.4);
  }
</style>
