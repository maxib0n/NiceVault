<!-- Volume aperto / Password / Dettagli: the open passport. The cover (sidebar) on the left, the page on the right, the job tray. -->
<script lang="ts">
  import FileBrowserView from '../components/FileBrowserView.svelte';
  import JobTray from '../components/JobTray.svelte';
  import PhotoImportSheet from '../components/PhotoImportSheet.svelte';
  import Sidebar from '../components/Sidebar.svelte';
  import { app } from '../lib/stores/app.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import Details from './Details.svelte';
  import Passwords from './Passwords.svelte';
  import { m } from '../paraglide/messages.js';
  import { interiorRise, interiorSink } from '../components/ceremony';
  import Stitch from '../ornament/Stitch.svelte';
  import { sidebar } from '../lib/stores/sidebar.svelte';

  // folding / unfolding the sidebar: the page, the gutter and the seam slide from where they
  // were (a transform, transform-only motion), the cover slides with them (Sidebar.svelte)
  let slide = $state<'fold' | 'open' | undefined>();
  let was = sidebar.rail;
  let slideTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const now = sidebar.rail;
    if (now === was) return;
    was = now;
    slide = now ? 'fold' : 'open';
    clearTimeout(slideTimer);
    slideTimer = setTimeout(() => (slide = undefined), 400);
  });

  /** Impostazioni is open over it: kept as it is (not re-mounted on Indietro), not rendered, inert */
  let { covered = false }: { covered?: boolean } = $props();
</script>

<div class="vault-screen" data-rail={sidebar.rail ? '' : undefined} data-slide={slide} class:covered inert={covered} aria-hidden={covered ? 'true' : undefined} in:interiorRise out:interiorSink>
  <span class="gutter" aria-hidden="true"></span>
  <span class="seam" aria-hidden="true"><Stitch shape="seam" inset={0} len={4} gap={3} width={1.4} /></span>
  <Sidebar />
  {#if vaults.browser}
    {#if app.page === 'details'}
      <Details />
    {:else if app.page === 'passwords'}
      {#key vaults.activeId}
        <Passwords />
      {/key}
    {:else}
      {#key vaults.browser}
        <FileBrowserView browser={vaults.browser} />
      {/key}
    {/if}
  {:else}
    <main class="no-vault"><p class="hint">{m.vault_none_open()}</p></main>
  {/if}
  <JobTray />
  <PhotoImportSheet />
</div>

<style>
  .vault-screen {
    position: relative;
    z-index: 1;
    height: 100%;
    display: flex;
    background-color: var(--paper);
  }
  /* the page curls into the binding beside the cover */
  .gutter {
    position: absolute;
    top: 0;
    bottom: 0;
    left: var(--sidebar-w);
    width: 26px;
    z-index: 1;
    pointer-events: none;
    background: linear-gradient(90deg, rgb(var(--cast) / 0.13), rgb(var(--cast) / 0.04) 40%, transparent);
  }
  /* where the lining meets the binding: a saddle-stitched seam (only where the theme stitches) */
  /* it runs the cover's whole height: the seam goes on past the window's edges, as on a real
     cover the stitching is not cut short by what is laid on the page */
  .seam {
    position: absolute;
    top: 0;
    bottom: 0;
    /* a strip 8 px wide centred on the seam's line, 8 px inside the cover's edge */
    left: calc(var(--sidebar-w) - 12px);
    width: 8px;
    z-index: 3;
    pointer-events: none;
  }
  /* the unlock ceremony (ceremony.ts): the cover and the page's head wait for their glyphs */
  .vault-screen:global([data-compose='wait'] :is(.sidebar, .folder-head, .details, .passwords) > *) {
    opacity: 0;
  }
  /* under Impostazioni: its layout and state are kept, nothing of it is rendered */
  .vault-screen.covered {
    content-visibility: hidden;
    visibility: hidden;
  }
  @media (min-width: 641px) {
    .vault-screen[data-rail] {
      --sidebar-w: var(--rail-w);
    }
  }
  .vault-screen[data-slide] > :is(.gutter, .seam, :global(.sheet)) {
    translate: var(--nv-dx) 0;
  }
  .vault-screen[data-slide='fold'] > :is(.gutter, .seam, :global(.sheet)) {
    animation: nv-rail-fold var(--dur-rail, 280ms) var(--ease-out);
  }
  .vault-screen[data-slide='open'] > :is(.gutter, .seam, :global(.sheet)) {
    animation: nv-rail-open var(--dur-rail, 280ms) var(--ease-out);
  }
  @keyframes nv-rail-fold {
    from {
      --nv-dx: calc(var(--sidebar-full) - var(--rail-w));
    }
    to {
      --nv-dx: 0px;
    }
  }
  @keyframes nv-rail-open {
    from {
      --nv-dx: calc(var(--rail-w) - var(--sidebar-full));
    }
    to {
      --nv-dx: 0px;
    }
  }
  .no-vault {
    flex: 1;
    display: grid;
    place-items: center;
  }
</style>
