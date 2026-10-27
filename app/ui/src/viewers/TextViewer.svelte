<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, Table, TextWrap } from '@lucide/svelte';
  import type { FsNode } from '../lib/types';
  import { extension } from '../lib/filetype';
  import { formatNumber } from '../lib/format';
  import { skeleton } from '../lib/reveal';
  import type { ByteSource } from './source';
  import { detectEncoding, encodingLabel, type Detected } from './encoding';
  import { LANGUAGE_LABEL, toLines, tokenize, type Token } from './highlight';
  import { TEXT_PAGE } from './kinds';
  import { parseCsv } from './csv';
  import type { InfoReason } from './InfoViewer.svelte';

  let { src, node, lang, onfail }: { src: ByteSource; node: FsNode; lang: string | null; onfail: (r: InfoReason) => void } = $props();

  const LOOK = 64 * 1024;
  const ext = $derived(extension(node.name));
  const csv = $derived(ext === 'csv' || ext === 'tsv');

  let size = $state(0);
  let enc = $state<Detected | null>(null);
  let pageNo = $state(0);
  let pages = $state(1);
  let lines = $state.raw<Token[][] | null>(null);
  let firstLine = $state<number | null>(1);
  let wrap = $state(true);
  let table = $state(true);
  let error = $state(false);
  let scroller: HTMLDivElement | undefined = $state();
  const lineStarts = new Map<number, number>([[0, 1]]);

  const isNl = (b: Uint8Array, i: number, e: Detected) =>
    e.encoding === 'utf-16le' ? b[i] === 10 && b[i + 1] === 0 : e.encoding === 'utf-16be' ? b[i] === 0 && b[i + 1] === 10 : b[i] === 10;
  const unit = (e: Detected) => (e.encoding.startsWith('utf-16') ? 2 : 1);

  function cutAfter(raw: Uint8Array, from: number, e: Detected, eof: boolean): number {
    const u = unit(e);
    from -= from % u;
    if (from >= raw.length) return raw.length;
    for (let i = from; i + u - 1 < raw.length; i += u) if (isNl(raw, i, e)) return i + u;
    if (eof) return raw.length;
    let i = from;
    if (e.encoding === 'utf-8') while (i > 0 && (raw[i] & 0xc0) === 0x80) i--;
    return i;
  }

  async function show(k: number) {
    const e = enc!;
    const start = k * TEXT_PAGE;
    const want = Math.min(size, start + TEXT_PAGE + LOOK);
    const raw = await src.read(start, want);
    const eof = want >= size;
    const from = k === 0 ? e.bom : cutAfter(raw, 0, e, false);
    const to = start + TEXT_PAGE >= size ? raw.length : cutAfter(raw, TEXT_PAGE, e, eof);
    const text = new TextDecoder(e.encoding).decode(raw.subarray(from, Math.max(from, to)));
    const ls = toLines(tokenize(text, lang));
    if (ls.length > 1 && ls[ls.length - 1].length === 0 && text.endsWith('\n')) ls.pop();
    const nl = (text.match(/\n/g) ?? []).length;
    const known = lineStarts.get(k);
    if (known !== undefined) lineStarts.set(k + 1, known + nl);
    firstLine = known ?? null;
    lines = ls;
    pageNo = k;
    scroller?.scrollTo({ top: 0 });
  }

  $effect(() => {
    const s = src;
    lines = null;
    error = false;
    enc = null;
    let gone = false;
    (async () => {
      size = await s.size();
      pages = Math.max(1, Math.ceil(size / TEXT_PAGE));
      const head = await s.read(0, Math.min(size, 64 * 1024));
      if (gone) return;
      const d = detectEncoding(head, size > head.length);
      if (d.binary) return onfail('binary');
      enc = d;
      await show(0);
    })().catch(() => {
      if (!gone) error = true;
    });
    return () => {
      gone = true;
    };
  });

  const go = (k: number) => {
    if (k < 0 || k >= pages || !enc) return;
    void show(k).catch(() => (error = true));
  };

  const plain = $derived(lines ? lines.map((l) => l.map((t) => t.s).join('')).join('\n') : '');
  const rows = $derived(csv && table && lines ? parseCsv(plain, ext === 'tsv' ? '\t' : null) : null);

  function key(e: KeyboardEvent) {
    if (pages <= 1 || e.ctrlKey || e.metaKey || e.altKey) return;
    if ((e.target as HTMLElement | null)?.closest('input, textarea')) return;
    if (e.key === 'PageDown' && e.shiftKey) go(pageNo + 1);
    else if (e.key === 'PageUp' && e.shiftKey) go(pageNo - 1);
    else return;
    e.preventDefault();
  }
</script>

<svelte:window onkeydown={key} />

