<!--
  Markdown, typeset on a sheet. Parsed by markdown.ts into a tree rendered here as Svelte
  elements and text nodes: no {@html}, so raw HTML in the file shows as text. Images are not
  fetched (their alt text stands in). A link is not a link to the webview: clicking it asks
  first, then the system browser opens it (viewers.rs open_link: http/https/mailto only).
  "Sorgente" shows the file as text; files over 4 MB open as paged text.
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { BookOpen, CodeXml, ImageOff, Square, SquareCheck } from '@lucide/svelte';
  import type { FsNode } from '../lib/types';
  import type { ByteSource } from './source';
  import { CAPS } from './kinds';
  import { detectEncoding } from './encoding';
  import { parseMarkdown, type Block, type Inline } from './markdown';
  import { languageFor, toLines, tokenize } from './highlight';
  import { openLink } from './actions';
  import { skeleton } from '../lib/reveal';
  import TextViewer from './TextViewer.svelte';
  import type { InfoReason } from './InfoViewer.svelte';

  let { src, node, onfail }: { src: ByteSource; node: FsNode; onfail: (r: InfoReason) => void } = $props();

  let blocks = $state.raw<Block[] | null>(null);
  let source = $state(false);
  let big = $state(false);
  let error = $state(false);
  let pending = $state<string | null>(null);
  let cancelBtn: HTMLButtonElement | undefined = $state();

  $effect(() => {
    const s = src;
    blocks = null;
    error = false;
    let gone = false;
    (async () => {
      const size = await s.size();
      if (size > CAPS.markdown) {
        big = true;
        return;
      }
      const bytes = await s.read(0, size);
      if (gone) return;
      const d = detectEncoding(bytes, false);
      if (d.binary) return onfail('binary');
      blocks = parseMarkdown(new TextDecoder(d.encoding).decode(bytes.subarray(d.bom)));
    })().catch(() => {
      if (!gone) error = true;
    });
    return () => {
      gone = true;
    };
  });

  const ALIAS: Record<string, string> = { javascript: 'js', typescript: 'ts', shell: 'sh', console: 'sh', python: 'py', rust: 'rs', yml: 'yaml', golang: 'go', 'c++': 'cpp', csharp: 'cs', html: 'html', xml: 'xml' };
  const codeLines = (lang: string, s: string) => toLines(tokenize(s, lang ? languageFor(`x.${ALIAS[lang] ?? lang}`) : null));

  function ask(href: string | null) {
    if (!href) return;
    pending = href;
    // Cancel has the focus: a stray Enter / Space never leaves the app
    queueMicrotask(() => cancelBtn?.focus());
  }
  async function confirmOpen() {
    const h = pending;
    pending = null;
    if (h) await openLink(h);
  }
</script>

{#snippet inlines(list: Inline[])}
  {#each list as n, k (k)}
    {#if n.t === 'text'}{n.s}{:else if n.t === 'code'}<code>{n.s}</code>{:else if n.t === 'em'}<em>{@render inlines(n.c)}</em>{:else if n.t === 'strong'}<strong>{@render inlines(n.c)}</strong>{:else if n.t === 'del'}<del>{@render inlines(n.c)}</del>{:else if n.t === 'br'}<br />{:else if n.t === 'img'}<span class="md-img" title={n.src}><ImageOff size={14} aria-hidden="true" /> {n.alt || m.vw_md_image()} <span class="md-img-note">{m.vw_md_image_blocked()}</span></span>{:else if n.t === 'link'}{#if n.href}<span
          class="md-link"
          role="link"
          tabindex="0"
          title={n.href}
          data-href={n.href}
          onclick={(e) => {
            e.preventDefault();
            ask(n.href);
          }}
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              e.stopPropagation();
              ask(n.href);
            }
          }}>{@render inlines(n.c)}</span
        >{:else}<span class="md-deadlink">{@render inlines(n.c)}</span>{/if}{/if}
  {/each}
{/snippet}

