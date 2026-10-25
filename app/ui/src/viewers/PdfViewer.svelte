<!--
  PDF through the bundled pdf.js (Apache-2.0, offline; WebKitGTK has no PDF viewer of its
  own). The bytes come in on demand through a range transport over the preview URL, so a
  500-page file opens as fast as a one-page one; pages are drawn when they scroll near and
  dropped when far, canvases capped in pixels. Page field + ‹ ›, zoom − / + / fit width.
-->
<script lang="ts">
  import { tick } from 'svelte';
  import { m } from '../paraglide/messages.js';
  import { ChevronLeft, ChevronRight, Minus, Plus, Scan } from '@lucide/svelte';
  import type { PDFDocumentProxy, PDFPageProxy } from 'pdfjs-dist';
  import { skeleton } from '../lib/reveal';
  import type { ByteSource } from './source';
  import type { InfoReason } from './InfoViewer.svelte';

  let { src, onfail, onzoom }: { src: ByteSource; onfail: (r: InfoReason) => void; onzoom?: (zoomed: boolean) => void } = $props();

  const MAX_PAGES = 5000;
  /** canvas budget per page (pixels): beyond, the page is drawn softer and scaled by CSS */
  const MAX_CANVAS_PX = 12_000_000;
  const ZOOMS = [0.5, 0.67, 0.8, 1, 1.25, 1.5, 2, 3];

  let scroller: HTMLDivElement | undefined = $state();
  let status = $state<'loading' | 'ready'>('loading');
  let doc = $state.raw<PDFDocumentProxy | null>(null);
  let count = $state(0);
  /** first page's size in points (all placeholders start with it) */
  let base = $state({ w: 595, h: 842 });
  let sizes = $state<{ w: number; h: number }[]>([]);
  let zoom = $state(1);
  let current = $state(1);
  let pageField = $state('1');
  let width = $state(800);

  // fit width: the sheet as wide as the stage allows (max 900 px), times the zoom
  const fitScale = $derived(Math.min(width - 48, 900) / base.w);
  const scale = $derived(fitScale * zoom);

  $effect(() => {
    const s = src;
    let task: { destroy(): Promise<void> } | null = null;
    let cancelled = false;
    status = 'loading';
    doc = null;
    (async () => {
      const pdfjs = await import('pdfjs-dist');
      const worker = (await import('pdfjs-dist/build/pdf.worker.min.mjs?url')).default;
      pdfjs.GlobalWorkerOptions.workerSrc = worker;
      const length = await s.size();
      const first = await s.read(0, Math.min(length, 512 * 1024));
      const transport = new pdfjs.PDFDataRangeTransport(length, first);
      transport.requestDataRange = (begin: number, end: number) => {
        s.read(begin, end).then(
          (b) => !cancelled && transport.onDataRange(begin, b),
          () => undefined,
        );
      };
      const t = pdfjs.getDocument({
        range: transport,
        rangeChunkSize: 256 * 1024,
        disableAutoFetch: true,
        disableStream: true,
        enableXfa: false,
        // no fetches of fonts/cmaps from anywhere: what the PDF embeds, else the system's
        useSystemFonts: true,
      });
      task = t;
      const d = await t.promise;
      if (cancelled) return;
      const p1 = await d.getPage(1);
      const vp = p1.getViewport({ scale: 1 });
      base = { w: vp.width, h: vp.height };
      count = Math.min(d.numPages, MAX_PAGES);
      sizes = Array.from({ length: count }, () => ({ w: vp.width, h: vp.height }));
      doc = d;
      status = 'ready';
    })().catch(() => {
      if (!cancelled) onfail('error');
    });
    return () => {
      cancelled = true;
      void task?.destroy();
      rendered.clear();
    };
  });

  $effect(() => {
    if (!scroller) return;
    const ro = new ResizeObserver(() => scroller && (width = scroller.clientWidth));
    ro.observe(scroller);
    return () => ro.disconnect();
  });

  // --- drawing: pages near the viewport, at the current scale --------------------------------
  const rendered = new Map<number, { scale: number; task: { cancel(): void } | null }>();

  async function draw(i: number, canvas: HTMLCanvasElement) {
    const d = doc;
    if (!d) return;
    const want = scale;
    const prev = rendered.get(i);
    if (prev && prev.scale === want) return;
    prev?.task?.cancel();
    rendered.set(i, { scale: want, task: null });
    let page: PDFPageProxy;
    try {
      page = await d.getPage(i);
    } catch {
      return;
    }
    const vp1 = page.getViewport({ scale: 1 });
    if (vp1.width !== sizes[i - 1]?.w || vp1.height !== sizes[i - 1]?.h) sizes[i - 1] = { w: vp1.width, h: vp1.height };
    const dpr = window.devicePixelRatio || 1;
    let px = want * dpr;
    const area = vp1.width * vp1.height * px * px;
    if (area > MAX_CANVAS_PX) px *= Math.sqrt(MAX_CANVAS_PX / area);
    const vp = page.getViewport({ scale: px });
    const off = document.createElement('canvas');
    off.width = Math.floor(vp.width);
    off.height = Math.floor(vp.height);
    const job = page.render({ canvas: off, viewport: vp });
    const entry = rendered.get(i);
    if (entry) entry.task = job;
    try {
      await job.promise;
    } catch {
      return; // cancelled
    }
    if (rendered.get(i)?.scale !== want) return;
    // swap in the finished picture at once (never a half-drawn page)
    canvas.width = off.width;
    canvas.height = off.height;
    canvas.getContext('2d')?.drawImage(off, 0, 0);
    off.width = off.height = 0;
    canvas.dataset.drawn = '1';
  }

  function forget(i: number, canvas: HTMLCanvasElement) {
    rendered.get(i)?.task?.cancel();
    rendered.delete(i);
    canvas.width = canvas.height = 0;
    delete canvas.dataset.drawn;
  }

  /** each page placeholder: drawn while within two screens of the viewport */
  function page(canvas: HTMLCanvasElement, p: { i: number; scale: number }) {
    let i = p.i;
    let near = false;
    const io = new IntersectionObserver(
      ([e]) => {
        near = e.isIntersecting;
        if (near) void draw(i, canvas);
        else forget(i, canvas);
      },
      { root: scroller, rootMargin: '200% 0px' },
    );
    io.observe(canvas);
    return {
      update(q: { i: number; scale: number }) {
        i = q.i;
        if (near) void draw(i, canvas);
      },
      destroy() {
        io.disconnect();
        forget(i, canvas);
      },
    };
  }

  // the page in the middle of the view is "current"
  function onscroll() {
    const el = scroller;
    if (!el) return;
    const mid = el.scrollTop + el.clientHeight / 2;
    const pages = el.querySelectorAll<HTMLElement>('[data-page]');
    let best = 1;
    for (const p of pages) {
      if (p.offsetTop <= mid) best = Number(p.dataset.page);
      else break;
    }
    if (best !== current) {
      current = best;
      pageField = String(best);
    }
  }

  function goTo(n: number) {
    const k = Math.max(1, Math.min(count, Math.round(n) || 1));
    const el = scroller?.querySelector<HTMLElement>(`[data-page="${k}"]`);
    if (el && scroller) scroller.scrollTop = el.offsetTop - 16;
    current = k;
    pageField = String(k);
  }

  function setZoom(z: number) {
    const el = scroller;
    // keep the same spot of the document in view
    const ratio = el ? (el.scrollTop + el.clientHeight / 2) / Math.max(1, el.scrollHeight) : 0;
    zoom = Math.max(ZOOMS[0], Math.min(ZOOMS[ZOOMS.length - 1], z));
    requestAnimationFrame(() => {
      if (el) el.scrollTop = ratio * el.scrollHeight - el.clientHeight / 2;
    });
  }
  $effect(() => onzoom?.(zoom > 1.001));

  // ---- fingers: pinch about the midpoint, double tap to zoom in where it lands (and back to fit
  // width). One finger is the scroller's own (native scroll, its momentum); two are ours: the
  // pages are scaled by a transform while the fingers move (nothing laid out per frame), then
  // drawn at the new size with the same point of the document under the fingers.
  let pagesEl: HTMLDivElement | undefined = $state();
  const TOUCH_ZOOM = { min: 1, max: ZOOMS[ZOOMS.length - 1] };
  let pinch: { d0: number; z0: number; ox: number; oy: number; mx0: number; my0: number; k: number; mx: number; my: number } | null = null;
  let lastTap: { t: number; x: number; y: number } | null = null;
  let tapStart: { x: number; y: number; t: number } | null = null;
  const two = (e: TouchEvent) => {
    const [a, b] = [e.touches[0], e.touches[1]];
    return { x: (a.clientX + b.clientX) / 2, y: (a.clientY + b.clientY) / 2, d: Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY) };
  };
  /** zoom to `z` keeping the document point under (cx, cy) (client px) under it */
  async function zoomAt(z: number, cx: number, cy: number) {
    const el = scroller;
    if (!el) return setZoom(z);
    const r = el.getBoundingClientRect();
    const z0 = zoom;
    const ox = el.scrollLeft + cx - r.left;
    const oy = el.scrollTop + cy - r.top;
    zoom = Math.max(TOUCH_ZOOM.min, Math.min(TOUCH_ZOOM.max, z));
    await tick();
    const k = zoom / z0;
    el.scrollLeft = ox * k - (cx - r.left);
    el.scrollTop = oy * k - (cy - r.top);
  }
  function touchstart(e: TouchEvent) {
    if (e.touches.length === 1) {
      const t = e.touches[0];
      tapStart = { x: t.clientX, y: t.clientY, t: performance.now() };
    } else tapStart = null;
    if (e.touches.length !== 2 || !scroller || !pagesEl) return;
    e.preventDefault();
    const p = two(e);
    const pr = pagesEl.getBoundingClientRect();
    // the transform's origin: the midpoint, in the pages' own box
    pinch = { d0: Math.max(1, p.d), z0: zoom, ox: p.x - pr.left, oy: p.y - pr.top, mx0: p.x, my0: p.y, k: 1, mx: p.x, my: p.y };
    pagesEl.style.transformOrigin = `${pinch.ox}px ${pinch.oy}px`;
    pagesEl.style.willChange = 'transform';
  }
  function touchmove(e: TouchEvent) {
    if (!pinch || e.touches.length < 2 || !pagesEl) return;
    e.preventDefault();
    const p = two(e);
    let z = (pinch.z0 * p.d) / pinch.d0;
    if (z < TOUCH_ZOOM.min) z = TOUCH_ZOOM.min * Math.pow(z / TOUCH_ZOOM.min, 0.35);
    else if (z > TOUCH_ZOOM.max) z = TOUCH_ZOOM.max * Math.pow(z / TOUCH_ZOOM.max, 0.35);
    pinch.k = z / pinch.z0;
    pinch.mx = p.x;
    pinch.my = p.y;
    pagesEl.style.transform = `translate3d(${p.x - pinch.mx0}px, ${p.y - pinch.my0}px, 0) scale(${pinch.k})`;
  }
  async function touchend(e: TouchEvent) {
    if (pinch && e.touches.length < 2) {
      const pz = pinch;
      pinch = null;
      if (pagesEl) {
        pagesEl.style.transform = '';
        pagesEl.style.willChange = '';
      }
      // the document point that was under the midpoint at the start, under where it is now
      const el = scroller;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const ox = el.scrollLeft + pz.mx0 - r.left;
      const oy = el.scrollTop + pz.my0 - r.top;
      const z0 = zoom;
      zoom = Math.max(TOUCH_ZOOM.min, Math.min(TOUCH_ZOOM.max, pz.z0 * pz.k));
      await tick();
      const k = zoom / z0;
      el.scrollLeft = ox * k - (pz.mx - r.left);
      el.scrollTop = oy * k - (pz.my - r.top);
      return;
    }
    // a double tap: in where it lands, again: back to fit width
    const t0 = tapStart;
    tapStart = null;
    const t = e.changedTouches[0];
    if (!t0 || !t || e.touches.length || Math.hypot(t.clientX - t0.x, t.clientY - t0.y) > 12 || performance.now() - t0.t > 300) return;
    const now = performance.now();
    if (lastTap && now - lastTap.t < 320 && Math.hypot(t.clientX - lastTap.x, t.clientY - lastTap.y) < 40) {
      lastTap = null;
      e.preventDefault();
      void zoomAt(zoom > 1.001 ? 1 : 2, t.clientX, t.clientY);
    } else lastTap = { t: now, x: t.clientX, y: t.clientY };
  }
  $effect(() => {
    const el = scroller;
    if (!el) return;
    // not passive: two fingers are ours, not the page's
    el.addEventListener('touchstart', touchstart, { passive: false });
    el.addEventListener('touchmove', touchmove, { passive: false });
    el.addEventListener('touchend', touchend, { passive: false });
    el.addEventListener('touchcancel', touchend, { passive: false });
    return () => {
      el.removeEventListener('touchstart', touchstart);
      el.removeEventListener('touchmove', touchmove);
      el.removeEventListener('touchend', touchend);
      el.removeEventListener('touchcancel', touchend);
    };
  });

  const zoomIn = () => setZoom(ZOOMS.find((z) => z > zoom + 0.001) ?? zoom);
  const zoomOut = () => setZoom([...ZOOMS].reverse().find((z) => z < zoom - 0.001) ?? zoom);

  function key(e: KeyboardEvent) {
    if (status !== 'ready' || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (t?.closest('input, textarea')) return;
    if (e.key === '+' || e.key === '=') zoomIn();
    else if (e.key === '-') zoomOut();
    else if (e.key === '0' && !e.ctrlKey && !e.metaKey) setZoom(1);
    else if (e.key === 'PageDown' && !e.shiftKey) goTo(current + 1);
    else if (e.key === 'PageUp' && !e.shiftKey) goTo(current - 1);
    else if (e.key === 'Home' && (e.ctrlKey || e.metaKey)) goTo(1);
    else if (e.key === 'End' && (e.ctrlKey || e.metaKey)) goTo(count);
    else return;
    e.preventDefault();
  }
</script>

<svelte:window onkeydown={key} />

<div class="vw pdf-vw" data-testid="viewer-pdf" data-status={status} data-pages={count} data-page={current} data-zoom={Math.round(zoom * 100)} aria-busy={status === 'loading'}>
  <div class="vw-scroll" bind:this={scroller} {onscroll} data-testid="viewer-scroll">
    {#if status === 'loading'}
      <!-- the page it will be: paper proportions, the glyphs where the text will land -->
      <div class="vw-sheet vw-loading nv-enter page-skeleton" data-state="loading">
        <span class="visually-hidden">{m.pdf_opening()}</span>
        <span class="vw-skeleton" aria-hidden="true" use:skeleton={{ lines: 12, seed: 7, widths: ['58%', '100%', '94%', '100%', '72%', '0%', '100%', '88%', '100%', '96%', '64%', '30%'] }}></span>
      </div>
    {:else}
      <div class="pages" bind:this={pagesEl} aria-label={m.pdf_pages({ count, n: String(count) })}>
        {#each sizes as sz, k (k)}
          <div class="pdf-page" data-page={k + 1} role="img" aria-label={m.pdf_page({ n: String(k + 1) })} style:width="{Math.round(sz.w * scale)}px" style:height="{Math.round(sz.h * scale)}px">
            <canvas aria-hidden="true" use:page={{ i: k + 1, scale }}></canvas>
          </div>
        {/each}
      </div>
    {/if}
  </div>
  {#if status === 'ready'}
    <div class="vw-bar nv-enter" role="toolbar" aria-label={m.vw_pdf_tools()}>
      <button type="button" class="btn ghost icon" aria-label={m.vw_prev_page()} title={m.vw_prev_page()} disabled={current <= 1} onclick={() => goTo(current - 1)}><ChevronLeft size={15} /></button>
      <input
        class="vw-page"
        inputmode="numeric"
        aria-label={m.vw_page_field()}
        bind:value={pageField}
        onkeydown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            goTo(Number(pageField));
          }
          e.stopPropagation();
        }}
        onblur={() => (pageField = String(current))}
      />
      <span class="vw-bar-label">{m.vw_of_pages({ n: String(count) })}</span>
      <button type="button" class="btn ghost icon" aria-label={m.vw_next_page()} title={m.vw_next_page()} disabled={current >= count} onclick={() => goTo(current + 1)}><ChevronRight size={15} /></button>
      <span class="vw-bar-sep"></span>
      <button type="button" class="btn ghost icon" aria-label={m.vw_zoom_out()} title={m.vw_zoom_out()} disabled={zoom <= ZOOMS[0]} onclick={zoomOut}><Minus size={15} /></button>
      <span class="vw-bar-label" data-testid="viewer-zoom">{Math.round(zoom * 100)}%</span>
      <button type="button" class="btn ghost icon" aria-label={m.vw_zoom_in()} title={m.vw_zoom_in()} disabled={zoom >= ZOOMS[ZOOMS.length - 1]} onclick={zoomIn}><Plus size={15} /></button>
      <button type="button" class="btn ghost icon" aria-label={m.vw_fit_width()} title={m.vw_fit_width()} aria-pressed={zoom === 1} onclick={() => setZoom(1)}><Scan size={15} /></button>
    </div>
  {/if}
</div>

<style>
  .page-skeleton {
    width: min(900px, 100% - 32px);
    max-width: none;
  }
  .pages {
    display: flex;
    flex-direction: column;
    align-items: center;
    /* zoomed wider than the stage: the pages grow the scroll area both ways (a centred box
       wider than its parent would hang off the left edge, out of reach) */
    width: max-content;
    min-width: 100%;
    gap: var(--space-5);
  }
  /* pages are paper on the stage, in both themes */
  .pdf-page {
    flex: none;
    overflow: hidden;
    background: white;
    border-radius: 1px;
    box-shadow:
      0 0 0 1px rgb(var(--cast) / 0.08),
      0 2px 4px rgb(var(--cast) / 0.1),
      0 18px 36px -14px rgb(var(--cast) / 0.4);
  }
  .pdf-page canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
