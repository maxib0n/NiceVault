<!--
  Transport for quick look audio/video, replacing the webview's native controls
  (they differ per OS and break the material). Presentation only: it reflects
  the media element's real state and forwards play / seek / mute / fullscreen.
  Space and arrows stay here, so they do not close or step the quick look.
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Maximize, Pause, Play, Settings2, Volume2, VolumeX } from '@lucide/svelte';
  import { isIos, isMobile } from '../lib/platform';

  let { media, kind }: { media: HTMLMediaElement | undefined; kind: 'audio' | 'video' } = $props();

  let playing = $state(false);
  let time = $state(0);
  let duration = $state(0);
  let muted = $state(false);

  // ---- speed, embedded / sidecar subtitles, audio tracks (desktop; phones keep the native player) ----
  const RATES = [0.5, 0.75, 1, 1.25, 1.5, 2];
  /** the speed is kept for the session (module-level in this file's instance chain via window) */
  const SESSION = '__nvRate';
  const getRate = () => ((globalThis as Record<string, unknown>)[SESSION] as number | undefined) ?? 1;
  let rate = $state(getRate());
  type Tr = { id: number; label: string };
  let subs = $state<Tr[]>([]);
  let subOn = $state(-1);
  let auds = $state<Tr[]>([]);
  let audOn = $state(0);
  let menu = $state(false);
  const rich = $derived(!isMobile && kind === 'video');

  function setRate(r: number) {
    rate = r;
    (globalThis as Record<string, unknown>)[SESSION] = r;
    if (media) media.playbackRate = r;
  }
  function stepRate(d: number) {
    const i = RATES.indexOf(rate);
    const n = Math.max(0, Math.min(RATES.length - 1, (i < 0 ? 2 : i) + d));
    setRate(RATES[n]);
  }
  const texts = (el: HTMLMediaElement) => [...el.textTracks].filter((t) => t.kind === 'subtitles' || t.kind === 'captions');
  const nameOf = (t: { label?: string; language?: string }, i: number) => t.label || t.language || m.mc_track({ n: i + 1 });
  function scanTracks() {
    const el = media as (HTMLMediaElement & { audioTracks?: { length: number; [i: number]: { label: string; language: string; enabled: boolean } } }) | undefined;
    if (!el || !rich) return;
    const ts = texts(el);
    subs = ts.map((t, i) => ({ id: i, label: nameOf(t, i) }));
    subOn = ts.findIndex((t) => t.mode === 'showing');
    const at = el.audioTracks;
    auds = at && at.length > 1 ? Array.from({ length: at.length }, (_, i) => ({ id: i, label: nameOf(at[i], i) })) : [];
    audOn = at ? Math.max(0, Array.from({ length: at.length }, (_, i) => at[i]).findIndex((t) => t.enabled)) : 0;
  }
  function pickSub(i: number) {
    if (!media) return;
    texts(media).forEach((t, k) => (t.mode = k === i ? 'showing' : 'disabled'));
    scanTracks();
  }
  function pickAudio(i: number) {
    const at = (media as (HTMLMediaElement & { audioTracks?: { length: number; [i: number]: { enabled: boolean } } }) | undefined)?.audioTracks;
    if (!at) return;
    for (let k = 0; k < at.length; k++) at[k].enabled = k === i;
    scanTracks();
  }
  $effect(() => {
    const el = media;
    if (!el || !rich) return;
    el.playbackRate = rate;
    const again = () => {
      el.playbackRate = rate;
      scanTracks();
    };
    const lists = [el.textTracks, (el as unknown as { audioTracks?: EventTarget }).audioTracks].filter(Boolean) as EventTarget[];
    for (const l of lists) for (const e of ['addtrack', 'removetrack', 'change']) l.addEventListener(e, scanTracks);
    el.addEventListener('loadedmetadata', again);
    el.addEventListener('loadeddata', again);
    scanTracks();
    return () => {
      for (const l of lists) for (const e of ['addtrack', 'removetrack', 'change']) l.removeEventListener(e, scanTracks);
      el.removeEventListener('loadedmetadata', again);
      el.removeEventListener('loadeddata', again);
    };
  });
  function windowKey(e: KeyboardEvent) {
    if (!rich || e.ctrlKey || e.metaKey || e.altKey || (e.target as HTMLElement | null)?.closest('input, textarea')) return;
    if (e.key === '<' || e.key === '>') {
      e.preventDefault();
      stepRate(e.key === '>' ? 1 : -1);
    }
  }
  let root: HTMLElement | undefined = $state();
  function outside(e: PointerEvent) {
    if (menu && root && !root.contains(e.target as Node)) menu = false;
  }

  $effect(() => {
    const m = media;
    if (!m) return;
    const sync = () => {
      playing = !m.paused && !m.ended;
      time = m.currentTime;
      duration = Number.isFinite(m.duration) ? m.duration : 0;
      muted = m.muted;
    };
    const events = ['play', 'pause', 'timeupdate', 'durationchange', 'loadedmetadata', 'volumechange', 'ended', 'seeked'];
    for (const e of events) m.addEventListener(e, sync);
    sync();
    return () => {
      for (const e of events) m.removeEventListener(e, sync);
    };
  });

  const fmt = (s: number) => {
    const t = Math.max(0, Math.floor(s));
    return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
  };

  function toggle() {
    if (!media) return;
    if (media.paused || media.ended) void media.play().catch(() => undefined);
    else media.pause();
  }

  function seek(v: number) {
    if (media && duration > 0) media.currentTime = v;
  }

  // desktop: our own full screen (QuickLook + the window, never the webview's native player);
  // phones keep the platform's player, which has its own speed / subtitle / audio pickers
  function fullscreen() {
    if (!isMobile) return void window.dispatchEvent(new CustomEvent('nv:ql-fullscreen'));
    const v = media as (HTMLVideoElement & { webkitEnterFullscreen?: () => void }) | undefined;
    if (isIos && v?.webkitEnterFullscreen) v.webkitEnterFullscreen();
    else if (v && 'requestFullscreen' in v) void v.requestFullscreen().catch(() => undefined);
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && menu) {
      e.stopPropagation();
      e.preventDefault();
      menu = false;
    }
    if (e.key === ' ' || e.key.startsWith('Arrow') || e.key === 'Home' || e.key === 'End') e.stopPropagation();
  }
