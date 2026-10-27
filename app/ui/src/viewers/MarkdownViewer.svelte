<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { BookOpen, CodeXml, TableOfContents, X } from '@lucide/svelte';
  import type { FsNode, VaultInfo } from '../lib/types';
  import { ROOT } from '../lib/types';
  import * as api from '../lib/api';
  import type { ByteSource } from './source';
  import { CAPS, viewerKind } from './kinds';
  import { detectEncoding } from './encoding';
  import { resolveVolumePath, safeHref } from './links';
  import type { Rendered } from './markdown';
  import { openLink } from './actions';
  import { skeleton } from '../lib/reveal';
  import TextViewer from './TextViewer.svelte';
  import type { InfoReason } from './InfoViewer.svelte';

  let { src, node, vault = null, onfail }: { src: ByteSource; node: FsNode; vault?: VaultInfo | null; onfail: (r: InfoReason) => void } = $props();

  let doc = $state.raw<Rendered | null>(null);
  let source = $state(false);
  let big = $state(false);
  let error = $state(false);
  let pending = $state<string | null>(null);
  let cancelBtn: HTMLButtonElement | undefined = $state();
  let body: HTMLDivElement | undefined = $state();
  let scroller: HTMLDivElement | undefined = $state();
  let width = $state(0);
  let tocOpen = $state(true);
  let tocSlip = $state(false);
  let active = $state<string | null>(null);
  let mathState = $state<'none' | 'loading' | 'ready'>('none');
  let mermaidState = $state<'none' | 'loading' | 'ready' | 'error'>('none');
  let pictures = $state(0);

  const wide = $derived(width >= 1060);
  const hasToc = $derived(!!doc && doc.toc.length >= 2);
  const column = $derived(hasToc && wide && tocOpen);
  const slip = $derived(hasToc && !wide && tocSlip);
  const minLevel = $derived(doc && doc.toc.length ? Math.min(...doc.toc.map((t) => t.level)) : 1);

  $effect(() => {
    const s = src;
    doc = null;
    error = false;
    let gone = false;
    (async () => {
      const size = await s.size();
      if (size > CAPS.markdown) {
        big = true;
        return;
      }
      const [bytes, md] = await Promise.all([s.read(0, size), import('./markdown')]);
      if (gone) return;
      const d = detectEncoding(bytes, false);
      if (d.binary) return onfail('binary');
      const text = new TextDecoder(d.encoding).decode(bytes.subarray(d.bom));
      doc = md.renderMarkdown(text, {
        note: m.vw_md_alert_note(),
        tip: m.vw_md_alert_tip(),
        important: m.vw_md_alert_important(),
        warning: m.vw_md_alert_warning(),
        caution: m.vw_md_alert_caution(),
        done: m.vw_md_done(),
        todo: m.vw_md_todo(),
        image: m.vw_md_image(),
        imageBlocked: m.vw_md_image_blocked(),
      });
    })().catch(() => {
      if (!gone) error = true;
    });
    return () => {
      gone = true;
    };
  });

  $effect(() => {
    const d = doc;
    const el = body;
    if (!d || !el || source) return;
    let gone = false;
    mathState = d.math ? 'loading' : 'none';
    mermaidState = d.mermaid ? 'loading' : 'none';
    pictures = 0;
    let observer: MutationObserver | null = null;
    let media: MediaQueryList | null = null;
    let redraw: (() => void) | null = null;
    if (d.math || d.mermaid) {
      void import('./markdownExtras').then(async (x) => {
        if (gone) return;
        if (d.math) {
          await x.renderMath(el).catch(() => 0);
          if (!gone) mathState = 'ready';
        }
        if (d.mermaid && !gone) {
          const draw = () =>
            x.renderMermaid(el).catch((e) => {
              el.dataset.error = String(e?.message ?? e).slice(0, 300);
              return 'error' as const;
            });
          const first = await draw();
          if (gone) return;
          mermaidState = first;
          let timer = 0;
          redraw = () => {
            clearTimeout(timer);
            timer = window.setTimeout(async () => {
              if (!gone) mermaidState = await draw();
            }, 60);
          };
          observer = new MutationObserver(() => redraw?.());
          observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-accent', 'data-theme', 'class'] });
          media = matchMedia('(prefers-color-scheme: dark)');
          media.addEventListener('change', redraw);
        }
      });
    }
    void showPictures(el, () => gone);
    return () => {
      gone = true;
      observer?.disconnect();
      if (media && redraw) media.removeEventListener('change', redraw);
    };
  });

  async function showPictures(el: HTMLElement, gone: () => boolean) {
    const spots = [...el.querySelectorAll<HTMLElement>('.md-img-local')].slice(0, 64);
    if (!spots.length || !vault) return;
    const v = vault;
    let dir: string;
    try {
      const p = await api.fsProps(v.id, node.id);
      dir = p.path.slice(0, p.path.lastIndexOf('/')) || '/';
    } catch {
      return;
    }
    const lists = new Map<number, Promise<FsNode[]>>();
    const list = (id: number) => {
      let l = lists.get(id);
      if (!l) lists.set(id, (l = api.fsList(v.id, id)));
      return l;
    };
    for (const spot of spots) {
      if (gone()) return;
      const segs = resolveVolumePath(dir, spot.dataset.path ?? '');
      if (!segs) continue;
      let at = ROOT;
      let hit: FsNode | undefined;
      try {
        for (let i = 0; i < segs.length; i++) {
          const last = i === segs.length - 1;
          hit = (await list(at)).find((n) => n.name === segs[i] && n.kind === (last ? 'file' : 'dir'));
          if (!hit) break;
          at = hit.id;
        }
      } catch {
        hit = undefined;
      }
      if (!hit || gone() || viewerKind(hit.name) !== 'image' || hit.size > CAPS.image) continue;
      const url = api.previewUrl(v.id, hit.id, undefined, Math.ceil(1400 * (devicePixelRatio || 1)));
      if (!url) continue;
      const img = document.createElement('img');
      img.className = 'md-pic';
      img.alt = spot.textContent ?? '';
      img.decoding = 'async';
      img.draggable = false;
      const fig = document.createElement('span');
      fig.className = 'md-figure';
      fig.appendChild(img);
      if (img.alt) {
        const cap = document.createElement('span');
        cap.className = 'md-caption';
        cap.textContent = img.alt;
        fig.appendChild(cap);
      }
      img.addEventListener('error', () => fig.replaceWith(spot), { once: true });
      img.src = url;
      spot.replaceWith(fig);
      pictures++;
    }
  }

  function ask(href: string | null) {
    if (!href) return;
    pending = href;
    queueMicrotask(() => cancelBtn?.focus());
  }
  async function confirmOpen() {
    const h = pending;
    pending = null;
    if (h) await openLink(h);
  }

  function jump(id: string) {
    const t = body?.querySelector<HTMLElement>(`#${CSS.escape(id)}`) ?? body?.querySelector<HTMLElement>(`#${CSS.escape('md-' + id)}`);
    if (!t) return;
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
    t.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'start' });
    if (!t.hasAttribute('tabindex')) t.setAttribute('tabindex', '-1');
    t.focus({ preventScroll: true });
    if (doc?.toc.some((x) => x.id === t.id)) active = t.id;
  }

  function click(e: MouseEvent) {
    const a = (e.target as Element | null)?.closest?.('a');
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute('href') ?? '';
    if (href.startsWith('#')) {
      let id = href.slice(1);
      try {
        id = decodeURIComponent(id);
      } catch {
      }
      jump(id);
    } else ask(safeHref(href));
  }

  let raf = 0;
  function scrolled() {
    if (raf || !doc?.toc.length) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      if (!scroller || !body || !doc) return;
      const top = scroller.getBoundingClientRect().top + 96;
      let cur: string | null = doc.toc[0].id;
      for (const t of doc.toc) {
        const h = body.querySelector(`#${CSS.escape(t.id)}`);
        if (!h) continue;
        if (h.getBoundingClientRect().top <= top) cur = t.id;
        else break;
      }
      active = cur;
    });
  }
  $effect(() => {
    if (doc?.toc.length) active = doc.toc[0].id;
  });

  function toggleToc() {
    if (wide) tocOpen = !tocOpen;
    else tocSlip = !tocSlip;
  }
