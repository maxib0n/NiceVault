<!--
  Word/Writer, Excel/Calc and PowerPoint/Impress files, read-only: the document's properties
  and its words (paragraphs and headings, the first sheet as a table, slide by slide). The
  layout is not reproduced — "Apri con app esterna" (mounted) or Esporta for that.
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { skeleton } from '../lib/reveal';
  import { formatNumber } from '../lib/format';
  import type { ByteSource } from './source';
  import { readOffice, type OfficeDoc } from './office';
  import type { InfoReason } from './InfoViewer.svelte';

  let { src, ext, onfail }: { src: ByteSource; ext: string; onfail: (r: InfoReason) => void } = $props();

  let doc = $state.raw<OfficeDoc | null>(null);

  $effect(() => {
    const s = src;
    const ac = new AbortController();
    doc = null;
    readOffice(s, ext, ac.signal).then(
      (d) => !ac.signal.aborted && (doc = d),
      () => !ac.signal.aborted && onfail('error'),
    );
    return () => ac.abort();
  });

  const when = (iso?: string) => {
    if (!iso) return '';
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
  };
  const width = $derived(doc?.body.kind === 'sheet' ? Math.max(1, ...doc.body.rows.map((r) => r.length)) : 1);
  /** A, B, … Z, AA, AB … */
  const colName = (k: number): string => (k >= 26 ? colName(Math.floor(k / 26) - 1) : '') + String.fromCharCode(65 + (k % 26));
  const isEmpty = $derived(
    doc !== null &&
      (doc.body.kind === 'text' ? doc.body.blocks.every((b) => !b.s.trim()) : doc.body.kind === 'slides' ? doc.body.slides.every((s) => !s.length) : doc.body.rows.length === 0),
  );
</script>

<div class="vw office-vw" data-testid="viewer-office" data-kind={doc?.body.kind ?? 'loading'}>
  <div class="vw-scroll" data-testid="viewer-scroll">
    {#if doc === null}
      <div class="vw-sheet vw-loading nv-enter" data-state="loading">
        <span class="visually-hidden">{m.fv_reading()}</span>
        <span class="vw-skeleton" aria-hidden="true" use:skeleton={{ lines: 10, seed: 9, widths: ['50%', '0%', '100%', '96%', '88%', '100%', '0%', '100%', '76%', '40%'] }}></span>
      </div>
    {:else}
      <article class="vw-sheet" class:wide={doc.body.kind === 'sheet'}>
        <p class="vw-eyebrow">{m.vw_office_readonly()}</p>
        {#if doc.meta.title || doc.meta.author || doc.meta.modified || doc.meta.app}
          <dl class="vw-facts props">
            {#if doc.meta.title}<dt>{m.vw_f_title()}</dt><dd>{doc.meta.title}</dd>{/if}
            {#if doc.meta.author}<dt>{m.vw_f_author()}</dt><dd>{doc.meta.author}</dd>{/if}
            {#if doc.meta.modified}<dt>{m.vw_f_modified()}</dt><dd>{when(doc.meta.modified)}</dd>{/if}
            {#if doc.meta.app}<dt>{m.vw_f_app()}</dt><dd>{doc.meta.app}</dd>{/if}
          </dl>
          <hr class="vw-rule" />
        {/if}

        {#if isEmpty}
          <p class="hint">{m.vw_office_empty()}</p>
        {:else if doc.body.kind === 'text'}
          <div class="doc">
            {#each doc.body.blocks as b, i (i)}
              {#if b.level > 0}
                <svelte:element this={`h${Math.min(b.level + 1, 6)}`} class="doc-h">{b.s}</svelte:element>
              {:else if b.s.trim()}
                <p>{b.s}</p>
              {/if}
            {/each}
          </div>
        {:else if doc.body.kind === 'slides'}
          <ol class="slides">
            {#each doc.body.slides as slide, i (i)}
              <li class="slide" data-testid="viewer-slide">
                <span class="slide-n vw-mono">{m.vw_slide({ n: String(i + 1) })}</span>
                {#each slide as line, k (k)}
                  {#if k === 0}<h3 class="slide-title">{line}</h3>{:else}<p>{line}</p>{/if}
                {/each}
              </li>
            {/each}
          </ol>
        {:else}
          {#if doc.body.sheet}<p class="sheet-name">{doc.body.sheet}{#if doc.body.sheets > 1}<span class="sub"> · {m.vw_sheets_more({ n: String(doc.body.sheets - 1) })}</span>{/if}</p>{/if}
          <div class="grid-wrap">
            <table class="vw-table grid" data-testid="viewer-sheet">
              <thead>
                <tr><th class="num"></th>{#each Array.from({ length: width }, (_, k) => k) as k (k)}<th>{colName(k)}</th>{/each}</tr>
              </thead>
              <tbody>
                {#each doc.body.rows as r, i (i)}
                  <tr><th class="num" scope="row">{i + 1}</th>{#each Array.from({ length: width }, (_, k) => r[k] ?? '') as c, k (k)}<td class:num={c !== '' && !Number.isNaN(Number(c))}>{c}</td>{/each}</tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
        {#if doc.truncated}<p class="vw-note">{m.vw_office_truncated()}</p>{/if}
        {#if doc.body.kind === 'sheet'}<p class="vw-note">{m.vw_rows({ n: formatNumber(doc.body.rows.length) })}</p>{/if}
      </article>
    {/if}
  </div>
</div>

<style>
  .props {
    margin-bottom: 0;
  }
  .doc {
    font-size: 15px;
    line-height: 1.7;
    max-width: 72ch;
  }
  .doc p {
    margin: 0 0 0.9em;
    white-space: pre-wrap;
  }
  .doc :global(.doc-h) {
    font-family: var(--font-display);
    font-weight: 500;
    line-height: 1.3;
    margin: 1.4em 0 0.5em;
  }
  .slides {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-4);
  }
  .slide {
    position: relative;
    padding: var(--space-5) var(--space-5) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--paper);
    box-shadow:
      0 0 0 1px var(--paper-rule),
      0 2px 6px -2px rgb(var(--cast) / 0.18);
    aspect-ratio: 16 / 9;
    overflow: hidden;
  }
  .slide-n {
    position: absolute;
    top: 10px;
    right: 14px;
    font-size: var(--text-xs);
    color: var(--ink-4);
  }
  .slide-title {
    margin: 0 0 var(--space-3);
    font-family: var(--font-display);
    font-weight: 500;
    font-size: 1.35em;
  }
  .slide p {
    margin: 0 0 0.4em;
    color: var(--ink-2);
  }
  .sheet-name {
    margin: 0 0 var(--space-3);
    font-weight: 600;
  }
  .sub {
    font-weight: 400;
    color: var(--ink-3);
  }
  .grid-wrap {
    overflow: auto;
    max-height: 60vh;
  }
  .grid td,
  .grid th {
    border-right: 1px solid var(--paper-rule);
    white-space: nowrap;
    max-width: 28ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grid th.num {
    color: var(--ink-4);
  }
</style>
