<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { File, Folder, Lock } from '@lucide/svelte';
  import type { FsNode } from '../lib/types';
  import { extension } from '../lib/filetype';
  import { formatBytes, formatDateTime, formatNumber } from '../lib/format';
  import { skeleton } from '../lib/reveal';
  import type { ByteSource } from './source';
  import { listZip, looksLikeZip } from './zip';
  import { listTar, looksLikeTar } from './tar';
  import { magicLabel } from './kinds';
  import type { InfoReason } from './InfoViewer.svelte';

  let { src, node, onfail }: { src: ByteSource; node: FsNode; onfail: (r: InfoReason) => void } = $props();

  interface Row {
    name: string;
    depth: number;
    dir: boolean;
    size: number;
    packed: number | null;
    mtime: number | null;
    locked: boolean;
  }
  let rows = $state.raw<Row[] | null>(null);
  let truncated = $state(false);
  let total = $state(0);
  let opaque = $state<string | null>(null);
  let filter = $state('');

  $effect(() => {
    const s = src;
    const ac = new AbortController();
    rows = null;
    opaque = null;
    (async () => {
      const head = await s.read(0, 512, ac.signal);
      const ext = extension(node.name);
      let list: Row[];
      if (looksLikeZip(head)) {
        const z = await listZip(s, 5000, ac.signal);
        truncated = z.truncated;
        total = z.total;
        list = z.entries.map((e) => ({ name: e.name, depth: 0, dir: e.dir, size: e.size, packed: e.compressed, mtime: e.mtime, locked: e.encrypted }));
      } else if (looksLikeTar(head)) {
        const t = await listTar(s, 5000, ac.signal);
        truncated = t.truncated;
        list = t.entries.map((e) => ({ name: e.name, depth: 0, dir: e.dir, size: e.size, packed: null, mtime: e.mtime, locked: false }));
        total = list.length;
      } else {
        opaque = magicLabel(head) ?? ext.toUpperCase();
        return;
      }
      if (ac.signal.aborted) return;
      const seen = new Set<string>();
      const out: Row[] = [];
      for (const r of list.sort((a, b) => a.name.localeCompare(b.name))) {
        const parts = r.name.replace(/\/+$/, '').split('/');
        for (let k = 1; k < parts.length; k++) {
          const dir = parts.slice(0, k).join('/');
          if (!seen.has(dir)) {
            seen.add(dir);
            out.push({ name: parts.slice(0, k).join('/') + '/', depth: k - 1, dir: true, size: 0, packed: null, mtime: null, locked: false });
          }
        }
        const key = parts.join('/');
        if (r.dir && seen.has(key)) continue;
        seen.add(key);
        out.push({ ...r, depth: parts.length - 1 });
      }
      rows = out;
    })().catch(() => {
      if (!ac.signal.aborted) onfail('error');
    });
    return () => ac.abort();
  });

  const files = $derived(rows ? rows.filter((r) => !r.dir) : []);
  const unpacked = $derived(files.reduce((s, r) => s + r.size, 0));
  const shown = $derived(rows && filter.trim() ? rows.filter((r) => r.name.toLowerCase().includes(filter.trim().toLowerCase())) : rows);
  const base = (n: string) => n.replace(/\/+$/, '').split('/').pop() ?? n;
</script>

<div class="vw archive-vw" data-testid="viewer-archive" data-entries={files.length}>
  <div class="vw-scroll">
    {#if opaque}
      <article class="vw-sheet">
        <p class="vw-eyebrow">{m.vw_archive_title()}</p>
        <h2 class="vw-state-title">{node.name}</h2>
        <p class="hint">{m.vw_archive_opaque({ what: opaque })}</p>
      </article>
    {:else if rows === null}
      <div class="vw-sheet vw-loading nv-enter" data-state="loading">
        <span class="visually-hidden">{m.fv_reading()}</span>
        <span class="vw-skeleton" aria-hidden="true" use:skeleton={{ lines: 10, seed: 11, widths: ['40%', '72%', '66%', '80%', '58%', '74%', '62%', '70%', '48%', '64%'] }}></span>
      </div>
    {:else}
      <article class="vw-sheet wide">
        <header class="summary">
          <div>
            <p class="vw-eyebrow">{m.vw_archive_title()}</p>
            <p class="counts" data-testid="viewer-archive-summary">
              {m.vw_archive_counts({ files: formatNumber(files.length), dirs: formatNumber(rows.length - files.length) })} ·
              <span class="vw-mono">{formatBytes(unpacked)}</span>
              {#if node.size && unpacked}<span class="sub"> · {m.vw_archive_ratio({ pct: String(Math.max(0, Math.round((1 - node.size / unpacked) * 100))) })}</span>{/if}
            </p>
          </div>
          <input class="input filter" type="search" placeholder={m.vw_archive_filter()} aria-label={m.vw_archive_filter()} bind:value={filter} onkeydown={(e) => e.stopPropagation()} />
        </header>
        <table class="vw-table">
          <thead>
            <tr><th>{m.vw_f_name()}</th><th class="num">{m.vw_f_size()}</th><th class="num">{m.vw_f_modified()}</th></tr>
          </thead>
          <tbody>
            {#each shown ?? [] as r (r.name)}
              <tr class:dir={r.dir}>
                <td>
                  <span class="entry" style:padding-left="{filter ? 0 : r.depth * 18}px">
                    {#if r.dir}<Folder size={14} aria-hidden="true" />{:else}<File size={14} aria-hidden="true" />{/if}
                    <span class="nm" title={r.name}>{filter ? r.name : base(r.name)}</span>
                    {#if r.locked}<Lock size={12} aria-label={m.vw_archive_encrypted()} />{/if}
                  </span>
                </td>
                <td class="num">{r.dir ? '' : formatBytes(r.size)}</td>
                <td class="num">{r.mtime ? formatDateTime(r.mtime) : ''}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if truncated}<p class="vw-note">{m.vw_archive_truncated({ n: formatNumber(files.length), total: formatNumber(total) })}</p>{/if}
      </article>
    {/if}
  </div>
</div>

<style>
  .summary {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-4);
  }
  .counts {
    margin: 0;
    font-size: var(--text-sm);
  }
  .sub {
    color: var(--ink-3);
  }
  .filter {
    width: 220px;
    flex: none;
  }
  .entry {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    color: var(--ink);
  }
  .entry :global(svg) {
    flex: none;
    color: var(--ink-3);
  }
  tr.dir .entry :global(svg) {
    color: var(--accent);
  }
  .nm {
    overflow-wrap: anywhere;
  }
</style>
