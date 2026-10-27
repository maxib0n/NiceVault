<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Maximize2, Minus, Plus, Scan } from '@lucide/svelte';
  import type { FsNode } from '../lib/types';
  import { decodeMedia } from '../lib/reveal';
  import type { InfoReason } from './InfoViewer.svelte';

  let {
    url,
    node,
    key,
    onfail,
    onzoom,
  }: { url: string; node: FsNode; key: string; onfail: (r: InfoReason) => void; onzoom?: (zoomed: boolean) => void } = $props();

  const MIN = 0.25;
  const MAX = 16;
  let stage: HTMLDivElement | undefined = $state();
  let img: HTMLImageElement | undefined = $state();
  let natural = $state<{ w: number; h: number } | null>(null);
  let fitW = $state(0);
  let s = $state(1);
  let tx = $state(0);
  let ty = $state(0);
  let animate = $state(false);
  let dragging = $state(false);

  const pct = $derived(natural && fitW ? Math.round(((s * fitW) / natural.w) * 100) : 100);
  const zoomed = $derived(s > 1.001);
  $effect(() => onzoom?.(zoomed));

  function loaded() {
    if (!img) return;
    natural = { w: img.naturalWidth, h: img.naturalHeight };
    fitW = img.clientWidth;
  }

  function clampPan() {
    if (!stage || !img) return;
    const maxX = Math.max(0, (img.clientWidth * s - stage.clientWidth) / 2);
    const maxY = Math.max(0, (img.clientHeight * s - stage.clientHeight) / 2);
    tx = Math.max(-maxX, Math.min(maxX, tx));
    ty = Math.max(-maxY, Math.min(maxY, ty));
    if (s <= 1) tx = ty = 0;
  }

  function zoomTo(next: number, px = 0, py = 0, smooth = true) {
    const n = Math.max(MIN, Math.min(MAX, next));
    const k = n / s;
    tx = px - (px - tx) * k;
    ty = py - (py - ty) * k;
    s = n;
    animate = smooth;
    clampPan();
  }
  const fit = () => zoomTo(1);
  function actual() {
    if (natural && fitW) zoomTo(natural.w / fitW);
  }

  function centreOf(e: { clientX: number; clientY: number }) {
    const r = stage!.getBoundingClientRect();
    return [e.clientX - r.left - r.width / 2, e.clientY - r.top - r.height / 2] as const;
  }

  function wheel(e: WheelEvent) {
    e.preventDefault();
    const [px, py] = centreOf(e);
    const delta = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
    zoomTo(s * Math.exp(-delta * 0.0022), px, py, false);
  }

  const pointers = new Map<number, { x: number; y: number }>();
  let pinch: { d0: number; s0: number; mx: number; my: number; tx0: number; ty0: number } | null = null;
  let pan: { x: number; y: number; tx0: number; ty0: number } | null = null;
  let vel = { x: 0, y: 0, t: 0 };
  let raf = 0;
  const TOUCH_MIN = 1;
  const stopInertia = () => {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
  };
  function bounds(sc = s) {
    if (!stage || !img) return { x: 0, y: 0 };
    return { x: Math.max(0, (img.clientWidth * sc - stage.clientWidth) / 2), y: Math.max(0, (img.clientHeight * sc - stage.clientHeight) / 2) };
  }
  const band = (v: number, max: number, give = 140) => (v > max ? max + (give * (v - max)) / (give + v - max) : v < -max ? -max - (give * (-max - v)) / (give - max - v) : v);
  const mid = () => {
    const [a, b] = [...pointers.values()];
    return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, d: Math.hypot(a.x - b.x, a.y - b.y) };
  };
  function startPinch() {
    const p = mid();
    const [mx, my] = centreOf({ clientX: p.x, clientY: p.y });
    pinch = { d0: Math.max(1, p.d), s0: s, mx, my, tx0: tx, ty0: ty };
    pan = null;
  }
  function startPan(x: number, y: number) {
    pan = { x, y, tx0: tx, ty0: ty };
    vel = { x: 0, y: 0, t: performance.now() };
  }
  function down(e: PointerEvent) {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    stopInertia();
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size === 2) startPinch();
    else if (pointers.size === 1 && zoomed) startPan(e.clientX, e.clientY);
    if (!zoomed && pointers.size < 2) return;
    e.stopPropagation();
    dragging = true;
    animate = false;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  }
  function move(e: PointerEvent) {
    const prev = pointers.get(e.pointerId);
    if (!prev || !dragging) return;
    e.stopPropagation();
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size >= 2 && pinch) {
      const p = mid();
      const [px, py] = centreOf({ clientX: p.x, clientY: p.y });
      let ns = (pinch.s0 * p.d) / pinch.d0;
      const lo = e.pointerType === 'mouse' ? MIN : TOUCH_MIN;
      if (ns < lo) ns = lo * Math.pow(ns / lo, 0.35);
      else if (ns > MAX) ns = MAX * Math.pow(ns / MAX, 0.35);
      const k = ns / pinch.s0;
      tx = px - (pinch.mx - pinch.tx0) * k;
      ty = py - (pinch.my - pinch.ty0) * k;
      s = ns;
      return;
    }
    if (!pan) return;
    const now = performance.now();
    const dt = Math.max(1, now - vel.t);
    vel = { x: vel.x * 0.3 + ((e.clientX - prev.x) / dt) * 0.7, y: vel.y * 0.3 + ((e.clientY - prev.y) / dt) * 0.7, t: now };
    const b = bounds();
    tx = band(pan.tx0 + e.clientX - pan.x, b.x);
    ty = band(pan.ty0 + e.clientY - pan.y, b.y);
  }
  function settle() {
    const lo = TOUCH_MIN;
    if (s < lo + 0.001) {
      s = lo;
      tx = ty = 0;
    } else if (s > MAX) {
      const k = MAX / s;
      tx *= k;
      ty *= k;
      s = MAX;
    }
    const b = bounds();
    tx = Math.max(-b.x, Math.min(b.x, tx));
    ty = Math.max(-b.y, Math.min(b.y, ty));
    animate = true;
  }
  function glide(vx: number, vy: number) {
    let last = performance.now();
    const step = (now: number) => {
      const dt = Math.min(32, now - last);
      last = now;
      const f = Math.pow(0.998, dt);
      vx *= f;
      vy *= f;
      const b = bounds();
      tx += vx * dt;
      ty += vy * dt;
      const out = Math.abs(tx) > b.x + 0.5 || Math.abs(ty) > b.y + 0.5;
      if (out || Math.hypot(vx, vy) < 0.02) {
        raf = 0;
        if (out) settle();
        return;
      }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
  }
  function up(e: PointerEvent) {
    if (!pointers.has(e.pointerId)) return;
    pointers.delete(e.pointerId);
    if (pinch && pointers.size < 2) {
      pinch = null;
      const rest = [...pointers.values()][0];
      if (rest && s > 1.001) startPan(rest.x, rest.y);
      else pan = null;
      if (!rest) settle();
      return;
    }
    if (pointers.size > 0) return;
    dragging = false;
    if (!pan) {
      if (e.pointerType !== 'mouse') settle();
      return;
    }
    pan = null;
    const b = bounds();
    const inside = Math.abs(tx) <= b.x && Math.abs(ty) <= b.y;
    const fresh = performance.now() - vel.t < 80;
    if (e.pointerType !== 'mouse' && inside && fresh && Math.hypot(vel.x, vel.y) > 0.15) glide(vel.x, vel.y);
    else settle();
  }
  $effect(() => stopInertia);

  let lastTap: { t: number; x: number; y: number } | null = null;
  let tapStart: { id: number; x: number; y: number } | null = null;
  let touchZoomAt = 0;
  function tapDown(e: PointerEvent) {
    tapStart = e.pointerType === 'touch' && e.isPrimary && pointers.size === 0 ? { id: e.pointerId, x: e.clientX, y: e.clientY } : null;
  }
  function tapUp(e: PointerEvent) {
    const s0 = tapStart;
    tapStart = null;
    if (!s0 || s0.id !== e.pointerId || pointers.size > 0 || Math.hypot(e.clientX - s0.x, e.clientY - s0.y) > 12) return;
    const now = performance.now();
    if (lastTap && now - lastTap.t < 320 && Math.hypot(e.clientX - lastTap.x, e.clientY - lastTap.y) < 40) {
      lastTap = null;
      touchZoomAt = now;
      toggleZoom(e, true);
    } else lastTap = { t: now, x: e.clientX, y: e.clientY };
  }
  const born = performance.now();
  function dbl(e: MouseEvent) {
    if (performance.now() - born < 600 || performance.now() - touchZoomAt < 500) return;
    toggleZoom(e);
  }
  function toggleZoom(e: { clientX: number; clientY: number }, finger = false) {
    if (performance.now() - born < 600) return;
    const [px, py] = centreOf(e);
    const real = natural && fitW && natural.w / fitW > 1.2 ? natural.w / fitW : 2.5;
    if (zoomed) fit();
    else zoomTo(finger ? Math.min(MAX, Math.max(2.5, real)) : real, px, py);
  }

  function onkey(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (t?.closest('input, textarea, [contenteditable="true"]')) return;
    if (e.key === '+' || e.key === '=') zoomTo(s * 1.25);
    else if (e.key === '-' || e.key === '_') zoomTo(s / 1.25);
    else if (e.key === '0') fit();
    else if (e.key === '1') actual();
    else return;
    e.preventDefault();
  }

  $effect(() => {
    if (!stage) return;
    const ro = new ResizeObserver(() => {
      if (img) fitW = img.clientWidth;
      clampPan();
    });
    ro.observe(stage);
    return () => ro.disconnect();
  });
</script>

<svelte:window onkeydown={onkey} />

<div class="vw image-vw" data-testid="viewer-image" data-zoom={pct}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stage"
    class:zoomed
    class:dragging
    bind:this={stage}
    onwheel={wheel}
    onpointerdown={(e) => {
      tapDown(e);
      down(e);
    }}
    onpointermove={move}
    onpointerup={(e) => {
      up(e);
      tapUp(e);
    }}
    onpointercancel={(e) => {
      up(e);
      tapStart = null;
    }}
    ondblclick={dbl}
  >
    <span class="print" class:animate style:--nv-t="translate3d({tx}px, {ty}px, 0) scale({s})">
      <img
        bind:this={img}
        src={url}
        alt={node.name}
        decoding="async"
        draggable="false"
        onload={loaded}
        onerror={() => onfail('unsupported')}
        use:decodeMedia={{ speed: 1.6, min: 0, key }}
      />
    </span>
  </div>
  {#if natural}
    <div class="vw-bar nv-enter" role="toolbar" aria-label={m.vw_image_tools()}>
      <button type="button" class="btn ghost icon" aria-label={m.vw_zoom_out()} title={m.vw_zoom_out()} onclick={() => zoomTo(s / 1.25)} disabled={s <= MIN}><Minus size={15} /></button>
      <span class="vw-bar-label" aria-live="polite" data-testid="viewer-zoom">{pct}%</span>
      <button type="button" class="btn ghost icon" aria-label={m.vw_zoom_in()} title={m.vw_zoom_in()} onclick={() => zoomTo(s * 1.25)} disabled={s >= MAX}><Plus size={15} /></button>
      <span class="vw-bar-sep"></span>
      <button type="button" class="btn ghost icon" aria-label={m.vw_fit()} title={m.vw_fit()} aria-pressed={!zoomed} onclick={fit}><Scan size={15} /></button>
      <button type="button" class="btn ghost icon" aria-label={m.vw_actual()} title={m.vw_actual()} onclick={actual}><Maximize2 size={15} /></button>
      <span class="vw-bar-sep"></span>
      <span class="vw-bar-label">{natural.w} × {natural.h}</span>
    </div>
  {/if}
</div>

<style>
  .image-vw {
    overflow: hidden;
  }
  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    overflow: hidden;
    padding: var(--space-6) var(--space-6) calc(var(--space-6) + 40px);
    touch-action: none;
    cursor: zoom-in;
    container-type: size;
  }
  .stage.zoomed {
    cursor: grab;
  }
  .stage.dragging {
    cursor: grabbing;
  }
  .print {
    --ascii-surface: var(--print-develop);
    display: block;
    padding: 8px;
    background:
      linear-gradient(var(--print-develop), var(--print-develop)) content-box,
      var(--print-border);
    box-shadow:
      0 0 0 1px rgb(var(--cast) / 0.08),
      0 2px 4px rgb(var(--cast) / 0.1),
      0 22px 44px -16px rgb(var(--cast) / 0.45),
      var(--print-glow);
    transform: var(--nv-t);
    transform-origin: 50% 50%;
    will-change: transform;
  }
  .print.animate {
    transition: --nv-t var(--dur-ui) var(--ease-out);
  }
  @media (prefers-reduced-motion: reduce) {
    .print.animate {
      transition: none;
    }
  }
  img {
    display: block;
    max-width: calc(100cqw - 16px);
    max-height: calc(100cqh - 16px);
    object-fit: contain;
    image-orientation: from-image;
    user-select: none;
    -webkit-user-drag: none;
  }
</style>
