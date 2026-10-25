<!--
  A folder as an open security envelope. Its flap stands open behind it, the inside printed
  with the security tint; it was slit open, so no seal is left on it. What it holds stands up out of it from INSIDE, between the far wall and
  the near one (prints of its first photos, sheets for its documents, smaller envelopes for
  its folders); the near wall is a pocket whose V-cut mouth hides their feet. It reads the
  folder's listing to know what is inside. An empty folder is an envelope never opened: seen
  from the back, its flap folded down and closed with an intact wax seal in the volume's ink.
  Every part is a percentage of the picture box: the same envelope at S, M, L and XL and in the
  list's small row, contents included. No canvas: boxes, clips and small SVGs.
-->
<script lang="ts">
  import { previewUrl } from '../lib/api';
  import * as api from '../lib/api';
  import { fileType } from '../lib/filetype';
  import type { FsNode, VaultId } from '../lib/types';
  import Wax from '../ornament/Wax.svelte';

  let {
    vault,
    node,
    layout = 'grid',
    refresh,
    count = $bindable<number | null>(null),
  }: {
    vault: VaultId;
    node: FsNode;
    layout?: 'grid' | 'list';
    /** anything that changes when the listing reloads (re-reads the contents) */
    refresh?: unknown;
    count?: number | null;
  } = $props();

  type Peek = { kind: 'print'; src: string } | { kind: 'sheet' } | { kind: 'envelope' };
  let peeks = $state<Peek[]>([]);
  let failed = $state(new Set<string>());

  $effect(() => {
    void refresh;
    const v = vault;
    const id = node.id;
    let live = true;
    api
      .fsList(v, id)
      .then((kids) => {
        if (!live) return;
        count = kids.length;
        const imgs = kids.filter((k) => k.kind === 'file' && fileType(k) === 'image');
        const files = kids.filter((k) => k.kind === 'file' && fileType(k) !== 'image');
        const dirs = kids.filter((k) => k.kind === 'dir');
        const out: Peek[] = [];
        for (const k of imgs) {
          const src = previewUrl(v, k.id, 384); // a print in the envelope: never the full photo
          if (src && out.length < 3) out.push({ kind: 'print', src });
        }
        for (let i = 0; i < files.length && out.length < 3; i++) out.push({ kind: 'sheet' });
        for (let i = 0; i < dirs.length && out.length < 3; i++) out.push({ kind: 'envelope' });
        peeks = out;
      })
      .catch(() => {
        // a locked or vanished volume: the envelope just stands empty
        if (live) peeks = [];
      });
    return () => {
      live = false;
    };
  });

  // standing in the envelope, fanned: positions for 1, 2 or 3 pieces (x: % of the box, r: deg,
  // y: how much lower it stands, % of the box). The middle one stands lowest: the flap's tip
  // (the printed inside of the flap behind it) always shows.
  const FAN: Record<number, { x: number; r: number; y: number }[]> = {
    1: [{ x: 0, r: -2, y: 0 }],
    2: [
      { x: -14, r: -6, y: 1 },
      { x: 14, r: 5, y: 0 },
    ],
    3: [
      { x: -23, r: -8, y: 1 },
      { x: 0, r: -1, y: 5 },
      { x: 22, r: 6.5, y: 2 },
    ],
  };
  const fan = $derived(FAN[Math.min(3, peeks.length)] ?? []);
  /** known to hold nothing: the closed envelope, sealed */
  const closed = $derived(count === 0);
</script>

