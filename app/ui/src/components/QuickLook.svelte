<script lang="ts">
  import { pointerLast } from '../lib/modality';
  import { m } from '../paraglide/messages.js';
  import { appLook } from '../lib/stores/applook.svelte';
  import { flushSync } from 'svelte';
  import { ChevronLeft, ChevronRight, Download, ExternalLink, Maximize2, Minimize2, X } from '@lucide/svelte';
  import { isTauri, previewUrl } from '../lib/api';
  import { fetchAsBlob, isSourceRejected, MEDIA_BLOB_MAX } from '../lib/mediaBlob';
  import { fileType, typeLabel } from '../lib/filetype';
  import { formatBytes, formatDate, nsToDate } from '../lib/format';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import FileIcon from './FileIcon.svelte';
  import MediaControls from './MediaControls.svelte';
  import PdfView from './PdfView.svelte';
  import TextView from './TextView.svelte';
  import { decodeMedia } from '../lib/reveal';
  import Viewer from '../viewers/Viewer.svelte';
  import { openMounted } from '../viewers/actions';
  const VIEWERS = true;
  import { isMobile } from '../lib/platform';
  import { pinchZoom, swipe } from '../lib/gestures';
  import QlDock from './QlDock.svelte';

  let { browser }: { browser: FileBrowser } = $props();

  let el: HTMLDialogElement | undefined = $state();
  let mediaEl: HTMLMediaElement | undefined = $state();
  const node = $derived(browser.previewNode);
  const type = $derived(node ? fileType(node) : 'other');
  const url = $derived(node ? previewUrl(browser.vault.id, node.id) : '');
  const fitted = $derived(node && type === 'image' ? previewUrl(browser.vault.id, node.id, undefined, Math.ceil(Math.max(innerWidth, innerHeight) * (devicePixelRatio || 1))) : '');

  let blobUrl = $state<string | null>(null);
  let mediaFailed = $state(false);
  let blobAbort: AbortController | null = null;
  const mediaSrc = $derived(blobUrl ?? url);
  $effect(() => {
    void node?.id;
    return () => {
      blobAbort?.abort();
      blobAbort = null;
      if (blobUrl) URL.revokeObjectURL(blobUrl);
      blobUrl = null;
      mediaFailed = false;
    };
  });
  function mediaError() {
    const n = node;
    if (!n || blobUrl || blobAbort || !isSourceRejected(mediaEl) || n.size > MEDIA_BLOB_MAX) {
      if (!blobAbort) mediaFailed = true;
      return;
    }
    const ac = (blobAbort = new AbortController());
    fetchAsBlob(url, n.size, ac.signal).then(
      (b) => {
        if (ac.signal.aborted) return;
        blobUrl = URL.createObjectURL(b);
        blobAbort = null;
      },
      () => {
        if (!ac.signal.aborted) mediaFailed = true;
        blobAbort = null;
      },
    );
  }
  const many = $derived(browser.previewable.length > 1);

  const IDLE_MS = 2000;
  let fullChosen = $state(false);
  const full = $derived(isMobile || fullChosen);
  let chrome = $state(true);
  let zoomed = $state(false);
  let headH = $state(0);
  let idleTimer: ReturnType<typeof setTimeout> | null = null;
  let held = false;
  function wake() {
    chrome = true;
    if (idleTimer) clearTimeout(idleTimer);
    idleTimer = full && !held ? setTimeout(() => (chrome = false), IDLE_MS) : null;
  }
  function hold(on: boolean) {
    held = on;
    wake();
  }
  $effect(() => {
    void full;
    void node?.id;
    wake();
    return () => {
      if (idleTimer) clearTimeout(idleTimer);
    };
  });
  const EDGE = 120;
  let lastMouse: { x: number; y: number } | null = null;
  let scrolledAt = 0;
  let downType = '';
  function pointerMoved(e: PointerEvent) {
    if (!full || e.pointerType !== 'mouse') return;
    const prev = lastMouse;
    lastMouse = { x: e.clientX, y: e.clientY };
    if (!prev || (prev.x === e.clientX && prev.y === e.clientY)) return;
    if (performance.now() - scrolledAt < 400) return;
    const h = el?.clientHeight ?? innerHeight;
    if (chrome || e.clientY < EDGE || e.clientY > h - EDGE) wake();
  }
  function setFull(on: boolean) {
    morph(() => (fullChosen = on));
    queueMicrotask(() => el?.focus());
  }

  $effect(() => {
    if (!el) return;
    if (node && !el.open) {
      el.showModal();
      if (pointerLast()) el.focus();
    } else if (!node && el.open) el.close();
  });

  let leaving = false;
  const calm = () => !matchMedia('(prefers-reduced-motion: reduce)').matches;
  function close(instant = false) {
    if (leaving) return;
    const done = () => {
      leaving = false;
      fullChosen = false;
      browser.quickLook = null;
      queueMicrotask(() => document.querySelector<HTMLElement>('.file-view')?.focus());
    };
    if (instant || !el?.open || !calm()) return done();
    leaving = true;
    const a = el.animate([{ '--nv-o': '1', '--nv-t': 'scale(1)' }, { '--nv-o': '0', '--nv-t': 'scale(0.97)' }] as Keyframe[], {
      duration: 160,
      easing: 'cubic-bezier(0.2, 0, 0.4, 1)',
      fill: 'forwards',
    });
    a.finished.then(
      () => (a.cancel(), done()),
      () => done(),
    );
  }
  function morph(change: () => void) {
    if (!el || !calm()) return change();
    const from = el.getBoundingClientRect();
    change();
    flushSync();
    const to = el.getBoundingClientRect();
    if (!to.width || !to.height) return;
    const sx = from.width / to.width;
    const sy = from.height / to.height;
    el.style.transformOrigin = '0 0';
    const a = el.animate(
      [
        { '--nv-t': `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${sx}, ${sy})` },
        { '--nv-t': 'translate(0px, 0px) scale(1, 1)' },
      ] as Keyframe[],
      { duration: 240, easing: 'cubic-bezier(0.2, 0, 0.2, 1)' },
    );
    const end = () => (el!.style.transformOrigin = '');
    a.addEventListener('finish', end);
    a.addEventListener('cancel', end);
  }

  $effect(() => {
    const on = () => !isMobile && node && setFull(!fullChosen);
    window.addEventListener('nv:ql-fullscreen', on);
    return () => window.removeEventListener('nv:ql-fullscreen', on);
  });
  let winFull = false;
  $effect(() => {
    const on = !!(isTauri && !isMobile && fullChosen && type === 'video' && node);
    if (on === winFull) return;
    winFull = on;
    void import('@tauri-apps/api/window').then((w) => w.getCurrentWindow().setFullscreen(on)).catch(() => undefined);
  });

  function keydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    const inMedia = t.tagName === 'VIDEO' || t.tagName === 'AUDIO' || e.altKey;
    const film = full && appLook.videoArrows === 'seek' && !!el?.querySelector('[data-testid="viewer-media"][data-kind="video"]');
    if (full && !['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End'].includes(e.key)) wake();
    if ((e.key === 'f' || e.key === 'F') && !e.ctrlKey && !e.metaKey && !e.altKey && !isMobile && !t.closest('input, textarea')) {
      e.preventDefault();
      setFull(!fullChosen);
    } else if (e.key === ' ' && !inMedia && !el?.querySelector('[data-testid="viewer-media"]')) {
      e.preventDefault();
      close();
    } else if (e.key === 'ArrowRight' && !inMedia && !film) {
      e.preventDefault();
      browser.stepQuickLook(1);
    } else if (e.key === 'ArrowLeft' && !inMedia && !film) {
      e.preventDefault();
      browser.stepQuickLook(-1);
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={el}
  class="quick-look nv-enter"
  tabindex="-1"
  data-type={type}
  data-full={full ? 'true' : undefined}
  data-chrome={full ? (chrome ? 'shown' : 'hidden') : undefined}
  data-zoomed={zoomed ? 'true' : undefined}
  style:--ql-head-h={full && headH ? `${headH}px` : undefined}
  aria-label={node ? m.ql_preview_of({ name: node.name }) : m.ql_preview()}
  oncancel={(e) => {
    // Esc in full screen (desktop): back to the window-sized preview, not closed
    e.preventDefault();
    if (fullChosen && !isMobile) setFull(false);
    else close();
  }}
  onpointermove={pointerMoved}
  onpointerdowncapture={(e) => (downType = e.pointerType)}
  onscrollcapture={() => (scrolledAt = performance.now())}
  onwheelcapture={() => (scrolledAt = performance.now())}
  onclick={(e) => {
    // a click on the stage (not a key, a link, a field) brings the chrome back
    if (full && !chrome && downType === 'mouse' && !(e.target as Element).closest('a[href], button, input, select, textarea, summary, label, [role="button"]')) wake();
  }}
  onclose={() => {
    // a close event is queued as a task: if a key reopened the preview before it was
    // delivered, the dialog is open again and the stale event must not shut it
    if (el?.open) return;
    if (browser.quickLook !== null) close();
  }}
  onkeydown={keydown}
>
  {#if node}
    <header class="ql-head" bind:clientHeight={headH}>
      <div class="ql-title">
        <span class="ql-glyph"><FileIcon {type} size={16} /></span>
        <span class="ql-name" title={node.name}>{node.name}</span>
        <span class="ql-meta">
          {typeLabel(type)} · <span class="num">{formatBytes(node.size)}</span> · <span class="num">{formatDate(nsToDate(node.mtime))}</span>
        </span>
      </div>
      <div class="ql-actions">
        {#if many}
          <button type="button" class="btn ghost icon" aria-label={m.ql_prev()} onclick={() => browser.stepQuickLook(-1)}>
            <ChevronLeft size={16} />
          </button>
          <button type="button" class="btn ghost icon" aria-label={m.ql_next()} onclick={() => browser.stepQuickLook(1)}>
            <ChevronRight size={16} />
          </button>
        {/if}
        {#if !VIEWERS}
          <button type="button" class="btn ghost" onclick={() => browser.openExternal(node.id)}>
            <ExternalLink size={16} aria-hidden="true" /> {m.ql_open_with()}
          </button>
        {:else if browser.vault.mountpoint}
          <button type="button" class="btn ghost" onclick={() => openMounted(browser.vault.id, node.id)}>
            <ExternalLink size={16} aria-hidden="true" /> {m.vw_open_external()}
          </button>
        {/if}
        <button type="button" class="btn ghost" onclick={() => browser.exportNodes([node.id])}>
          <Download size={16} aria-hidden="true" /> {m.tb_export()}
        </button>
        {#if !isMobile}
          <button type="button" class="btn ghost icon ql-full" aria-label={fullChosen ? m.ql_exit_fullscreen() : m.ql_fullscreen()} aria-pressed={fullChosen} title="{fullChosen ? m.ql_exit_fullscreen() : m.ql_fullscreen()} (F)" data-testid="ql-fullscreen" onclick={() => setFull(!fullChosen)}>
            {#if fullChosen}<Minimize2 size={16} />{:else}<Maximize2 size={16} />{/if}
          </button>
        {/if}
        <!-- svelte-ignore a11y_autofocus -->
        <button type="button" class="btn ghost icon" aria-label={m.ql_close()} onclick={() => close()} autofocus>
          <X size={16} />
        </button>
      </div>
    </header>

    <div
      class="ql-body"
      use:swipe={{
        onLeft: () => browser.stepQuickLook(1),
        onRight: () => browser.stepQuickLook(-1),
        onDown: () => close(true),
        onTap: () => (full && chrome ? (chrome = false) : wake()),
        enabled: () => !zoomed,
      }}
    >
      {#key node.id}
        {#if VIEWERS}
          <Viewer vault={browser.vault} {node} {url} onexport={() => browser.exportNodes([node.id])} onzoom={(z) => (zoomed = z)} />
        {:else if !url}
          <div class="ql-none"><span class="ql-plate"><FileIcon {type} size={34} /></span><p class="hint">{m.ql_none()}</p></div>
        {:else if type === 'image'}
          <span class="ql-print" use:pinchZoom={(z) => (zoomed = z)}><img class="ql-image" src={zoomed ? url : fitted} alt={node.name} decoding="async" use:decodeMedia={{ speed: 1.6, min: 0, key: `ql:${browser.vault.id}:${node.id}` }} /></span>
        {:else if type === 'video'}
          <div class="ql-player">
            <!-- svelte-ignore a11y_media_has_caption, a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
            <video
              class="ql-video"
              src={mediaSrc}
              preload="metadata"
              bind:this={mediaEl}
              onerror={mediaError}
              onclick={() => (mediaEl?.paused ? void mediaEl.play().catch(() => undefined) : mediaEl?.pause())}
              use:decodeMedia={{ speed: 1, min: 0, key: `ql:${browser.vault.id}:${node.id}` }}
            ></video>
            <MediaControls media={mediaEl} kind="video" />
            {#if mediaFailed}<p class="hint" role="status">{m.ql_video_failed()}</p>{/if}
          </div>
        {:else if type === 'audio'}
          <div class="ql-audio">
            <span class="ql-plate"><FileIcon {type} size={34} /></span>
            <audio src={mediaSrc} preload="metadata" bind:this={mediaEl} onerror={mediaError}></audio>
            <MediaControls media={mediaEl} kind="audio" />
            {#if mediaFailed}<p class="hint" role="status">{m.ql_audio_failed()}</p>{/if}
          </div>
        {:else if type === 'pdf'}
          <PdfView {url} />
        {:else if type === 'text' || type === 'key'}
          <TextView {url} />
        {:else}
          <div class="ql-none"><span class="ql-plate"><FileIcon {type} size={34} /></span><p class="hint">{m.ql_none()}</p></div>
        {/if}
      {/key}
    </div>
    {#if full}
      <QlDock {browser} shown={chrome} onhold={hold} />
    {/if}
  {/if}
</dialog>

<style>
  .quick-look {
    width: min(1180px, calc(100vw - 56px));
    height: min(840px, calc(100vh - 56px));
    max-width: none;
    max-height: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius-lg);
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      var(--shadow-pop);
    overflow: hidden;
  }
  .quick-look[open] {
    display: flex;
    flex-direction: column;
    animation: nv-enter 240ms var(--ease-out) backwards;
    --enter-from: translateY(10px) scale(0.965);
    --enter-to: translateY(0) scale(1);
  }
  @media (prefers-reduced-motion: reduce) {
    .quick-look[data-full] .ql-head {
      transition: none;
    }
  }
  .quick-look[data-full] {
    width: 100vw;
    height: 100dvh;
    border-radius: 0;
    position: fixed;
    inset: 0;
  }
  .quick-look[data-full] .ql-head {
    position: absolute;
    inset: 0 0 auto;
    z-index: 2;
    background: color-mix(in oklab, var(--paper-hi) 86%, transparent);
    backdrop-filter: blur(10px);
    will-change: opacity;
    --nv-o: 1;
    --nv-t: translate3d(0, 0, 0);
    opacity: var(--nv-o);
    transform: var(--nv-t);
    transition:
      --nv-o var(--dur-ui) var(--ease-out),
      --nv-t var(--dur-ui) var(--ease-out);
  }
  .quick-look[data-chrome='hidden'] .ql-head {
    --nv-o: 0;
    --nv-t: translate3d(0, -8px, 0);
    pointer-events: none;
  }
  .quick-look[data-chrome='hidden'] {
    cursor: none;
  }
  .quick-look[data-full] :global(.vw-scroll) {
    padding-top: calc(var(--ql-head-h, 0px) + var(--space-5));
    padding-bottom: calc(var(--space-7) + 112px);
    scroll-padding-top: var(--ql-head-h, 0px);
  }
  .quick-look[data-full]:has(:global(.ql-dock)) :global(.vw-bar) {
    translate: 0 var(--nv-dy);
    transition: --nv-dy var(--dur-ui) var(--ease-out);
  }
  .quick-look[data-chrome='shown']:has(:global(.ql-dock)) :global(.vw-bar) {
    --nv-dy: calc(var(--space-4) - 108px - env(safe-area-inset-bottom, 0px));
  }
  .quick-look[data-full] .ql-image {
    max-width: calc(100vw - 32px);
    max-height: calc(100dvh - 32px);
  }
  .ql-body {
    position: relative;
    touch-action: pan-y;
  }
  .quick-look[data-zoomed] .ql-body {
    touch-action: pan-x pan-y;
  }
  .quick-look:is([data-type='image'], [data-type='video']) .ql-body {
    touch-action: none;
  }
  .quick-look::backdrop {
    background: var(--scrim);
  }
  .ql-head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 58px;
    padding: 8px var(--space-3) 8px var(--space-4);
    border-bottom: 1px solid var(--paper-rule);
    flex: none;
  }
  .ql-title {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    grid-template-areas:
      'glyph name'
      'glyph meta';
    column-gap: var(--space-3);
    align-items: center;
  }
  .ql-glyph {
    grid-area: glyph;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .ql-name {
    grid-area: name;
    font-size: var(--text-base);
    font-weight: 600;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }
  .ql-meta {
    grid-area: meta;
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
  .ql-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }
  .ql-body {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--stage);
    background-image: radial-gradient(90% 80% at 30% 20%, color-mix(in oklab, var(--paper-hi) 45%, transparent), transparent 70%);
    box-shadow: inset 0 2px 6px rgb(var(--cast) / 0.12);
  }
  .ql-print {
    --ascii-surface: var(--print-develop);
    display: block;
    padding: 8px;
    background:
      linear-gradient(var(--print-develop), var(--print-develop)) content-box,
      var(--print-border);
    box-shadow:
      0 0 0 1px rgb(var(--cast) / 0.08),
      0 2px 4px rgb(var(--cast) / 0.1),
      0 22px 44px -16px rgb(var(--cast) / 0.45),
      var(--print-glow);
  }
  .ql-image {
    display: block;
    max-width: calc(min(1180px, 100vw - 56px) - 72px - 16px);
    max-height: calc(min(840px, 100vh - 56px) - 58px - 72px - 16px);
    object-fit: contain;
  }
  .ql-player {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    width: min(880px, calc(100% - 64px));
    max-height: calc(100% - 48px);
  }
  .ql-video {
    max-width: 100%;
    min-height: 0;
    flex: 1 1 auto;
    max-height: calc(100% - 72px);
    object-fit: contain;
    border-radius: 2px;
    background: #000;
    box-shadow: 0 22px 44px -16px rgb(var(--cast) / 0.5);
  }
  .ql-player :global(.media-controls) {
    max-width: 640px;
  }
  .ql-audio,
  .ql-none {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
    width: min(460px, calc(100% - 48px));
  }
  .ql-plate {
    display: grid;
    place-items: center;
    width: 88px;
    height: 88px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--paper-hi);
    box-shadow:
      0 0 0 1px var(--paper-rule),
      0 2px 4px rgb(var(--cast) / 0.12),
      0 10px 20px -10px rgb(var(--cast) / 0.3);
  }
  .ql-plate :global(svg *) {
    vector-effect: non-scaling-stroke;
    stroke-width: 1.5px;
  }
</style>
