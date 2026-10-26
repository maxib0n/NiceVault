<!--
  Audio and video with real seeking. On Linux (WebKitGTK plays only http/blob URLs, and
  buffers an http stream to /var/tmp while it plays) media up to 32 MB plays from an in-memory
  Blob (lib/mediaBlob.ts), larger files from a one-off loopback stream with HTTP Range
  (viewers.rs media_url) played with `loop` always set: WebKit never disk-buffers looping
  media; this component stops it at the end
  itself. Elsewhere the nvault:// URL plays directly. If none works, the info panel.
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import type { FsNode, VaultInfo } from '../lib/types';
  import * as api from '../lib/api';
  import { fetchAsBlob, isSourceRejected, MEDIA_BLOB_MAX } from '../lib/mediaBlob';
  import { decodeMedia } from '../lib/reveal';
  import MediaControls from '../components/MediaControls.svelte';
  import { peekVideoThumb } from '../lib/videoThumb';
  import FileIcon from '../components/FileIcon.svelte';
  import { isMobile } from '../lib/platform';
  import { sidecarTracks } from '../lib/subtitles';
  import type { InfoReason } from './InfoViewer.svelte';
  import { idle } from '../lib/stores/idle.svelte';
  import { skipper } from '../lib/skip';
  import { appLook } from '../lib/stores/applook.svelte';

  let {
    vault,
    node,
    url,
    kind,
    onfail,
  }: { vault: VaultInfo; node: FsNode; url: string; kind: 'video' | 'audio'; onfail: (r: InfoReason) => void } = $props();

  let mediaEl: HTMLMediaElement | undefined = $state();
  let src = $state<string | null>(null);
  let via = $state<'stream' | 'scheme' | 'blob'>('scheme');
  let blobUrl: string | null = null;
  let blobAbort: AbortController | null = null;

  $effect(() => {
    const v = vault.id;
    const n = node.id;
    let ticket: string | null = null;
    let gone = false;
    src = null;
    (api.isTauri ? api.mediaUrl(v, n).catch(() => null) : Promise.resolve(null)).then((u) => {
      if (gone) {
        if (u) void api.mediaRelease(u).catch(() => undefined);
        return;
      }
      ticket = u;
      via = u ? 'stream' : 'scheme';
      src = u ?? url;
    });
    return () => {
      gone = true;
      if (ticket) void api.mediaRelease(ticket).catch(() => undefined);
      blobAbort?.abort();
      blobAbort = null;
      if (blobUrl) URL.revokeObjectURL(blobUrl);
      blobUrl = null;
    };
  });

  // sidecar subtitles (same folder, same base name, .srt / .vtt): read through the app, turned into
  // WebVTT in memory and attached as <track> blobs; nothing is written anywhere (desktop only)
  $effect(() => {
    const el = mediaEl;
    if (!el || kind !== 'video' || isMobile) return;
    let gone = false;
    const made: { url: string; node: HTMLTrackElement }[] = [];
    void sidecarTracks(vault.id, node).then((ts) => {
      if (gone) return;
      for (const t of ts) {
        const url = URL.createObjectURL(new Blob([t.vtt], { type: 'text/vtt' }));
        const tr = document.createElement('track');
        tr.kind = 'subtitles';
        tr.label = t.label;
        if (t.lang) tr.srclang = t.lang;
        tr.src = url;
        el.appendChild(tr);
        tr.track.mode = 'disabled';
        made.push({ url, node: tr });
      }
    });
    return () => {
      gone = true;
      for (const x of made) {
        x.node.remove();
        URL.revokeObjectURL(x.url);
      }
    };
  });

  function error() {
    if (via === 'blob' || blobAbort) return onfail('unsupported');
    // the scheme refused (no stream): replay from memory if it fits
    if (via === 'scheme' && isSourceRejected(mediaEl) && node.size <= MEDIA_BLOB_MAX) {
      const ac = (blobAbort = new AbortController());
      fetchAsBlob(url, node.size, ac.signal).then(
        (b) => {
          if (ac.signal.aborted) return;
          blobUrl = URL.createObjectURL(b);
          via = 'blob';
          src = blobUrl;
          blobAbort = null;
        },
        () => {
          if (!ac.signal.aborted) onfail('unsupported');
        },
      );
      return;
    }
    onfail('unsupported');
  }

  // The stream always loops (see the top): WebKit decides on disk buffering again at every
  // play() and every new HTTP response, so `loop` can never be off, not even at the end. And
  // its wrap back to 0 stalls on a stream: this stops 0.1 s before the end instead (a frame
  // watch while playing), and the next play starts over from 0.
  let atEnd = false;
  let watch = 0;
  function endWatch() {
    const el = mediaEl;
    watch = 0;
    if (!el || via !== 'stream' || el.paused) return;
    if (Number.isFinite(el.duration) && el.currentTime >= el.duration - 0.1) {
      el.pause();
      atEnd = true;
      return;
    }
    watch = requestAnimationFrame(endWatch);
  }
  function played() {
    const el = mediaEl;
    if (!el || via !== 'stream') return;
    if (atEnd) {
      atEnd = false;
      el.currentTime = 0;
    }
    if (!watch) watch = requestAnimationFrame(endWatch);
  }
  // playing counts as activity: the auto-lock never stops a film or a song halfway
  function playing() {
    if (mediaEl && !mediaEl.paused) idle.bump();
  }
  function seeking() {
    atEnd = false; // the reader moved: play from there
  }
  $effect(() => () => {
    if (watch) cancelAnimationFrame(watch);
  });

  const skip = skipper();
  function key(e: KeyboardEvent) {
    // j / l: back / forward 10 s, k: play/pause (the arrows: see appLook.videoArrows)
    const el = mediaEl;
    if (!el || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (t?.closest('input, textarea')) return;
    // a film in full screen: ←/→ seek too, further with each quick press (lib/skip.ts)
    const film = kind === 'video' && appLook.videoArrows === 'seek' && !!document.querySelector('.quick-look[data-full]');
    if (film && (e.key === 'ArrowLeft' || e.key === 'ArrowRight') && !e.shiftKey) {
      e.preventDefault();
      const dir = e.key === 'ArrowLeft' ? -1 : 1;
      const d = skip(e.timeStamp, dir) * dir;
      if (Number.isFinite(el.duration)) el.currentTime = Math.max(0, Math.min(el.duration, el.currentTime + d));
    } else if (e.key === 'j' || e.key === 'l') {
      e.preventDefault();
      const d = e.key === 'j' ? -10 : 10;
      if (Number.isFinite(el.duration)) el.currentTime = Math.max(0, Math.min(el.duration, el.currentTime + d));
    } else if (e.key === 'k' || (e.key === ' ' && !t?.closest('button, a, [role="slider"]'))) {
      e.preventDefault();
      if (el.paused) void el.play().catch(() => undefined);
      else el.pause();
    }
  }
</script>

<svelte:window onkeydown={key} />

<!-- not started / paused: the first frame, not black (the tile's frame as poster while it loads;
     `#t=0.001` makes WebKit decode and show the frame at the start with preload=metadata) -->


<div class="vw media-vw" data-testid="viewer-media" data-kind={kind} data-via={via}>
  {#if kind === 'video'}
    <div class="player">
      <!-- svelte-ignore a11y_media_has_caption, a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <video
        loop={via === 'stream'}
        src={src ? (src.includes('#') ? src : `${src}#t=0.001`) : undefined}
        poster={peekVideoThumb(vault.id, node) ?? undefined}
        preload="metadata"
        playsinline
        bind:this={mediaEl}
        onerror={error}
        onplay={played}
        onseeking={seeking}
        ontimeupdate={playing}
        ondblclick={() => !isMobile && window.dispatchEvent(new CustomEvent('nv:ql-fullscreen'))}
        onclick={() => (mediaEl?.paused ? void mediaEl.play().catch(() => undefined) : mediaEl?.pause())}
        use:decodeMedia={{ speed: 1, min: 0, key: `ql:${vault.id}:${node.id}` }}
      ></video>
      <MediaControls media={mediaEl} kind="video" />
    </div>
  {:else}
    <div class="audio">
      <span class="vw-plate"><FileIcon type="audio" size={34} /></span>
      <audio loop={via === 'stream'} src={src ?? undefined} preload="metadata" bind:this={mediaEl} onerror={error} onplay={played} onseeking={seeking} ontimeupdate={playing}></audio>
      <MediaControls media={mediaEl} kind="audio" />
      <p class="hint keys">{m.vw_media_keys()}</p>
    </div>
  {/if}
</div>

<style>
  .media-vw {
    align-items: center;
    justify-content: center;
  }
  /* the film takes the room above, letterboxed in it; the transport stays put at the foot,
     whatever the film's shape */
  .player {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
    width: min(880px, calc(100% - 64px));
    height: calc(100% - 48px);
    min-height: 0;
  }
  video {
    width: 100%;
    min-height: 0;
    flex: 1 1 0;
    object-fit: contain;
    background: transparent;
  }
  /* full screen: the film fills the window (contain), the transport floats over it above the dock
     and goes with the head and the dock (same slide + fade, through --nv-o / --nv-t) */
  :global(.quick-look[data-full]) .player {
    position: relative;
    width: 100%;
    height: 100%;
    gap: 0;
  }
  /* a film in full screen sits on pure black (letterbox bands, the sheet behind it) */
  :global(.quick-look[data-full][data-type='video']),
  :global(.quick-look[data-full][data-type='video'] .ql-body) {
    background: #000;
  }
  :global(.quick-look[data-full]) .player video {
    background: #000;
  }
  :global(.quick-look[data-full]) .player :global(.media-controls) {
    position: absolute;
    left: 50%;
    bottom: calc(14px + env(safe-area-inset-bottom, 0px));
    /* full screen: a long progress bar, grown smoothly from the window's 640 px */
    width: min(1200px, calc(100% - 64px));
    z-index: 2;
    will-change: opacity;
    --nv-o: 1;
    --nv-t: translate3d(-50%, 0, 0);
    opacity: var(--nv-o);
    transform: var(--nv-t);
    transition:
      --nv-o var(--dur-ui) var(--ease-out),
      --nv-t var(--dur-ui) var(--ease-out),
      width 420ms var(--ease-out);
  }
  :global(.quick-look[data-full]:has(.ql-dock)) .player :global(.media-controls) {
    bottom: calc(108px + env(safe-area-inset-bottom, 0px));
  }
  :global(.quick-look[data-chrome='hidden']) .player :global(.media-controls) {
    --nv-o: 0;
    --nv-t: translate3d(-50%, 18px, 0);
    pointer-events: none;
  }
  /* a definite width in both modes, so entering full screen animates it (see above) */
  .player :global(.media-controls) {
    width: min(640px, 100%);
  }
  .audio {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
    width: min(460px, calc(100% - 48px));
  }
  .keys {
    font-size: var(--text-xs);
  }
</style>