{#snippet render(list: Block[])}
  {#each list as b, k (k)}
    {#if b.t === 'h'}
      <svelte:element this={`h${b.level}`} class="md-h">{@render inlines(b.c)}</svelte:element>
    {:else if b.t === 'p'}
      <p>{@render inlines(b.c)}</p>
    {:else if b.t === 'code'}
      <pre class="md-code"><code
          >{#each codeLines(b.lang, b.s) as line, i (i)}{#if i > 0}{'\n'}{/if}{#each line as t, j (j)}{#if t.c}<span class="t-{t.c}">{t.s}</span>{:else}{t.s}{/if}{/each}{/each}</code
        ></pre>
    {:else if b.t === 'quote'}
      <blockquote>{@render render(b.c)}</blockquote>
    {:else if b.t === 'hr'}
      <hr />
    {:else if b.t === 'list'}
      {#if b.ordered}
        <ol start={b.start}>{#each b.items as it, i (i)}<li>{@render render(it.c)}</li>{/each}</ol>
      {:else}
        <ul>
          {#each b.items as it, i (i)}
            <li class:task={it.task !== null}>
              {#if it.task !== null}<span class="box" role="img" aria-label={it.task ? m.vw_md_done() : m.vw_md_todo()}>{#if it.task}<SquareCheck size={15} />{:else}<Square size={15} />{/if}</span>{/if}
              {@render render(it.c)}
            </li>
          {/each}
        </ul>
      {/if}
    {:else if b.t === 'table'}
      <div class="md-table-wrap">
        <table class="vw-table">
          <thead><tr>{#each b.head as c, i (i)}<th style:text-align={b.align[i]}>{@render inlines(c)}</th>{/each}</tr></thead>
          <tbody>{#each b.rows as r, i (i)}<tr>{#each r as c, j (j)}<td style:text-align={b.align[j]}>{@render inlines(c)}</td>{/each}</tr>{/each}</tbody>
        </table>
      </div>
    {/if}
  {/each}
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
  <div class="vw md-vw" data-testid="viewer-markdown" data-mode="rendered">
    <div class="vw-scroll" data-testid="viewer-scroll">
      {#if error}
        <p class="hint vw-state">{m.text_error()}</p>
      {:else if blocks === null}
        <div class="vw-sheet vw-loading nv-enter" data-state="loading">
          <span class="visually-hidden">{m.fv_reading()}</span>
          <span class="vw-skeleton" aria-hidden="true" use:skeleton={{ lines: 9, seed: 3, widths: ['44%', '0%', '100%', '92%', '86%', '0%', '100%', '70%', '52%'] }}></span>
        </div>
      {:else}
        <article class="vw-sheet md">{@render render(blocks)}</article>
      {/if}
    </div>
    {#if blocks}
      <div class="vw-bar nv-enter" role="toolbar" aria-label={m.vw_text_tools()}>
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
  .md-switch {
    position: absolute;
    top: var(--space-3);
    right: var(--space-4);
    z-index: 2;
  }
  /* the reading measure: the UI sans at a book size, Bodoni for the titles */
  .md {
    max-width: 760px;
    font-size: 15px;
    line-height: 1.7;
    color: var(--ink);
    overflow-wrap: break-word;
  }
  .md :global(.md-h) {
    font-family: var(--font-display);
    font-weight: 500;
    line-height: 1.3;
    margin: 1.6em 0 0.5em;
    color: var(--title-ink, var(--ink));
  }
  .md :global(h1.md-h) {
    font-size: 2em;
  }
  .md :global(h2.md-h) {
    font-size: 1.55em;
    padding-bottom: 0.25em;
    border-bottom: 1px solid var(--paper-rule);
  }
  .md :global(h3.md-h) {
    font-size: 1.25em;
  }
  .md :global(h4.md-h),
  .md :global(h5.md-h),
  .md :global(h6.md-h) {
    font-family: var(--font-ui);
    font-size: 1em;
    font-weight: 600;
  }
  .md :global(p),
  .md :global(ul),
  .md :global(ol),
  .md :global(blockquote),
  .md :global(pre) {
    margin: 0 0 1em;
  }
  .md :global(ul),
  .md :global(ol) {
    padding-left: 1.6em;
  }
  .md :global(li > p) {
    margin: 0 0 0.3em;
  }
  .md :global(li.task) {
    list-style: none;
    position: relative;
  }
  .md :global(.box) {
    position: absolute;
    left: -1.45em;
    top: 0.25em;
    color: var(--accent);
  }
  .md :global(blockquote) {
    padding: 0.1em 1.1em;
    border-left: 3px solid color-mix(in oklab, var(--accent) 60%, var(--paper-rule));
    color: var(--ink-2);
  }
  .md :global(code) {
    font-family: var(--font-mono);
    font-size: 0.88em;
    padding: 0.1em 0.35em;
    border-radius: var(--radius-sm);
    background: var(--paper-sunk);
  }
  .md :global(pre.md-code) {
    padding: var(--space-4);
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    box-shadow: inset 0 1px 2px var(--field-shade);
    overflow-x: auto;
    line-height: 1.55;
  }
  .md :global(pre.md-code code) {
    padding: 0;
    background: none;
    font-size: 12.5px;
  }
  .md :global(hr) {
    height: 1px;
    margin: 2em 0;
    border: 0;
    background: var(--paper-rule);
  }
  .md :global(.md-link) {
    color: var(--accent);
    text-decoration: underline;
    text-decoration-thickness: 1px;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .md :global(.md-link:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
    border-radius: 2px;
  }
  .md :global(.md-img) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 8px;
    border: 1px dashed var(--paper-rule-2, var(--paper-rule));
    border-radius: var(--radius-sm);
    color: var(--ink-2);
    font-size: 0.9em;
  }
  .md :global(.md-img-note) {
    color: var(--ink-4);
    font-size: 0.85em;
  }
  .md :global(.md-table-wrap) {
    overflow-x: auto;
    margin: 0 0 1em;
  }
  .md :global(.t-com) {
    color: var(--ink-3);
    font-style: italic;
  }
  .md :global(.t-str) {
    color: color-mix(in oklab, var(--accent) 55%, var(--ink-2));
  }
  .md :global(.t-kw) {
    color: var(--accent);
    font-weight: 600;
  }
  .md :global(.t-num),
  .md :global(.t-lit) {
    color: color-mix(in oklab, var(--gold-lo) 70%, var(--ink));
  }
  .md :global(.t-type),
  .md :global(.t-tag) {
    color: color-mix(in oklab, var(--accent) 35%, var(--ink));
  }
  /* the question before a link leaves the app: a slip over the foot of the sheet */
  .link-confirm {
    position: absolute;
    left: 0;
    right: 0;
    bottom: var(--space-5);
    z-index: 3;
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
  /* nothing above the first block but the sheet's own margin (after the heading rules) */
  .md > :global(:first-child) {
    margin-top: 0;
  }
</style>