{#if closed}
  <!-- nothing in it yet: closed, its flap folded down over the back, the wax seal intact -->
  <span class="envelope closed" data-envelope data-layout={layout} data-empty="true" data-closed aria-hidden="true">
    <span class="back"></span>
    <svg class="back-folds" viewBox="0 0 100 100" preserveAspectRatio="none">
      <path class="crease" d="M0.6 99.4 L38 52 M99.4 99.4 L62 52" />
    </svg>
    <span class="flap-down"></span>
    <svg class="flap-down-edge" viewBox="0 0 100 100" preserveAspectRatio="none"><path d="M0.5 0.5 L50 99 L99.5 0.5" /></svg>
    <span class="wax" data-seal><Wax size={layout === 'grid' ? 26 : 12} seed={node.id} /></span>
  </span>
{:else}
<span class="envelope" data-envelope data-layout={layout} data-empty={peeks.length === 0} aria-hidden="true">
  <!-- the flap, open: its inside printed with the security tint, its edges cut clean -->
  <span class="flap">
    <span class="flap-in"></span>
    <svg class="flap-edge" viewBox="0 0 100 100" preserveAspectRatio="none"><path d="M0.5 99.5 L50 1 L99.5 99.5" /></svg>
  </span>
  <!-- the mouth: the inside of the far wall, in its own shadow -->
  <span class="mouth"></span>
  <span class="contents">
      {#each peeks as p, i (i)}
        {@const f = fan[i]}
        <!-- the kind is a data attribute, not a class: a class "envelope" here would also
             pick up the root .envelope rule and stretch the piece to the tile -->
        <span class="piece" data-peek={p.kind} style:--x="{f.x}%" style:--r="{f.r}deg" style:--y="{f.y}%">
          {#if p.kind === 'print' && !failed.has(p.src)}
            <img
              src={p.src}
              alt=""
              loading="lazy"
              decoding="async"
              draggable="false"
              onerror={() => (failed = new Set([...failed, p.src]))}
            />
          {:else if p.kind === 'sheet' || p.kind === 'print'}
            <span class="lines"></span>
          {:else}
            <svg class="mini-folds" viewBox="0 0 100 66" preserveAspectRatio="none">
              <path d="M0.8 1 L50 38 L99.2 1" />
            </svg>
          {/if}
        </span>
      {/each}
  </span>
  <!-- the near wall: a pocket, its mouth cut in a V, the bottom flap's creases below it -->
  <span class="pocket">
    <svg class="folds" viewBox="0 0 100 100" preserveAspectRatio="none">
      <path class="crease" d="M0.6 99.4 L50 58 L99.4 99.4" />
      <path class="lip" d="M0 0.6 L50 37 L100 0.6" />
    </svg>
  </span>
</span>
{/if}

<style>
  /* the box: everything below is a share of it, so the envelope is the same at every size */
  .envelope {
    --left: 8%;
    --width: 84%;
    --body-top: 47%;
    --body-bottom: 3px;
    --apex: 5%;
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
  }

  /* ---- the flap, standing open behind the envelope ---- */
  .flap {
    position: absolute;
    left: var(--left);
    width: var(--width);
    top: var(--apex);
    bottom: calc(100% - var(--body-top) - 1px);
  }
  .flap-in {
    position: absolute;
    inset: 0;
    clip-path: polygon(0 100%, 50% 0, 100% 100%);
    /* the inside of the stock, printed with a fine security tint (the paper's own ink) */
    background:
      repeating-linear-gradient(60deg, color-mix(in srgb, var(--engrave-2) 34%, transparent) 0 0.6px, transparent 0.6px 3.2px),
      repeating-linear-gradient(-60deg, color-mix(in srgb, var(--engrave-2) 34%, transparent) 0 0.6px, transparent 0.6px 3.2px),
      linear-gradient(180deg, var(--env-paper-2), color-mix(in oklab, var(--env-paper-lo) 86%, #000));
  }
  .flap-edge {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
    fill: none;
    stroke: var(--env-crease);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .flap-edge path {
    vector-effect: non-scaling-stroke;
  }


  /* ---- the body: the far wall's inside, seen through the mouth ---- */
  .mouth {
    position: absolute;
    left: var(--left);
    width: var(--width);
    top: var(--body-top);
    bottom: var(--body-bottom);
    border-radius: 1.5px;
    background: linear-gradient(180deg, color-mix(in oklab, var(--env-paper-lo) 55%, #000), var(--env-paper-lo) 60%);
    box-shadow:
      0 0 0 0.5px rgb(var(--cast) / 0.2),
      0 1px 1px rgb(var(--cast) / 0.12),
      0 5px 9px -5px rgb(var(--cast) / 0.42),
      var(--env-glow);
  }

  /* ---- what it holds, standing inside, their feet in the pocket ---- */
  .contents {
    position: absolute;
    inset: 0;
    /* lifted under a drop (FileItem): a registered property, not its transform (styles/motion.css) */
    translate: 0 var(--nv-dy);
    transition: --nv-dy var(--dur-ui) var(--ease-out);
  }
  .piece {
    position: absolute;
    left: 50%;
    /* standing on the inside of the envelope's bottom: the pocket hides the lower part, and no
       piece rises past a third of the box, so the flap's printed inside stays in view */
    bottom: calc(22% - var(--y));
    /* narrower than the envelope's mouth even fanned and tilted: nothing stands outside it */
    width: 37%;
    aspect-ratio: 1.12;
    transform: translateX(calc(-50% + var(--x) * 2)) rotate(var(--r));
    transform-origin: 50% 100%;
    box-shadow:
      0 0 0 0.5px rgb(var(--cast) / 0.18),
      0 1px 2px rgb(var(--cast) / 0.2);
  }
  /* a print: the photograph with its white border */
  .piece[data-peek='print'] {
    padding: 2.5px;
    background: var(--print-border);
    box-shadow:
      0 0 0 0.5px rgb(var(--cast) / 0.16),
      0 1px 2px rgb(var(--cast) / 0.22),
      var(--print-glow);
  }
  .piece img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  /* a sheet: ruled lines of text, portrait */
  .piece[data-peek='sheet'] {
    width: 26%;
    aspect-ratio: 0.74;
    background: var(--sheet-paper);
  }
  .lines {
    position: absolute;
    inset: 16% 15% 34%;
    background: repeating-linear-gradient(180deg, color-mix(in srgb, var(--sheet-ink) 30%, transparent) 0 1px, transparent 1px 4px);
    opacity: 0.9;
  }
  /* a smaller envelope inside, its flap folded down */
  .piece[data-peek='envelope'] {
    width: 36%;
    aspect-ratio: 1.5;
    bottom: calc(29% - var(--y));
    border-radius: 1px;
    background: linear-gradient(176deg, var(--env-paper-hi), var(--env-paper) 45%, var(--env-paper-2));
    box-shadow:
      inset 0 1px 0 var(--env-lip),
      0 0 0 0.5px rgb(var(--cast) / 0.22),
      0 1px 2px rgb(var(--cast) / 0.24);
  }
  .mini-folds {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    fill: none;
    stroke: var(--env-crease);
    stroke-width: 1;
  }
  .mini-folds path {
    vector-effect: non-scaling-stroke;
  }

  /* ---- the near wall: the pocket ---- */
  .pocket {
    position: absolute;
    left: var(--left);
    width: var(--width);
    top: var(--body-top);
    bottom: var(--body-bottom);
    border-radius: 0 0 1.5px 1.5px;
    clip-path: polygon(0 0, 50% 37%, 100% 0, 100% 100%, 0 100%);
    background:
      linear-gradient(172deg, var(--env-paper-hi), var(--env-paper) 55%, var(--env-paper-lo)),
      var(--env-paper);
  }
  .folds {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    fill: none;
  }
  .folds path {
    vector-effect: non-scaling-stroke;
  }
  .crease {
    stroke: var(--env-crease);
    stroke-width: 0.9;
  }
  /* the mouth's cut edge catches the light, a hair of shade just under it */
  .lip {
    stroke: var(--env-lip);
    stroke-width: 1.4;
  }

  /* ---- empty: the closed envelope, from the back, sealed ---- */
  .closed .back {
    position: absolute;
    left: var(--left);
    width: var(--width);
    top: var(--closed-top);
    bottom: var(--body-bottom);
    border-radius: 1.5px;
    background: linear-gradient(172deg, var(--env-paper-hi), var(--env-paper) 55%, var(--env-paper-lo));
    box-shadow:
      0 0 0 0.5px rgb(var(--cast) / 0.2),
      0 1px 1px rgb(var(--cast) / 0.12),
      0 5px 9px -5px rgb(var(--cast) / 0.42),
      var(--env-glow);
  }
  .closed .back-folds,
  .closed .flap-down,
  .closed .flap-down-edge {
    position: absolute;
    left: var(--left);
    width: var(--width);
    top: var(--closed-top);
    bottom: var(--body-bottom);
  }
  .closed .back-folds {
    bottom: auto;
    height: calc(100% - var(--closed-top) - var(--body-bottom));
  }
  .closed .back-folds,
  .closed .flap-down-edge {
    fill: none;
    overflow: visible;
  }
  .closed .back-folds path,
  .closed .flap-down-edge path {
    vector-effect: non-scaling-stroke;
    stroke: var(--env-crease);
    stroke-width: 0.9;
  }
  /* the flap, folded down to just past the middle: a shade lighter, it catches the light */
  .closed .flap-down {
    bottom: auto;
    height: calc((100% - var(--closed-top) - var(--body-bottom)) * 0.62);
    clip-path: polygon(0 0, 100% 0, 50% 100%);
    background: linear-gradient(180deg, var(--env-paper-hi), var(--env-paper) 70%);
    box-shadow: inset 0 1px 0 var(--env-lip);
  }
  .closed .flap-down-edge {
    bottom: auto;
    height: calc((100% - var(--closed-top) - var(--body-bottom)) * 0.62);
  }
  .closed .flap-down-edge path {
    stroke: var(--env-lip);
  }
  /* the seal where the flap's point meets the back */
  .closed .wax {
    position: absolute;
    left: 50%;
    top: calc(var(--closed-top) + (100% - var(--closed-top) - var(--body-bottom)) * 0.62);
    width: 17%;
    aspect-ratio: 1;
    translate: -50% -58%;
  }
  .closed .wax :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
  .envelope.closed {
    --closed-top: 30%;
  }

  /* ---- the list's small row: the same envelope, its contents smaller ---- */
  .envelope[data-layout='list'] {
    --left: 4%;
    --width: 92%;
    --body-top: 42%;
    --body-bottom: 4%;
    --apex: 6%;
  }
  .envelope.closed[data-layout='list'] {
    --closed-top: 24%;
  }
  .envelope.closed[data-layout='list'] .wax {
    width: 30%;
  }
  /* in a row the pieces stand taller out of the pocket, or they would not read at 30 px */
  .envelope[data-layout='list'] .piece {
    bottom: calc(34% - var(--y));
    width: 44%;
    box-shadow: 0 0 0 0.5px rgb(var(--cast) / 0.3);
  }
  .envelope[data-layout='list'] .piece[data-peek='sheet'] {
    width: 32%;
  }
  .envelope[data-layout='list'] .piece[data-peek='envelope'] {
    width: 44%;
    bottom: calc(38% - var(--y));
    background: linear-gradient(176deg, var(--env-paper-hi), var(--env-paper-hi) 45%, var(--env-paper));
  }
  .envelope[data-layout='list'] .piece[data-peek='print'] {
    padding: 1px;
  }
  .envelope[data-layout='list'] .lines {
    background: repeating-linear-gradient(180deg, color-mix(in srgb, var(--sheet-ink) 40%, transparent) 0 0.5px, transparent 0.5px 2px);
  }
  .envelope[data-layout='list'] .flap-in {
    background: linear-gradient(180deg, var(--env-paper-lo), color-mix(in oklab, var(--env-paper-lo) 80%, #000));
  }
</style>
