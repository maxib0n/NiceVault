<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import Thumbnail from './Thumbnail.svelte';

  let { browser, shown, onhold }: { browser: FileBrowser; shown: boolean; onhold?: (on: boolean) => void } = $props();

  let touching = false;
  let hovering = false;
  let scrollEnd: ReturnType<typeof setTimeout> | null = null;
  let held = false;
  function sync() {
    const on = touching || hovering || scrollEnd !== null;
    if (on !== held) {
      held = on;
      onhold?.(on);
    }
  }
  function scrolled() {
    if (scrollEnd) clearTimeout(scrollEnd);
    scrollEnd = setTimeout(() => {
      scrollEnd = null;
      sync();
    }, 500);
    sync();
  }
  $effect(() => () => {
    if (scrollEnd) clearTimeout(scrollEnd);
    if (held) onhold?.(false);
  });

  const list = $derived(browser.previewable);
  let strip: HTMLElement | undefined = $state();

  let placed = false;
  $effect(() => {
    const id = browser.quickLook;
    void list.length;
    const s = strip;
    if (!s || id === null) return;
    const raf = requestAnimationFrame(() => {
      const el = s.querySelector<HTMLElement>(`[data-node="${id}"]`);
      if (!el) return;
      const left = el.offsetLeft - s.offsetLeft - (s.clientWidth - el.offsetWidth) / 2;
      s.scrollTo({ left: Math.max(0, left), behavior: placed ? 'smooth' : 'instant' });
      placed = true;
    });
    return () => cancelAnimationFrame(raf);
  });
</script>

{#if list.length > 1}
  <nav class="ql-dock" class:shown aria-label={m.ql_dock()} data-testid="ql-dock" data-shown={shown}
    onpointerenter={(e) => { if (e.pointerType === 'mouse') { hovering = true; sync(); } }}
    onpointerleave={(e) => { if (e.pointerType === 'mouse') { hovering = false; sync(); } }}
    ontouchstart={() => { touching = true; sync(); }}
    ontouchend={(e) => { touching = e.touches.length > 0; sync(); }}
    ontouchcancel={() => { touching = false; sync(); }}
  >
    <div class="strip" bind:this={strip} onscroll={scrolled}>
      {#each list as n (n.id)}
        <button
          type="button"
          class="frame"
          data-node={n.id}
          aria-label={n.name}
          aria-current={browser.quickLook === n.id ? 'true' : undefined}
          tabindex={shown ? 0 : -1}
          onclick={() => {
            browser.quickLook = n.id;
            browser.sel = { selected: new Set([n.id]), anchor: n.id, focus: n.id };
          }}
        >
          <Thumbnail vault={browser.vault.id} node={n} size={52} px={104} />
        </button>
      {/each}
    </div>
  </nav>
{/if}

<style>
  .ql-dock {
    position: absolute;
    left: 50%;
    bottom: calc(14px + env(safe-area-inset-bottom, 0px));
    max-width: min(920px, calc(100% - 32px));
    padding: 2px 6px 4px;
    border-radius: 14px;
    background: color-mix(in oklab, var(--paper-hi) 82%, transparent);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--paper-rule),
      var(--shadow-pop);
    backdrop-filter: blur(10px);
    will-change: opacity;
    --nv-o: 0;
    --nv-t: translate3d(-50%, 18px, 0);
    opacity: var(--nv-o);
    transform: var(--nv-t);
    pointer-events: none;
    transition:
      --nv-o var(--dur-ui) var(--ease-out),
      --nv-t var(--dur-ui) var(--ease-out);
    z-index: 2;
  }
  .ql-dock.shown {
    --nv-o: 1;
    --nv-t: translate3d(-50%, 0, 0);
    pointer-events: auto;
  }
  .strip {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    scrollbar-width: none;
    padding: 10px 6px 6px;
  }
  .frame {
    flex: none;
    display: grid;
    place-items: center;
    width: 60px;
    height: 60px;
    padding: 0;
    border: 0;
    border-radius: 8px;
    background: transparent;
    --nv-o: 0.62;
    --nv-t: translateY(0);
    opacity: var(--nv-o);
    transform: var(--nv-t);
    transition:
      --nv-o var(--dur-ui) var(--ease-out),
      --nv-t var(--dur-ui) var(--ease-out);
    cursor: pointer;
  }
  .frame > :global(.thumb) {
    width: 52px;
    height: 52px;
    overflow: visible;
  }
  .frame :global(.paper-file) {
    flex: none;
    width: 36px;
    height: 48px;
    aspect-ratio: auto;
    gap: 5px;
  }
  .frame :global(.paper-file.wide) {
    width: 50px;
    height: 34px;
    gap: 2px;
  }
  .frame :global(.paper-file .ext) {
    font-size: 8px;
    letter-spacing: 0.06em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: clip;
  }
  .frame:hover {
    --nv-o: 0.9;
  }
  .frame[aria-current='true'] {
    --nv-o: 1;
    --nv-t: translateY(-4px);
    box-shadow: 0 0 0 2px var(--accent);
  }
  .frame:focus-visible {
    outline: 2px solid var(--focus, var(--accent));
    outline-offset: 2px;
  }
  @media (prefers-reduced-motion: reduce) {
    .ql-dock,
    .frame {
      transition: none;
    }
  }
</style>
