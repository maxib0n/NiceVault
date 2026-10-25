<!-- Folder path. Crumbs accept dropped items (move into that folder). -->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { ChevronRight } from '@lucide/svelte';
  import { DRAG_MIME } from '../lib/dnd';
  import type { FileBrowser } from '../lib/stores/browser.svelte';

  let { browser }: { browser: FileBrowser } = $props();
  let over = $state<number | null>(null);

  // presentation only: when the path is wider than its slot, the oldest crumbs
  // slide out at the left under a fade (data-overflow drives the mask)
  let nav: HTMLElement | undefined = $state();
  let list: HTMLOListElement | undefined = $state();
  let overflow = $state(false);
  $effect(() => {
    if (!nav || !list) return;
    const n = nav;
    const l = list;
    const measure = () => (overflow = l.offsetWidth > n.clientWidth + 1);
    const ro = new ResizeObserver(measure);
    ro.observe(n);
    ro.observe(l);
    measure();
    return () => ro.disconnect();
  });

  function drop(e: DragEvent, i: number) {
    over = null;
    const raw = e.dataTransfer?.getData(DRAG_MIME);
    if (!raw) return;
    e.preventDefault();
    void browser.moveInto(JSON.parse(raw), browser.trail[i].id);
  }
</script>

<nav class="breadcrumb" aria-label={m.breadcrumb()} bind:this={nav} data-overflow={overflow}>
  <ol bind:this={list}>
    {#each browser.trail as crumb, i (crumb.id)}
      <li>
        {#if i > 0}<ChevronRight size={14} aria-hidden="true" class="sep" />{/if}
        {#if i === browser.trail.length - 1}
          <span class="crumb current" aria-current="page">{crumb.name}</span>
        {:else}
          <button
            type="button"
            class="crumb"
            class:drop-target={over === i}
            onclick={() => browser.gotoCrumb(i)}
            ondragover={(e) => {
              if (e.dataTransfer?.types.includes(DRAG_MIME)) {
                e.preventDefault();
                over = i;
              }
            }}
            ondragleave={() => (over = null)}
            ondrop={(e) => drop(e, i)}
          >
            {crumb.name}
          </button>
        {/if}
      </li>
    {/each}
  </ol>
</nav>

<style>
  /* the current folder is never cut; when space runs out the oldest crumbs
     slide out at the left, under a fade */
  .breadcrumb {
    min-width: 0;
    overflow: hidden;
    display: flex;
    justify-content: flex-start;
  }
  .breadcrumb[data-overflow='true'] {
    justify-content: flex-end;
    -webkit-mask-image: linear-gradient(90deg, transparent, #000 28px);
    mask-image: linear-gradient(90deg, transparent, #000 28px);
  }
  ol {
    flex: none;
    list-style: none;
    display: flex;
    align-items: center;
    margin: 0;
    padding: 0;
    white-space: nowrap;
  }
  li {
    display: flex;
    align-items: center;
    color: var(--ink-4);
  }
  li :global(.sep) {
    flex: none;
    margin: 0 1px;
  }
  .crumb {
    display: inline-flex;
    align-items: center;
    height: 28px;
    padding: 0 7px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    font-size: var(--text-md);
    font-weight: 500;
    color: var(--ink-3);
  }
  button.crumb:hover {
    background: color-mix(in oklab, var(--ink) 5%, transparent);
    color: var(--ink);
  }
  button.crumb:active {
    background: color-mix(in oklab, var(--ink) 9%, transparent);
  }
  /* the path clips its overflow, so the keyboard ring is drawn inside the crumb */
  .crumb:focus-visible {
    outline-offset: -2px;
  }
  .crumb.current {
    font-weight: 600;
    color: var(--ink);
  }
  .crumb.drop-target {
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1.5px var(--accent);
    color: var(--ink);
  }
</style>
