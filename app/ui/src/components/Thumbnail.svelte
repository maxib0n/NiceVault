<script lang="ts">
  import { previewUrl } from '../lib/api';
  import { extension, fileType } from '../lib/filetype';
  import type { FsNode, VaultId } from '../lib/types';
  import FileIcon from './FileIcon.svelte';
  import { decodeMedia } from '../lib/reveal';
  import { videoThumb, videoThumbSettled } from '../lib/videoThumb';

  const RETRY_MS = 1500;

  let { vault, node, size = 40, px }: { vault: VaultId; node: FsNode; size?: number; px?: number } = $props();

  const type = $derived(fileType(node));
  let shown = 0;
  const want = $derived.by(() => (px ? (shown = Math.max(shown, px)) : 0));
  let frame = $state<string | null>(null);
  let box: HTMLElement | undefined = $state();
  $effect(() => {
    if (type !== 'video' || !box) return;
    const el = box;
    void node.mtime;
    frame = null;
    let ac: AbortController | null = null;
    let on = false;
    let again: ReturnType<typeof setTimeout> | undefined;
    const ask = () => {
      const a = (ac = new AbortController());
      void videoThumb(vault, node, px ?? size * 2, a.signal).then((u) => {
        if (a.signal.aborted) return;
        frame = u;
        if (u || videoThumbSettled(vault, node)) return io.disconnect();
        ac = null;
        again = setTimeout(() => on && !ac && ask(), RETRY_MS);
      });
    };
    const io = new IntersectionObserver((es) => {
      on = es.some((e) => e.isIntersecting);
      if (on && !ac) ask();
      else if (!on && ac && !frame) {
        ac.abort();
        ac = null;
      }
    });
    io.observe(el);
    return () => {
      io.disconnect();
      clearTimeout(again);
      ac?.abort();
    };
  });
  const src = $derived(type === 'image' ? previewUrl(vault, node.id, want || undefined) : type === 'video' ? (frame ?? '') : '');
  let failed = $state(false);
  const ext = $derived(node.kind === 'file' ? extension(node.name) : '');
  const wide = $derived(type === 'video' || type === 'presentation');
</script>

<span class="thumb" bind:this={box} data-type={type} class:has-image={!!src && !failed} class:small={size < 32}>
  {#if src && !failed}
    <span class="print">
      <img {src} alt="" data-testid="thumbnail-image" loading="lazy" decoding="async" draggable="false" onerror={() => (failed = true)} use:decodeMedia={{ speed: 1.15, key: `thumb:${vault}:${node.id}` }} />
      {#if type === 'video'}<span class="play-mark" data-testid="video-thumb" aria-hidden="true"></span>{/if}
    </span>
  {:else}
    <span class="paper-file" class:wide>
      <FileIcon {type} size={size >= 32 ? 20 : 12} />
      {#if size >= 32 && ext}<span class="ext">{ext}</span>{/if}
    </span>
  {/if}
</span>

<style>
  .thumb {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .print {
    --ascii-surface: var(--print-develop);
    position: absolute;
    inset: 0;
    display: block;
    padding: 4px;
    background:
      linear-gradient(var(--print-develop), var(--print-develop)) content-box,
      var(--print-border);
    box-shadow: var(--lift-print), var(--print-glow);
    transition: box-shadow var(--dur-micro) var(--ease-out);
  }
  .small .print {
    padding: 1.5px;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .play-mark {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 28%;
    max-width: 34px;
    min-width: 12px;
    aspect-ratio: 1;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    background: rgb(0 0 0 / 0.45);
    box-shadow: 0 0 0 1px rgb(255 255 255 / 0.55);
    pointer-events: none;
  }
  .play-mark::after {
    content: '';
    position: absolute;
    inset: 28% 24% 28% 36%;
    background: #fff;
    clip-path: polygon(0 0, 100% 50%, 0 100%);
  }

  .paper-file {
    position: relative;
    height: 100%;
    aspect-ratio: 0.74;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    background:
      linear-gradient(225deg, transparent 0 9px, var(--file-paper) 9px) top right / 100% 100% no-repeat;
    box-shadow:
      0 0 0 0.5px rgb(var(--cast) / 0.16),
      0 1px 1px rgb(var(--cast) / 0.1),
      0 4px 8px -4px rgb(var(--cast) / 0.28),
      var(--env-glow);
    color: var(--file-ink);
  }
  .paper-file.wide {
    height: auto;
    width: 82%;
    aspect-ratio: 1.45;
  }
  .paper-file::after {
    content: '';
    position: absolute;
    top: 0;
    right: 0;
    width: 9px;
    height: 9px;
    background: linear-gradient(225deg, transparent 50%, var(--file-fold) 50%);
    box-shadow: -1px 1px 1px rgb(var(--cast) / 0.14);
  }
  .small .paper-file {
    gap: 0;
    background: var(--file-paper);
  }
  .small .paper-file::after {
    width: 5px;
    height: 5px;
  }
  .small .paper-file.wide {
    width: 100%;
  }
  .ext {
    max-width: 86%;
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--file-ink);
    overflow-wrap: anywhere;
    text-align: center;
    line-height: 1.1;
  }
</style>