</script>

<svelte:window onkeydown={windowKey} onpointerdown={outside} />
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="media-controls" data-kind={kind} onkeydown={keydown} bind:this={root}>
  <button type="button" class="btn icon play" aria-label={playing ? m.mc_pause() : m.mc_play()} onclick={toggle}>
    {#if playing}<Pause size={16} />{:else}<Play size={16} />{/if}
  </button>
  <span class="time num" aria-hidden="true">{fmt(time)}</span>
  <input
    class="scrubber"
    type="range"
    min="0"
    max={duration || 0}
    step="0.01"
    value={time}
    disabled={!duration}
    aria-label={m.mc_position()}
    aria-valuetext={m.mc_of({ a: fmt(time), b: fmt(duration) })}
    style="--p: {duration ? (time / duration) * 100 : 0}%"
    oninput={(e) => seek(Number(e.currentTarget.value))}
  />
  <span class="time num" aria-hidden="true">{fmt(duration)}</span>
  <span class="tail">
  <button
    type="button"
    class="btn ghost icon"
    aria-label={muted ? m.mc_unmute() : m.mc_mute()}
    aria-pressed={muted}
    onclick={() => media && (media.muted = !media.muted)}
  >
    {#if muted}<VolumeX size={16} />{:else}<Volume2 size={16} />{/if}
  </button>
  {#if rich}
    <button type="button" class="btn ghost icon" aria-label={m.mc_options()} title={m.mc_options()} aria-haspopup="true" aria-expanded={menu} data-testid="mc-options" onclick={() => (menu = !menu)}>
      <Settings2 size={15} />
    </button>
  {/if}
  {#if kind === 'video'}
    <button type="button" class="btn ghost icon" data-testid="mc-fullscreen" aria-label={m.mc_fullscreen()} title={m.mc_fullscreen()} onclick={fullscreen}>
      <Maximize size={15} />
    </button>
  {/if}
  </span>
  {#if menu && rich}
    <div class="mc-menu" role="group" aria-label={m.mc_options()} data-testid="mc-menu">
      <div class="mc-group" role="radiogroup" aria-label={m.mc_speed()}>
        <span class="mc-h">{m.mc_speed()}</span>
        {#each RATES as r (r)}
          <button type="button" role="radio" aria-checked={rate === r} class="mc-opt" onclick={() => setRate(r)}>{r}×</button>
        {/each}
      </div>
      {#if subs.length}
        <div class="mc-group" role="radiogroup" aria-label={m.mc_subtitles()}>
          <span class="mc-h">{m.mc_subtitles()}</span>
          <button type="button" role="radio" aria-checked={subOn < 0} class="mc-opt" onclick={() => pickSub(-1)}>{m.mc_none()}</button>
          {#each subs as t (t.id)}
            <button type="button" role="radio" aria-checked={subOn === t.id} class="mc-opt" onclick={() => pickSub(t.id)}>{t.label}</button>
          {/each}
        </div>
      {/if}
      {#if auds.length}
        <div class="mc-group" role="radiogroup" aria-label={m.mc_audio()}>
          <span class="mc-h">{m.mc_audio()}</span>
          {#each auds as t (t.id)}
            <button type="button" role="radio" aria-checked={audOn === t.id} class="mc-opt" onclick={() => pickAudio(t.id)}>{t.label}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* the transport: a paper strip with a round primary key and a groove for time */
  .media-controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 7px 10px 7px 7px;
    position: relative;
    border-radius: var(--radius-pill);
    background: var(--paper-hi);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--key-edge),
      var(--lift-key-hover);
  }
  /* the keys at the end sit close together, a pair */
  .tail {
    display: flex;
    flex: none;
    gap: 0;
    margin-left: -4px;
  }
  .tail > :global(.btn.icon) {
    width: 32px;
    min-width: 32px;
  }
  .play {
    --face-hi: var(--key-accent-hi);
    --face-lo: var(--key-accent-lo);
    --edge: var(--key-accent-edge);
    --edge-lo: color-mix(in oklab, var(--key-accent-edge) 70%, #000);
    --lip: rgb(255 255 255 / 0.3);
    width: 36px;
    min-height: 36px;
    border-radius: 50%;
    color: var(--accent-ink);
  }
  .play > :global(svg) {
    color: var(--accent-ink);
  }
  .time {
    flex: none;
    min-width: 38px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--ink-3);
    text-align: center;
  }
  /* the scrubber: a groove pressed into the strip, inked up to now */
  .scrubber {
    -webkit-appearance: none;
    appearance: none;
    flex: 1;
    min-width: 60px;
    height: 22px;
    margin: 0;
    background: transparent;
  }
  .scrubber::-webkit-slider-runnable-track {
    height: 5px;
    border-radius: 3px;
    background: linear-gradient(90deg, var(--accent) var(--p), var(--paper-sunk) var(--p));
    box-shadow:
      inset 0 1px 1px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .scrubber::-moz-range-track {
    height: 5px;
    border-radius: 3px;
    background: linear-gradient(90deg, var(--accent) var(--p), var(--paper-sunk) var(--p));
  }
  .scrubber::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 15px;
    height: 15px;
    margin-top: -5px;
    border-radius: 50%;
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    box-shadow:
      0 0 0 1px var(--key-edge-lo),
      0 1px 2px rgb(var(--cast) / 0.35);
  }
  .scrubber::-moz-range-thumb {
    width: 15px;
    height: 15px;
    border: 0;
    border-radius: 50%;
    background: var(--key-hi);
    box-shadow: 0 1px 2px rgb(var(--cast) / 0.35);
  }
  .scrubber:hover::-webkit-slider-thumb {
    box-shadow:
      0 0 0 1px var(--accent),
      0 2px 4px rgb(var(--cast) / 0.35);
  }
  .scrubber:focus-visible {
    outline: none;
  }
  .scrubber:focus-visible::-webkit-slider-thumb {
    box-shadow:
      0 0 0 2px var(--paper-hi),
      0 0 0 4px var(--focus);
  }
  .scrubber:disabled::-webkit-slider-thumb {
    visibility: hidden;
  }
  .mc-menu {
    position: absolute;
    right: 8px;
    bottom: calc(100% + 8px);
    z-index: 3;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 160px;
    max-height: 60vh;
    overflow: auto;
    padding: var(--space-2);
    border-radius: 14px;
    background: var(--paper-hi);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--key-edge),
      var(--shadow-pop);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateY(4px);
    --enter-to: translateY(0);
    opacity: var(--nv-o);
    transform: var(--nv-t);
  }
  .mc-group {
    display: flex;
    flex-direction: column;
  }
  .mc-h {
    padding: 2px 8px;
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .mc-opt {
    padding: 5px 8px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--ink);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .mc-opt:hover {
    background: var(--paper-sunk);
  }
  .mc-opt[aria-checked='true'] {
    color: var(--accent);
    font-weight: 600;
  }
  .mc-opt:focus-visible {
    outline: 2px solid var(--focus);
  }
</style>
