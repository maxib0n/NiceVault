<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { skeleton } from '../lib/reveal';

  let { url }: { url: string } = $props();
  let host: HTMLDivElement | undefined = $state();
  let status = $state<'loading' | 'ready' | 'error'>('loading');
  let pages = $state(0);

  $effect(() => {
    const u = url;
    const el = host;
    if (!el) return;
    let cancelled = false;
    let destroy: (() => void) | null = null;
    status = 'loading';
    el.replaceChildren();
    (async () => {
      const pdfjs = await import('pdfjs-dist');
      const worker = (await import('pdfjs-dist/build/pdf.worker.min.mjs?url')).default;
      pdfjs.GlobalWorkerOptions.workerSrc = worker;
      const data = new Uint8Array(await (await fetch(u)).arrayBuffer());
      const task = pdfjs.getDocument({ data });
      destroy = () => void task.destroy();
      const doc = await task.promise;
      if (cancelled) return;
      pages = doc.numPages;
      const width = Math.min(el.clientWidth - 32, 900);
      const dpr = window.devicePixelRatio || 1;
      for (let i = 1; i <= Math.min(doc.numPages, 50); i++) {
        const page = await doc.getPage(i);
        if (cancelled) return;
        const base = page.getViewport({ scale: 1 });
        const vp = page.getViewport({ scale: (width / base.width) * dpr });
        const canvas = document.createElement('canvas');
        canvas.className = 'pdf-page';
        canvas.width = vp.width;
        canvas.height = vp.height;
        canvas.style.width = `${vp.width / dpr}px`;
        canvas.setAttribute('aria-label', m.pdf_page({ n: String(i) }));
        canvas.setAttribute('role', 'img');
        el.appendChild(canvas);
        await page.render({ canvas, viewport: vp }).promise;
        if (i === 1) status = 'ready';
      }
      status = 'ready';
    })().catch(() => {
      if (!cancelled) status = 'error';
    });
    return () => {
      cancelled = true;
      destroy?.();
    };
  });
</script>

<div class="pdf-view" data-status={status} aria-busy={status === 'loading'}>
  {#if status === 'loading'}
    <div class="state page-skeleton nv-enter" data-state="loading">
      <span class="visually-hidden">{m.pdf_opening()}</span>
      <span class="skeleton" aria-hidden="true" use:skeleton={{ lines: 12, seed: 7, widths: ['58%', '100%', '94%', '100%', '72%', '0%', '100%', '88%', '100%', '96%', '64%', '30%'] }}></span>
    </div>
  {/if}
  {#if status === 'error'}<p class="hint state" data-state="error">{m.pdf_error()}</p>{/if}
  <div class="pages" bind:this={host} aria-label={m.pdf_pages({ count: pages, n: String(pages) })}></div>
</div>

<style>
  .pdf-view {
    width: 100%;
    height: 100%;
    overflow: auto;
    background: transparent;
    padding: var(--space-5) var(--space-4);
  }
  .state {
    padding-top: var(--space-6);
    text-align: center;
  }
  .state[data-state='loading'] {
    animation: nv-enter 1ms linear 300ms backwards;
  }
  .page-skeleton {
    width: min(900px, 100% - 32px);
    margin: 0 auto;
    padding: var(--space-6) var(--space-6) var(--space-7);
    border-radius: 1px;
    background: var(--paper-hi);
    box-shadow:
      0 0 0 1px rgb(var(--cast) / 0.08),
      0 18px 36px -14px rgb(var(--cast) / 0.4);
    text-align: left;
  }
  .skeleton {
    display: block;
    font-size: var(--text-md);
    --ascii-skeleton: var(--ink-4);
  }
  .pages {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-5);
  }
  .pages :global(.pdf-page) {
    background: white;
    border-radius: 1px;
    box-shadow:
      0 0 0 1px rgb(var(--cast) / 0.08),
      0 2px 4px rgb(var(--cast) / 0.1),
      0 18px 36px -14px rgb(var(--cast) / 0.4);
  }
</style>
