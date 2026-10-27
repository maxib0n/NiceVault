<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { flushSync, onMount } from 'svelte';
  import * as api from '../lib/api';
  import { ROOT, type FsNode, type NodeId, type VaultId } from '../lib/types';
  import { toasts } from '../lib/stores/toasts.svelte';
  import MobileRename from './MobileRename.svelte';
  import { sheetSwipe } from '../lib/sheetSwipe';
  import { ChevronLeft, Folder, FolderPlus, X } from '@lucide/svelte';

  let {
    vault,
    volumeName,
    ids,
    label,
    start,
    from,
    onmove,
    oncancel,
  }: {
    vault: VaultId;
    volumeName: string;
    ids: NodeId[];
    label: string;
    start: { id: NodeId; name: string }[];
    from: NodeId;
    onmove: (dest: NodeId) => Promise<void>;
    oncancel: () => void;
  } = $props();

  let dlg: HTMLDialogElement | undefined = $state();
  let trail = $state<{ id: NodeId; name: string }[]>([]);
  let dirs = $state<FsNode[] | null>(null);
  let naming = $state(false);
  let busy = $state(false);
  const here = $derived(trail[trail.length - 1] ?? { id: ROOT, name: volumeName });
  const moving = $derived(new Set(ids));

  onMount(() => {
    trail = start.length ? [...start] : [{ id: ROOT, name: volumeName }];
    dlg?.showModal();
  });

  $effect(() => {
    const id = here.id;
    let gone = false;
    dirs = null;
    api.fsList(vault, id).then(
      (list) => {
        if (!gone) dirs = list.filter((n) => n.kind === 'dir' && !moving.has(n.id)).sort((a, b) => a.name.localeCompare(b.name));
      },
      (e) => {
        if (!gone) {
          dirs = [];
          toasts.error(e);
        }
      },
    );
    return () => {
      gone = true;
    };
  });

  async function make(name: string): Promise<boolean> {
    try {
      const n = await api.fsMkdir(vault, here.id, name.trim());
      naming = false;
      trail = [...trail, { id: n.id, name: n.name }];
      return true;
    } catch (e) {
      toasts.error(e);
      return false;
    }
  }

  async function moveHere() {
    if (busy) return;
    busy = true;
    await onmove(here.id);
    busy = false;
  }
</script>

<dialog
  bind:this={dlg}
  class="mobile-move"
  aria-label={m.mv_title({ label })}
  data-testid="mobile-move"
  oncancel={(e) => {
    e.preventDefault();
    oncancel();
  }}
  use:sheetSwipe={{ onclose: oncancel }}
>
  <header class="head" data-sheet-handle>
    <button type="button" class="btn ghost icon" aria-label={m.action_cancel()} onclick={oncancel}><X size={18} /></button>
    <div class="titles">
      <p class="what">{m.mv_title({ label })}</p>
      <h2 class="where" data-testid="move-where">{here.name}</h2>
    </div>
  </header>

  <ul class="dirs" aria-label={here.name}>
    {#if trail.length > 1}
      <li>
        <button type="button" class="row up" onclick={() => (trail = trail.slice(0, -1))}>
          <ChevronLeft size={18} aria-hidden="true" /><span>{m.mv_up({ name: trail[trail.length - 2].name })}</span>
        </button>
      </li>
    {/if}
    {#each dirs ?? [] as d (d.id)}
      <li>
        <button type="button" class="row" onclick={() => (trail = [...trail, { id: d.id, name: d.name }])}>
          <Folder size={18} aria-hidden="true" /><span>{d.name}</span>
        </button>
      </li>
    {/each}
    {#if dirs && dirs.length === 0}
      <li class="empty">{m.mv_empty()}</li>
    {/if}
  </ul>

  <footer class="foot">
    <button type="button" class="btn" onclick={() => flushSync(() => (naming = true))}><FolderPlus size={16} aria-hidden="true" />{m.fb_new_folder()}</button>
    <button type="button" class="btn primary" disabled={busy || here.id === from} onclick={moveHere}>{m.mv_here()}</button>
  </footer>
</dialog>

{#if naming}
  <MobileRename name="" title={m.fb_new_folder()} oncommit={make} oncancel={() => (naming = false)} />
{/if}

<style>
  .mobile-move {
    position: fixed;
    inset: 0;
    width: 100%;
    height: 100%;
    max-width: none;
    max-height: none;
    margin: 0;
    padding: env(safe-area-inset-top, 0px) env(safe-area-inset-right, 0px) env(safe-area-inset-bottom, 0px) env(safe-area-inset-left, 0px);
    border: 0;
    display: flex;
    flex-direction: column;
    background: var(--paper);
    color: var(--ink);
  }
  .mobile-move:not([open]) {
    display: none;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 10px 12px;
    border-bottom: 1px solid var(--paper-rule);
  }
  .titles {
    min-width: 0;
  }
  .what {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--ink-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .where {
    margin: 0;
    font-size: var(--text-lg, 18px);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dirs {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 0;
    padding: 6px 0;
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 48px;
    padding: 0 16px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .row span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row:active {
    background: var(--paper-sunk);
  }
  .up {
    color: var(--ink-3);
  }
  .empty {
    padding: 16px;
    color: var(--ink-3);
  }
  .foot {
    display: flex;
    gap: var(--space-2);
    justify-content: space-between;
    padding: 10px 12px 12px;
    border-top: 1px solid var(--paper-rule);
  }
  .foot .btn {
    flex: 1;
    gap: 6px;
  }
</style>
