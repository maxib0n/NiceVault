<script lang="ts" module>
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
    inset?: number | [number, number, number, number];
    radius?: Radii;
    len?: number;
    gap?: number;
    width?: number;
    slant?: number;
    thread?: string;
    track?: (w: number, h: number) => Track;
    pitch?: (w: number, h: number) => { len: number; gap: number };
  } = $props();

  let el: SVGSVGElement | undefined = $state();
  let w = $state(0);
  let h = $state(0);
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
  .light {
    stroke: rgb(255 255 255 / 0.26);
    stroke-width: calc(var(--sw) * 0.4);
  }
</style>
