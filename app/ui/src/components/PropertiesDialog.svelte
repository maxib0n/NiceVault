<!--
  Proprietà (menu "Proprietà", Alt+Invio): a record card of what the volume knows about a file,
  a folder, a selection or the volume itself. Quick facts at once (fs_props); the recursive
  totals of a folder / selection and a file's SHA-256 run as jobs with progress, cancelled when
  the card closes. What was never recorded (files added before NiceVault kept it) reads "—".
-->
<script lang="ts">
  import { Copy, Hash } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { m } from '../paraglide/messages.js';
  import * as api from '../lib/api';
  import { errorText } from '../lib/errors';
  import { fileType, typeLabel } from '../lib/filetype';
  import { fileCount, formatBytes, formatDateTime, formatNumber, itemCount } from '../lib/format';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { ROOT, type FsNode, type JobId, type NodeProps, type PropsDone, type PropsProgress, type VaultStats } from '../lib/types';
  import Dialog from './Dialog.svelte';
  import Emblem from './Emblem.svelte';
  import FolderPeek from './FolderPeek.svelte';
  import Thumbnail from './Thumbnail.svelte';
  import Rosette from '../ornament/Rosette.svelte';

  let { browser, onclosed }: { browser: FileBrowser; onclosed?: () => void } = $props();

  const ids = $derived(browser.props);
  const open = $derived(ids !== null);
  const single = $derived(ids?.length === 1 ? ids[0] : null);
  const isRoot = $derived(single === ROOT);

  let info = $state<NodeProps | null>(null);
  let stats = $state<VaultStats | null>(null);
  let failed = $state<string | null>(null);
  /** recursive totals (folder, selection, volume) */
  let tree = $state<{ job: JobId | null; files: number; dirs: number; bytes: number; done: boolean; error: string | null } | null>(null);
  /** SHA-256 job */
  let hash = $state<{ job: JobId | null; done: number; total: number; error: string | null } | null>(null);
  let seq = 0;

  const nodes = $derived((ids ?? []).map((id) => browser.byId.get(id)).filter((n): n is FsNode => !!n));
  const asNode = $derived<FsNode | null>(info ? { id: info.id, parent: 0, name: info.name, kind: info.kind, size: info.size, mtime: info.mtime } : null);

  // (re)load whenever the card opens on other nodes
  $effect(() => {
    const list = ids;
    const my = ++seq;
    info = null;
    stats = null;
    failed = null;
    pending.clear();
    tree = null;
    hash = null;
    if (!list) return;
    const vault = browser.vault.id;
    const many = list.length > 1;
    void (async () => {
      try {
        if (!many) {
          const p = await api.fsProps(vault, list[0]);
          if (my !== seq) return;
          info = p;
          if (p.kind === 'dir') startTree(vault, list, my);
          if (list[0] === ROOT) stats = await api.vaultStats(vault).catch(() => null);
        } else startTree(vault, list, my);
      } catch (e) {
        if (my === seq) failed = errorText(e);
      }
    })();
    return () => cancelJobs();
  });

  async function startTree(vault: string, list: number[], my: number) {
    tree = { job: null, files: 0, dirs: 0, bytes: 0, done: false, error: null };
    try {
      const job = await api.fsPropsTree(vault, list);
      if (my !== seq) {
        void api.jobCancel(job);
        return;
      }
      if (tree && !tree.done) tree = { ...tree, job };
      flushPending(job);
    } catch (e) {
      if (my === seq) tree = { job: null, files: 0, dirs: 0, bytes: 0, done: true, error: errorText(e) };
    }
  }

  async function computeHash() {
    if (!info || hash) return;
    const my = seq;
    hash = { job: null, done: 0, total: info.size, error: null };
    try {
      const job = await api.fsHash(browser.vault.id, info.id);
      if (my !== seq) {
        void api.jobCancel(job);
        return;
      }
      if (hash) hash = { ...hash, job };
      flushPending(job);
    } catch (e) {
      hash = { job: null, done: 0, total: 0, error: errorText(e) };
    }
  }

  function cancelJobs() {
    for (const j of [tree?.job, hash?.job]) if (j) void api.jobCancel(j);
  }

  // events can arrive before the command returns the job id: kept until it is known
  const pending = new Map<JobId, { progress?: PropsProgress; done?: PropsDone }>();
  function flushPending(job: JobId) {
    const p = pending.get(job);
    pending.delete(job);
    if (p?.progress) onProgress(p.progress);
    if (p?.done) onDone(p.done);
  }
  const mine = (job: JobId) => job === tree?.job || job === hash?.job;
  function onProgress(e: PropsProgress) {
    if (!mine(e.job)) {
      if (pending.size < 32) pending.set(e.job, { ...pending.get(e.job), progress: e });
      return;
    }
    if (tree && e.job === tree.job && !tree.done) tree = { ...tree, files: e.files ?? 0, dirs: e.dirs ?? 0, bytes: e.bytes ?? 0 };
    else if (hash && e.job === hash.job) hash = { ...hash, done: e.done_bytes ?? 0, total: e.total_bytes ?? hash.total };
  }
  function onDone(e: PropsDone) {
    if (!mine(e.job)) {
      if (pending.size < 32) pending.set(e.job, { ...pending.get(e.job), done: e });
      return;
    }
    const why = e.error && e.error.code !== 'cancelled' ? errorText(e.error) : null;
    if (tree && e.job === tree.job) tree = { ...tree, files: e.files ?? tree.files, dirs: e.dirs ?? tree.dirs, bytes: e.bytes ?? tree.bytes, done: true, error: why, job: null };
    else if (hash && e.job === hash.job) {
      if (e.sha256 && info) info = { ...info, sha256: e.sha256 };
      hash = why ? { ...hash, error: why, job: null } : null;
    }
  }

  onMount(() => {
    const offs: (() => void)[] = [];
    void api.onPropsProgress(onProgress).then((u) => offs.push(u));
    void api.onPropsDone(onDone).then((u) => offs.push(u));
    return () => {
      cancelJobs();
      offs.forEach((u) => u());
    };
  });

  function close() {
    cancelJobs();
    browser.props = null;
    onclosed?.();
  }

  async function copySha() {
    if (!info?.sha256) return;
    try {
      await navigator.clipboard.writeText(info.sha256);
      toasts.push(m.props_copied(), 'success');
    } catch {
      const el = document.getElementById('props-sha');
      if (el) window.getSelection()?.selectAllChildren(el);
    }
  }

  const DASH = '—';
  const when = (ns: number | null | undefined) => (ns ? formatDateTime(ns / 1_000_000) : DASH);
  const bytesLong = (n: number) => `${formatBytes(n)} (${m.props_bytes({ count: n, n: formatNumber(n) })})`;
  const parentOf = (path: string) => path.slice(0, Math.max(1, path.lastIndexOf('/'))) || '/';
  function duration(ms: number): string {
    const s = Math.round(ms / 1000);
    const h = Math.floor(s / 3600);
    const mm = Math.floor((s % 3600) / 60);
    const ss = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(mm).padStart(2, '0')}:${ss}` : `${mm}:${ss}`;
  }
  const typeText = $derived.by(() => {
    if (isRoot) return m.props_volume();
    if (!info) return '';
    return typeLabel(fileType(info));
  });
  const title = $derived(
    !ids ? '' : ids.length > 1 ? m.props_title_many({ n: formatNumber(ids.length) }) : isRoot ? browser.vault.name : (info?.name ?? browser.byId.get(ids[0])?.name ?? ''),
  );
  const treeText = $derived(tree ? `${fileCount(tree.files)} · ${m.props_dirs({ count: tree.dirs, n: formatNumber(tree.dirs) })}` : DASH);
  const selFiles = $derived(nodes.filter((n) => n.kind === 'file').length);
  const selDirs = $derived(nodes.length - selFiles);
  const hashPct = $derived(hash && hash.total > 0 ? Math.min(100, Math.round((hash.done / hash.total) * 100)) : 0);
</script>

<Dialog {open} {title} variant="wide" onclose={close}>
  <div class="card" data-testid="props" data-kind={isRoot ? 'volume' : ids && ids.length > 1 ? 'selection' : (info?.kind ?? '')}>
    {#if failed}
      <p class="error-text" role="alert">{m.props_failed({ why: failed })}</p>
    {:else}
      <header class="head">
        <span class="picture" aria-hidden="true">
          {#if isRoot}
            <Rosette size={64} kind="seal" frame={browser.vault.emblem_frame}><Emblem emblem={browser.vault.emblem} name={browser.vault.name} size={22} decorative /></Rosette>
          {:else if asNode && asNode.kind === 'dir'}
            <FolderPeek vault={browser.vault.id} node={asNode} layout="grid" />
          {:else if asNode}
            <Thumbnail vault={browser.vault.id} node={asNode} size={40} />
          {:else if ids && ids.length > 1}
            <span class="pile">{formatNumber(ids.length)}</span>
          {/if}
        </span>
        <span class="what">
          <span class="type">{ids && ids.length > 1 ? m.props_selection() : typeText}</span>
          {#if info && !isRoot}
            <span class="sub">{info.kind === 'dir' ? itemCount(info.children ?? 0) : formatBytes(info.size)}{#if info.mime}<span class="mime"> · {info.mime}</span>{/if}</span>
          {:else if isRoot && stats}
            <span class="sub">{formatBytes(stats.used_bytes)} · {fileCount(stats.file_count)}</span>
          {/if}
        </span>
      </header>

      <dl class="facts">
        {#if ids && ids.length > 1}
          <dt>{m.props_selection()}</dt>
          <dd>{fileCount(selFiles)} · {m.props_dirs({ count: selDirs, n: formatNumber(selDirs) })}</dd>
          <dt>{m.props_contents()}</dt>
          <dd data-testid="props-tree" data-done={tree?.done ?? false}>{tree?.done ? treeText : m.props_counting({ n: treeText })}</dd>
          <dt>{m.props_total()}</dt>
          <dd class="num" data-testid="props-total">{tree ? bytesLong(tree.bytes) : DASH}</dd>
          <dt>{m.props_location()}</dt>
          <dd class="path">/{browser.trail.slice(1).map((c) => c.name).join('/')}</dd>
        {:else if info}
          {#if isRoot}
            <dt>{m.props_volume_file()}</dt>
            <dd class="path">{browser.vault.path}</dd>
            {#if stats}
              <dt>{m.props_on_disk()}</dt>
              <dd class="num">{formatBytes(stats.file_size_bytes)}</dd>
              {#if stats.free_bytes !== undefined}
                <dt>{m.props_free()}</dt>
                <dd class="num">{formatBytes(stats.free_bytes)}</dd>
              {/if}
            {/if}
          {:else}
            <dt>{m.props_location()}</dt>
            <dd class="path" data-testid="props-location">{parentOf(info.path)}</dd>
          {/if}
          {#if info.kind === 'dir'}
            <dt class="g">{m.props_inside()}</dt>
            <dd class="g">{itemCount(info.children ?? 0)}</dd>
            <dt>{m.props_contents()}</dt>
            <dd data-testid="props-tree" data-done={tree?.done ?? false}>
              {tree?.error ?? (tree?.done ? treeText : m.props_counting({ n: treeText }))}
            </dd>
            <dt>{m.props_total()}</dt>
            <dd class="num" data-testid="props-total">{tree ? bytesLong(tree.bytes) : DASH}</dd>
          {:else}
            <dt class="g">{m.props_size()}</dt>
            <dd class="num g" data-testid="props-size">{bytesLong(info.size)}</dd>
            {#if info.width && info.height}
              <dt>{m.props_dimensions()}</dt>
              <dd class="num" data-testid="props-dimensions">{m.props_pixels({ w: formatNumber(info.width), h: formatNumber(info.height) })}</dd>
            {/if}
            {#if info.duration_ms}
              <dt>{m.props_duration()}</dt>
              <dd class="num" data-testid="props-duration">{duration(info.duration_ms)}</dd>
            {/if}
          {/if}
          <dt class="g">{m.props_modified()}</dt>
          <dd class="num g">{when(info.mtime)}</dd>
          <dt>{m.props_created()}</dt>
          <dd class="num" data-testid="props-created">{when(info.created)}</dd>
          {#if !isRoot}
            <dt>{m.props_imported()}</dt>
            <dd class="num" data-testid="props-imported">{info.imported ? when(info.imported) : info.created ? m.props_made_here() : DASH}</dd>
            <dt class="g">{m.props_origin()}</dt>
            <dd class="path g" data-testid="props-origin">{info.origin ?? DASH}</dd>
          {/if}
          {#if info.kind === 'file'}
            <dt class="g">{m.props_sha()}</dt>
            <dd class="sha g" data-testid="props-sha">
              {#if info.sha256}
                <code id="props-sha" class="hash">{info.sha256}</code>
                <button type="button" class="btn sm ghost icon" aria-label={m.action_copy()} title={m.action_copy()} onclick={copySha}><Copy size={14} /></button>
              {:else if hash && !hash.error}
                <span class="hashing" role="status">
                  <progress max="100" value={hashPct}></progress>
                  <span class="num">{m.props_sha_running({ pct: `${hashPct}%` })}</span>
                </span>
              {:else}
                <button type="button" class="btn sm" onclick={computeHash}><Hash size={14} aria-hidden="true" /> {m.props_sha_compute()}</button>
                {#if hash?.error}<span class="error-text">{hash.error}</span>{:else}<span class="hint">{m.props_sha_hint()}</span>{/if}
              {/if}
            </dd>
          {/if}
        {/if}
      </dl>
      {#if info && !isRoot && (!info.created || !info.imported) && !info.origin}
        <p class="hint footnote">{m.props_unknown_hint()}</p>
      {/if}
    {/if}
  </div>
  {#snippet actions()}
    <!-- svelte-ignore a11y_autofocus -->
    <button type="button" class="btn" autofocus onclick={close}>{m.action_close()}</button>
  {/snippet}
</Dialog>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    color: var(--ink);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--paper-rule);
  }
  /* the item itself, as it lies in the view, on a pressed bed */
  .picture {
    flex: none;
    position: relative;
    display: grid;
    place-items: center;
    width: 88px;
    height: 66px;
    padding: 5px 6px 3px;
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .picture > :global(.thumb),
  .picture > :global(.envelope),
  .picture > :global(*) {
    width: 100%;
    height: 100%;
  }
  .picture :global(.paper-file) {
    height: 100%;
  }
  .pile {
    display: grid;
    place-items: center;
    width: 46px !important;
    height: 46px !important;
    border-radius: 50%;
    font-family: var(--font-mono);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--accent-ink);
    background: var(--accent);
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .type {
    font-size: var(--text-lg);
    font-weight: 600;
  }
  .sub {
    font-size: var(--text-sm);
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }
  .mime {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  /* label / value, like the fields of a record card */
  .facts {
    display: grid;
    grid-template-columns: minmax(110px, max-content) 1fr;
    column-gap: var(--space-4);
    row-gap: 7px;
    margin: 0;
    font-size: var(--text-md);
  }
  dt {
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ink-3);
    padding-top: 2px;
  }
  dd {
    margin: 0;
    min-width: 0;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  /* the record's groups (where / how much / when / from where / its fingerprint), ruled apart */
  .facts .g {
    margin-top: 5px;
    padding-top: 12px;
    border-top: 1px solid var(--paper-rule);
  }
  .facts > .g:first-child,
  .facts > .g:first-child + dd {
    margin-top: 0;
    padding-top: 0;
    border-top: 0;
  }
  .num {
    font-family: var(--font-mono);
    font-size: 12.5px;
  }
  .path {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--ink-2);
  }
  .sha {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .hash {
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.45;
    word-break: break-all;
    user-select: all;
    flex: 1 1 280px;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    background: var(--paper-sunk);
    box-shadow: inset 0 1px 2px var(--field-shade);
  }
  .hashing {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1;
  }
  .hashing progress {
    flex: 1;
    max-width: 220px;
  }
  .footnote {
    font-size: var(--text-xs);
  }
</style>