</script>

{#snippet outline()}
  <nav class="md-toc" class:slip aria-label={m.vw_md_toc()} data-testid="md-toc">
    <div class="md-toc-head">
      <span class="md-toc-title">{m.vw_md_toc()}</span>
      {#if slip}
        <button type="button" class="btn ghost icon md-toc-close" aria-label={m.vw_cancel()} onclick={() => (tocSlip = false)}><X size={15} aria-hidden="true" /></button>
      {/if}
    </div>
    <ol>
      {#each doc?.toc ?? [] as t (t.id)}
        <li class:top={t.level === minLevel} style:--lvl={Math.min(3, t.level - minLevel)}>
          <a
            href={`#${t.id}`}
            aria-current={active === t.id ? 'location' : undefined}
            onclick={(e) => {
              e.preventDefault();
              jump(t.id);
              tocSlip = false;
            }}>{t.text}</a
          >
        </li>
      {/each}
    </ol>
  </nav>
{/snippet}

{#if big || source}
  <div class="vw md-vw" data-testid="viewer-markdown" data-mode="source">
    <TextViewer {src} {node} lang={null} {onfail} />
    {#if !big}
      <div class="md-switch">
        <button type="button" class="btn ghost" onclick={() => (source = false)}><BookOpen size={15} aria-hidden="true" /> {m.vw_md_rendered()}</button>
      </div>
    {/if}
  </div>
{:else}
  <div
    class="vw md-vw"
    class:with-toc={column}
    bind:clientWidth={width}
    data-testid="viewer-markdown"
    data-mode="rendered"
    data-math={mathState}
    data-mermaid={mermaidState}
    data-pictures={pictures}
  >
    {#if column}{@render outline()}{/if}
    <div class="vw-scroll" data-testid="viewer-scroll" bind:this={scroller} onscroll={scrolled}>
      {#if error}
        <p class="hint vw-state">{m.text_error()}</p>
      {:else if doc === null}
        <div class="vw-sheet vw-loading nv-enter" data-state="loading">
          <span class="visually-hidden">{m.fv_reading()}</span>
          <span class="vw-skeleton" aria-hidden="true" use:skeleton={{ lines: 9, seed: 3, widths: ['44%', '0%', '100%', '92%', '86%', '0%', '100%', '70%', '52%'] }}></span>
        </div>
      {:else}
        <article class="vw-sheet md">
          {#if doc.front}
            <dl class="md-front" aria-label={m.vw_md_front()} data-testid="md-front">
              {#each doc.front as [k, v], i (i)}
                <div><dt>{k}</dt><dd>{v}</dd></div>
              {/each}
            </dl>
          {/if}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="md-body" bind:this={body} onclick={click} onauxclick={(e) => e.preventDefault()}>
            {@html doc.html}
          </div>
          {#if mermaidState === 'error'}
            <p class="vw-note" data-testid="md-diagram-error">{m.vw_md_diagram_error()}</p>
          {/if}
        </article>
      {/if}
    </div>
    {#if slip}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="md-toc-scrim" onclick={() => (tocSlip = false)}></div>
      {@render outline()}
    {/if}
    {#if doc}
      <div class="vw-bar nv-enter" role="toolbar" aria-label={m.vw_text_tools()}>
        {#if hasToc}
          <button type="button" class="btn ghost" aria-pressed={column || slip} onclick={toggleToc}><TableOfContents size={15} aria-hidden="true" /> {m.vw_md_toc()}</button>
          <span class="vw-bar-sep" aria-hidden="true"></span>
        {/if}
        <button type="button" class="btn ghost" aria-pressed="false" onclick={() => (source = true)}><CodeXml size={15} aria-hidden="true" /> {m.vw_md_source()}</button>
      </div>
    {/if}
    {#if pending}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="link-confirm nv-enter"
        role="alertdialog"
        aria-labelledby="vw-link-q"
        aria-describedby="vw-link-url"
        tabindex="-1"
        data-testid="link-confirm"
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            e.preventDefault();
            e.stopPropagation();
            pending = null;
          }
        }}
      >
        <p id="vw-link-q" class="q">{m.vw_link_question()}</p>
        <p id="vw-link-url" class="url vw-mono">{pending}</p>
        <div class="vw-actions">
          <button type="button" class="btn ghost" bind:this={cancelBtn} onclick={() => (pending = null)}>{m.vw_cancel()}</button>
          <button type="button" class="btn primary" onclick={confirmOpen}>{m.vw_link_open()}</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .md-vw {
    position: relative;
  }
  .md-vw.with-toc {
    display: grid;
    grid-template-columns: 236px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
  }
  .md-switch {
    position: absolute;
    top: var(--space-3);
    right: var(--space-4);
    z-index: 2;
  }

  .md-toc {
    min-height: 0;
    overflow: auto;
    padding: var(--space-5) var(--space-3) var(--space-7) var(--space-4);
    font-size: 13px;
    line-height: 1.35;
    overscroll-behavior: contain;
  }
  .md-toc.slip {
    position: absolute;
    top: var(--space-3);
    left: var(--space-3);
    bottom: calc(var(--space-7) + 16px);
    z-index: 4;
    width: min(280px, calc(100% - 32px));
    padding: var(--space-4) var(--space-3);
    border-radius: var(--radius-lg);
    background: var(--paper-hi);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--key-edge),
      var(--shadow-pop);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateX(-8px);
    --enter-to: translateX(0);
  }
  @media (prefers-reduced-motion: reduce) {
    .md-toc.slip {
      animation: none;
    }
  }
  .md-toc-scrim {
    position: absolute;
    inset: 0;
    z-index: 3;
  }
  .md-toc-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 28px;
    margin: 0 0 var(--space-2) 10px;
  }
  .md-toc-title {
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-3);
  }
  .md-toc ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .md-toc li {
    margin: 0;
  }
  .md-toc a {
    position: relative;
    display: block;
    padding: 5px 8px 5px calc(10px + var(--lvl, 0) * 13px);
    border-radius: var(--radius-sm);
    color: var(--ink-2);
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color var(--dur-micro) var(--ease-out);
  }
  .md-toc li.top a {
    color: var(--ink);
    font-weight: 500;
  }
  .md-toc a:hover {
    color: var(--accent);
  }
  .md-toc a[aria-current] {
    color: var(--accent);
    font-weight: 600;
  }
  .md-toc a[aria-current]::before {
    content: '';
    position: absolute;
    left: 0;
    top: 6px;
    bottom: 6px;
    width: 2px;
    border-radius: 2px;
    background: var(--accent);
  }
  .md-toc a:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }

  .md {
    max-width: 780px;
    font-size: 15.5px;
    line-height: 1.72;
    color: var(--ink);
    overflow-wrap: break-word;
    font-feature-settings: 'kern', 'liga';
  }
  .md-body > :global(:first-child) {
    margin-top: 0;
  }
  .md-body :global(:is(h1, h2, h3, h4, h5, h6)) {
    font-family: var(--font-display);
    font-weight: 500;
    line-height: 1.25;
    margin: 1.7em 0 0.55em;
    color: var(--title-ink, var(--ink));
    scroll-margin-top: var(--space-5);
    text-wrap: balance;
  }
  .md-body :global(:is(h1, h2, h3, h4, h5, h6):focus) {
    outline: none;
  }
  .md-body :global(h1) {
    font-size: 2.15em;
    letter-spacing: -0.012em;
    margin-bottom: 0.6em;
  }
  .md-body :global(h2) {
    font-size: 1.55em;
    padding-bottom: 0.3em;
    border-bottom: 1px solid var(--paper-rule);
    background: linear-gradient(var(--accent), var(--accent)) left bottom / 2.4em 2px no-repeat;
  }
  .md-body :global(h3) {
    font-size: 1.25em;
  }
  .md-body :global(:is(h4, h5, h6)) {
    font-family: var(--font-ui);
    font-size: 0.82em;
    font-weight: 650;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-2);
  }
  .md-body :global(:is(p, ul, ol, dl, blockquote, pre, .md-table-wrap, .md-mermaid)) {
    margin: 0 0 1.05em;
  }
  .md-body :global(:is(ul, ol)) {
    padding-left: 1.55em;
  }
  .md-body :global(li) {
    margin: 0.15em 0;
  }
  .md-body :global(li::marker) {
    color: color-mix(in oklab, var(--accent) 70%, var(--ink-3));
  }
  .md-body :global(ol > li::marker) {
    font-variant-numeric: tabular-nums;
    font-size: 0.92em;
  }
  .md-body :global(li > :is(ul, ol)) {
    margin: 0.15em 0 0.3em;
  }
  .md-body :global(li > p) {
    margin: 0 0 0.3em;
  }

  .md-body :global(li.md-task) {
    list-style: none;
    position: relative;
  }
  .md-body :global(.md-box) {
    position: absolute;
    left: -1.5em;
    top: 0.32em;
    width: 15px;
    height: 15px;
    border-radius: 4px;
    box-shadow: inset 0 0 0 1.5px color-mix(in oklab, var(--accent) 75%, var(--paper-rule-2, var(--paper-rule)));
    background: var(--paper-hi);
  }
  .md-body :global(.md-box[data-done='1']) {
    background: var(--accent);
    box-shadow: none;
  }
  .md-body :global(.md-box[data-done='1'])::after {
    content: '';
    position: absolute;
    inset: 0;
    background: var(--key-accent-ink, #fff);
    mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'%3E%3Cpath d='M4 8.4l2.6 2.6L12 5.4' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E")
      center / 100% no-repeat;
  }
  .md-body :global(li.md-task:has(.md-box[data-done='1'])) {
    color: var(--ink-3);
  }

  .md-body :global(blockquote:not(.md-alert)) {
    padding: 0.1em 0 0.1em 1.15em;
    border-left: 3px solid color-mix(in oklab, var(--accent) 65%, var(--paper-rule));
    font-family: var(--font-display);
    font-size: 1.08em;
    font-style: italic;
    color: var(--ink-2);
  }
  .md-body :global(blockquote > :last-child) {
    margin-bottom: 0;
  }

  .md-body :global(.md-alert) {
    --al: var(--accent);
    position: relative;
    padding: 0.75em 1em 0.8em 1.1em;
    border-radius: var(--radius-md);
    border-left: 3px solid var(--al);
    background: color-mix(in oklab, var(--al) 7%, var(--paper-hi));
    font-size: 0.95em;
  }
  .md-body :global(.md-alert-warning) {
    --al: var(--gold-mid, #c8a961);
  }
  .md-body :global(.md-alert-caution) {
    --al: var(--danger);
  }
  .md-body :global(.md-alert-title) {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0 0 0.25em;
    font-size: 0.8em;
    font-weight: 650;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: color-mix(in oklab, var(--al) 80%, var(--ink));
  }
  .md-body :global(.md-alert-title)::before {
    content: '';
    width: 15px;
    height: 15px;
    flex: none;
    background: currentColor;
    mask: var(--al-icon) center / 100% no-repeat;
  }
  .md-body :global(.md-alert-note) {
    --al-icon: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round'%3E%3Ccircle cx='12' cy='12' r='9.5'/%3E%3Cpath d='M12 16v-4.5M12 8h.01'/%3E%3C/svg%3E");
  }
  .md-body :global(.md-alert-tip) {
    --al-icon: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M9 18h6M10 21.5h4M12 2.5a6.5 6.5 0 0 0-4 11.6c.6.5 1 1.2 1 2V16h6v-.1c0-.7.4-1.4 1-1.9a6.5 6.5 0 0 0-4-11.5z'/%3E%3C/svg%3E");
  }
  .md-body :global(.md-alert-important) {
    --al-icon: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z'/%3E%3Cpath d='M12 7v4M12 14h.01'/%3E%3C/svg%3E");
  }
  .md-body :global(.md-alert-warning) {
    --al-icon: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z'/%3E%3Cpath d='M12 9v4M12 17h.01'/%3E%3C/svg%3E");
  }
  .md-body :global(.md-alert-caution) {
    --al-icon: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M7.9 2h8.2L22 7.9v8.2L16.1 22H7.9L2 16.1V7.9z'/%3E%3Cpath d='M12 8v4M12 16h.01'/%3E%3C/svg%3E");
  }

  .md-body :global(code) {
    font-family: var(--font-mono);
    font-size: 0.86em;
    padding: 0.12em 0.38em;
    border-radius: var(--radius-sm);
    background: color-mix(in oklab, var(--accent) 7%, var(--paper-sunk));
    color: color-mix(in oklab, var(--accent) 30%, var(--ink));
  }
  .md-body :global(pre.md-code) {
    position: relative;
    padding: var(--space-4) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    box-shadow: inset 0 1px 2px var(--field-shade);
    overflow-x: auto;
    line-height: 1.6;
  }
  .md-body :global(pre.md-code[data-lang])::before {
    content: attr(data-lang);
    position: absolute;
    top: 7px;
    right: 12px;
    font-family: var(--font-ui);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-4);
  }
  .md-body :global(pre.md-code code) {
    padding: 0;
    background: none;
    color: var(--ink);
    font-size: 12.5px;
  }
  .md-body :global(.t-com) {
    color: var(--ink-3);
    font-style: italic;
  }
  .md-body :global(.t-str) {
    color: color-mix(in oklab, var(--accent) 55%, var(--ink-2));
  }
  .md-body :global(.t-kw) {
    color: var(--accent);
    font-weight: 600;
  }
  .md-body :global(:is(.t-num, .t-lit)) {
    color: color-mix(in oklab, var(--gold-lo) 70%, var(--ink));
  }
  .md-body :global(:is(.t-type, .t-tag)) {
    color: color-mix(in oklab, var(--accent) 35%, var(--ink));
  }
  .md-body :global(:is(.t-meta, .t-attr)) {
    color: color-mix(in oklab, var(--gold-lo) 45%, var(--ink-2));
  }

  .md-body :global(.md-table-wrap) {
    overflow-x: auto;
  }
  .md-body :global(.md-table-wrap table) {
    font-size: 0.9em;
    font-variant-numeric: tabular-nums;
  }
  .md-body :global(.md-table-wrap th) {
    position: static;
    padding: 8px 12px 7px;
    border-bottom: 1.5px solid color-mix(in oklab, var(--accent) 70%, var(--paper-rule));
  }
  .md-body :global(.md-table-wrap td) {
    padding: 7px 12px;
  }
  .md-body :global(.md-table-wrap tbody tr:nth-child(even)) {
    background: color-mix(in oklab, var(--paper-sunk) 45%, transparent);
  }
  .md-body :global(.md-al-center) {
    text-align: center;
  }
  .md-body :global(.md-al-right) {
    text-align: right;
  }

  .md-body :global(mark) {
    padding: 0.05em 0.12em;
    -webkit-box-decoration-break: clone;
    box-decoration-break: clone;
    border-radius: 3px;
    background: color-mix(in oklab, var(--gold) 55%, transparent);
    color: inherit;
  }
  .md-body :global(abbr[title]) {
    text-decoration: underline dotted color-mix(in oklab, var(--accent) 60%, var(--ink-3));
    text-underline-offset: 3px;
    cursor: help;
  }
  .md-body :global(:is(sub, sup)) {
    font-size: 0.72em;
    line-height: 0;
  }
  .md-body :global(hr) {
    height: 1px;
    margin: 2.2em 0;
    border: 0;
    background: var(--paper-rule);
  }

  .md-body :global(a) {
    color: var(--accent);
    text-decoration: underline;
    text-decoration-color: color-mix(in oklab, var(--accent) 40%, transparent);
    text-decoration-thickness: 1px;
    text-underline-offset: 2.5px;
    cursor: pointer;
    transition: text-decoration-color var(--dur-micro) var(--ease-out);
  }
  .md-body :global(a:hover) {
    text-decoration-color: currentColor;
  }
  .md-body :global(a:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
    border-radius: 2px;
  }
  .md-body :global(.md-deadlink) {
    color: var(--ink-2);
  }

  .md-body :global(dt) {
    font-weight: 600;
    margin-top: 0.5em;
  }
  .md-body :global(dd) {
    margin: 0.1em 0 0.4em;
    padding-left: 1em;
    border-left: 2px solid var(--paper-rule);
    color: var(--ink-2);
  }

  .md-body :global(.footnote-ref a) {
    padding: 0 0.15em;
    font-size: 0.95em;
    font-weight: 600;
    text-decoration: none;
  }
  .md-body :global(hr.footnotes-sep) {
    width: 30%;
    margin: 2.4em 0 1em;
  }
  .md-body :global(section.footnotes) {
    font-size: 0.86em;
    color: var(--ink-2);
  }
  .md-body :global(section.footnotes ol) {
    padding-left: 1.3em;
  }
  .md-body :global(section.footnotes li p) {
    margin: 0 0 0.35em;
  }
  .md-body :global(section.footnotes li:target) {
    background: color-mix(in oklab, var(--accent) 8%, transparent);
  }
  .md-body :global(.footnote-backref) {
    text-decoration: none;
    font-family: var(--font-ui);
  }

  .md-body :global(.md-img) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 1px 9px 1px 7px;
    border: 1px dashed var(--paper-rule-2, var(--paper-rule));
    border-radius: var(--radius-sm);
    color: var(--ink-2);
    font-size: 0.88em;
    line-height: 1.6;
  }
  .md-body :global(.md-img)::before {
    content: '';
    width: 13px;
    height: 13px;
    flex: none;
    background: var(--ink-3);
    mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M2 2l20 20M10.4 10.4a2 2 0 0 0 2.8 2.8M21 15V5a2 2 0 0 0-2-2H9M3 7v12a2 2 0 0 0 2 2h14M21 15l-3-3'/%3E%3C/svg%3E")
      center / 100% no-repeat;
  }
  .md-body :global(.md-img-note) {
    color: var(--ink-4);
    font-size: 0.88em;
  }
  .md-body :global(.md-figure) {
    display: block;
    margin: 0.4em 0 0.2em;
    text-align: center;
  }
  .md-body :global(.md-pic) {
    display: block;
    max-width: 100%;
    max-height: 70vh;
    margin: 0 auto;
    border-radius: 3px;
    box-shadow:
      0 0 0 1px rgb(var(--cast) / 0.1),
      0 10px 24px -12px rgb(var(--cast) / 0.45);
  }
  .md-body :global(.md-caption) {
    display: block;
    margin-top: 0.6em;
    font-size: 0.85em;
    font-style: italic;
    color: var(--ink-3);
  }

  .md-body :global(.md-math[data-display='1']) {
    margin: 0 0 1.05em;
    overflow-x: auto;
    overflow-y: hidden;
    text-align: center;
  }
  .md-body :global(.md-math:not([data-rendered])) {
    font-family: var(--font-mono);
    font-size: 0.88em;
    color: var(--ink-3);
  }
  .md-body :global(.katex) {
    font-size: 1.08em;
  }
  .md-body :global(.md-mermaid[data-state='ready']) {
    display: flex;
    justify-content: center;
    padding: var(--space-4);
    border-radius: var(--radius-md);
    border: 1px solid var(--paper-rule);
    overflow-x: auto;
  }
  .md-body :global(.md-mermaid svg) {
    max-width: 100%;
    height: auto;
  }
  .md-body :global(.md-mermaid:not([data-state])) {
    opacity: 0.55;
  }

  .md-front {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-3) var(--space-5);
    margin: 0 0 var(--space-6);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: color-mix(in oklab, var(--accent) 4%, var(--paper-sunk));
    font-size: 13px;
    line-height: 1.4;
  }
  .md-front div {
    min-width: 0;
  }
  .md-front dt {
    margin: 0 0 2px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-3);
  }
  .md-front dd {
    margin: 0;
    color: var(--ink);
    overflow-wrap: anywhere;
  }

  .link-confirm {
    position: absolute;
    left: 0;
    right: 0;
    bottom: var(--space-5);
    z-index: 5;
    width: min(520px, calc(100% - 32px));
    margin: 0 auto;
    padding: var(--space-4) var(--space-5);
    border-radius: var(--radius-lg);
    background: var(--paper-hi);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--key-edge),
      var(--shadow-pop);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateY(8px);
    --enter-to: translateY(0);
  }
  @media (prefers-reduced-motion: reduce) {
    .link-confirm {
      animation: none;
    }
  }
  .q {
    margin: 0 0 var(--space-2);
    font-weight: 600;
  }
  .url {
    margin: 0 0 var(--space-4);
    font-size: var(--text-sm);
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }
  .link-confirm .vw-actions {
    justify-content: flex-end;
  }
</style>