<div class="vw text-vw" data-testid="viewer-text" data-encoding={enc?.encoding} data-lang={lang ?? ''} data-page={pageNo + 1} data-pages={pages}>
  <div class="vw-scroll" bind:this={scroller} data-testid="viewer-scroll">
    {#if error}
      <p class="hint vw-state">{m.text_error()}</p>
    {:else if lines === null}
      <div class="vw-sheet vw-loading nv-enter" data-state="loading">
        <span class="visually-hidden">{m.fv_reading()}</span>
        <span class="vw-skeleton" aria-hidden="true" use:skeleton={{ lines: 9, seed: 5, widths: ['64%', '92%', '100%', '86%', '40%', '96%', '100%', '78%', '52%'] }}></span>
      </div>
    {:else if rows}
      <div class="vw-sheet wide table-sheet">
        <table class="vw-table" data-testid="viewer-csv">
          {#if rows.rows.length}
            <thead><tr>{#each rows.rows[0] as c, k (k)}<th>{c}</th>{/each}</tr></thead>
          {/if}
          <tbody>
            {#each rows.rows.slice(1) as r, i (i)}
              <tr>{#each r as c, k (k)}<td>{c}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
        {#if rows.truncated}<p class="vw-note">{m.vw_csv_truncated({ n: formatNumber(rows.rows.length) })}</p>{/if}
      </div>
    {:else}
      <div class="vw-sheet wide code-sheet" class:numbered={firstLine !== null}>
        <div class="code" class:wrap data-testid="viewer-code" style:--digits={String((firstLine ?? 1) + lines.length).length}>
          {#each lines as line, i (i)}
            <div class="ln" data-n={firstLine === null ? null : firstLine + i}>
              {#each line as t, k (k)}{#if t.c}<span class="t-{t.c}">{t.s}</span>{:else}{t.s}{/if}{/each}{#if line.length === 0}{'​'}{/if}
            </div>
          {/each}
        </div>
        {#if pages > 1 && pageNo < pages - 1}
          <button type="button" class="btn next-page" onclick={() => go(pageNo + 1)}>{m.vw_next_text_page()} <ChevronRight size={15} /></button>
        {/if}
      </div>
    {/if}
  </div>
  {#if enc && lines}
    <div class="vw-bar nv-enter" role="toolbar" aria-label={m.vw_text_tools()}>
      {#if pages > 1}
        <button type="button" class="btn ghost icon" aria-label={m.vw_first_page()} title={m.vw_first_page()} disabled={pageNo === 0} onclick={() => go(0)}><ChevronsLeft size={15} /></button>
        <button type="button" class="btn ghost icon" aria-label={m.vw_prev_page()} title={m.vw_prev_page()} disabled={pageNo === 0} onclick={() => go(pageNo - 1)}><ChevronLeft size={15} /></button>
        <span class="vw-bar-label" data-testid="viewer-text-page">{m.vw_text_page({ n: String(pageNo + 1), total: String(pages) })}</span>
        <button type="button" class="btn ghost icon" aria-label={m.vw_next_page()} title={m.vw_next_page()} disabled={pageNo >= pages - 1} onclick={() => go(pageNo + 1)}><ChevronRight size={15} /></button>
        <button type="button" class="btn ghost icon" aria-label={m.vw_last_page()} title={m.vw_last_page()} disabled={pageNo >= pages - 1} onclick={() => go(pages - 1)}><ChevronsRight size={15} /></button>
        <span class="vw-bar-sep"></span>
      {/if}
      <span class="vw-bar-label" data-testid="viewer-encoding">{encodingLabel(enc.encoding)}</span>
      {#if lang}<span class="vw-bar-label">{LANGUAGE_LABEL[lang]}</span>{/if}
      <span class="vw-bar-sep"></span>
      {#if csv}
        <button type="button" class="btn ghost icon" aria-label={m.vw_as_table()} title={m.vw_as_table()} aria-pressed={table} onclick={() => (table = !table)}><Table size={15} /></button>
      {/if}
      {#if !rows}
        <button type="button" class="btn ghost icon" aria-label={m.vw_wrap()} title={m.vw_wrap()} aria-pressed={wrap} onclick={() => (wrap = !wrap)}><TextWrap size={15} /></button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .code-sheet {
    padding: var(--space-5) var(--space-5) var(--space-6);
  }
  .code {
    font-family: var(--font-mono);
    font-size: 12.5px;
    line-height: 1.6;
    white-space: pre;
    overflow-x: auto;
    tab-size: 4;
    color: var(--ink);
  }
  .code.wrap {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .ln {
    position: relative;
    min-height: 1.6em;
  }
  .numbered .ln {
    padding-left: calc(var(--digits) * 1ch + 20px);
  }
  .numbered .ln::before {
    content: attr(data-n);
    position: absolute;
    left: 0;
    width: calc(var(--digits) * 1ch);
    text-align: right;
    color: var(--ink-4);
    user-select: none;
    -webkit-user-select: none;
  }
  .t-com {
    color: var(--ink-3);
    font-style: italic;
  }
  .t-str {
    color: color-mix(in oklab, var(--accent) 55%, var(--ink-2));
  }
  .t-num,
  .t-lit {
    color: color-mix(in oklab, var(--gold-lo) 70%, var(--ink));
  }
  .t-kw {
    color: var(--accent);
    font-weight: 600;
  }
  .t-type,
  .t-tag {
    color: color-mix(in oklab, var(--accent) 35%, var(--ink));
  }
  .t-fn,
  .t-key,
  .t-attr {
    color: var(--ink-2);
    font-weight: 500;
  }
  .t-meta {
    color: var(--ink-3);
  }
  .t-ins {
    color: color-mix(in oklab, var(--gold-lo) 60%, var(--ink));
  }
  .t-del {
    color: var(--danger);
  }
  .next-page {
    margin-top: var(--space-5);
  }
  .table-sheet {
    padding: var(--space-4);
    overflow-x: auto;
  }
</style>
