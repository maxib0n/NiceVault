<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { CheckCheck, Download, Ellipsis, Tag as TagIcon, Trash, X } from '@lucide/svelte';
  import { formatNumber } from '../lib/format';
  import type { FileBrowser } from '../lib/stores/browser.svelte';

  let { browser }: { browser: FileBrowser } = $props();
  const n = $derived(browser.selectedIds.length);
  const all = $derived(n > 0 && n === browser.entries.length);
  let more: HTMLButtonElement | undefined = $state();
</script>

<div class="m-select nv-enter" role="toolbar" aria-label={m.sel_count({ count: n, n: formatNumber(n) })} data-testid="mobile-select">
  <button type="button" class="btn ghost icon" aria-label={m.sel_done()} data-testid="mobile-select-done" onclick={() => browser.endSelecting()}>
    <X size={20} />
  </button>
  <p class="count" aria-live="polite">{m.sel_count({ count: n, n: formatNumber(n) })}</p>
  <button type="button" class="btn ghost icon" aria-label={m.fb_select_all()} aria-pressed={all} disabled={all} onclick={() => browser.selectAll()}>
    <CheckCheck size={19} />
  </button>
  <button
    type="button"
    class="btn ghost icon"
    aria-label={m.tag_button()}
    disabled={n === 0}
    onclick={(e) => {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      browser.openTagger({ x: r.left, y: r.top });
    }}
  >
    <TagIcon size={19} />
  </button>
  <button type="button" class="btn ghost icon" aria-label={m.tb_export()} disabled={n === 0} onclick={() => browser.exportNodes()}>
    <Download size={19} />
  </button>
  <button type="button" class="btn ghost icon danger" aria-label={m.action_delete()} disabled={n === 0} onclick={() => browser.requestDelete()}>
    <Trash size={19} />
  </button>
  <button
    bind:this={more}
    type="button"
    class="btn ghost icon"
    aria-label={m.fb_menu_item()}
    aria-haspopup="menu"
    disabled={n === 0}
    onclick={() => {
      const r = more?.getBoundingClientRect();
      const id = browser.sel.focus ?? browser.selectedIds[0];
      if (r && id !== undefined) browser.openMenu(r.left, r.top, id);
    }}
  >
    <Ellipsis size={20} />
  </button>
</div>

<style>
  .m-select {
    position: fixed;
    inset: auto 0 0;
    z-index: calc(var(--z-menu) - 1);
    display: flex;
    align-items: center;
    gap: 2px;
    height: calc(var(--flap-h, 62px) + var(--safe-bottom, 0px));
    padding: 0 max(6px, var(--safe-right, 0px)) var(--safe-bottom, 0px) max(6px, var(--safe-left, 0px));
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--slip-lip, var(--key-lip)),
      0 -1px 0 rgb(var(--cast) / 0.2),
      0 -12px 28px -14px rgb(var(--cast) / 0.55);
    --enter-from: translate3d(0, 24px, 0);
    --enter-to: translate3d(0, 0, 0);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
  }
  .count {
    flex: 1;
    min-width: 0;
    margin: 0 4px;
    font-weight: 600;
    font-size: var(--text-base);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-variant-numeric: tabular-nums;
  }
  .btn.icon {
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
    justify-content: center;
  }
  .danger {
    color: var(--danger);
  }
  @media (prefers-reduced-motion: reduce) {
    .m-select {
      animation: none;
    }
  }
</style>
