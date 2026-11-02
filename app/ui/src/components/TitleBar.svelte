<script lang="ts">
  import { Copy, Minus, Square, X } from '@lucide/svelte';
  import { m } from '../paraglide/messages.js';
  import { closeWindow, frame, minimizeWindow, startDrag, toggleMaximizeWindow } from '../lib/windowFrame.svelte';

  let { title, compact = false }: { title: string; compact?: boolean } = $props();

  let press: { x: number; y: number } | null = null;
  function down(e: MouseEvent) {
    press = null;
    if (e.button !== 0 || (e.target as Element).closest('button')) return;
    if (e.detail === 2) {
      if (!compact) void toggleMaximizeWindow();
    } else press = { x: e.screenX, y: e.screenY };
  }
  function move(e: MouseEvent) {
    if (!press || !(e.buttons & 1)) return (press = null);
    if (Math.hypot(e.screenX - press.x, e.screenY - press.y) < 3) return;
    press = null;
    void startDrag();
  }
</script>

{#if frame.kind && !frame.fullscreen}
  <!-- svelte-ignore a11y_no_static_element_interactions (the pointer moves the window; the keyboard has the buttons and the system's own keys) -->
  <header class="titlebar ground" data-testid="titlebar" onmousedown={down} onmousemove={move} onmouseup={() => (press = null)}>
    <span class="tb-title">{title}</span>
    {#if frame.kind !== 'macos'}
      <div class="win-ctl" role="group" aria-label={m.window_controls()}>
        {#if !compact}
          <button class="win-btn" type="button" aria-label={m.window_minimize()} title={m.window_minimize()} onclick={() => void minimizeWindow()}>
            <Minus size={14} strokeWidth={1.6} aria-hidden="true" />
          </button>
          <button
            class="win-btn"
            type="button"
            aria-label={frame.maximized ? m.window_restore() : m.window_maximize()}
            title={frame.maximized ? m.window_restore() : m.window_maximize()}
            onclick={() => void toggleMaximizeWindow()}
          >
            {#if frame.maximized}<Copy size={12} strokeWidth={1.6} aria-hidden="true" />{:else}<Square size={11.5} strokeWidth={1.7} aria-hidden="true" />{/if}
          </button>
        {/if}
        <button class="win-btn close" type="button" aria-label={m.window_close()} title={m.window_close()} onclick={() => void closeWindow()}>
          <X size={15} strokeWidth={1.6} aria-hidden="true" />
        </button>
      </div>
    {/if}
  </header>
{/if}

<style>
  .titlebar {
    flex: none;
    z-index: 45;
    height: var(--tb-h);
    box-shadow: inset 0 -1px 0 color-mix(in oklab, var(--accent) 38%, transparent);
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding: 0 6px 0 var(--tb-lead);
    user-select: none;
    -webkit-user-select: none;
    color: var(--ink-3);
    transition: opacity 160ms ease-out;
  }
  .tb-title {
    position: absolute;
    left: 50%;
    max-width: calc(100% - 2 * 120px);
    transform: translateX(-50%);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    pointer-events: none;
  }
  :global([data-window-blur]) .titlebar {
    opacity: 0.55;
  }
  .win-ctl {
    display: flex;
    gap: 2px;
  }
  .win-btn {
    display: grid;
    place-items: center;
    width: 34px;
    height: calc(var(--tb-h) - 8px);
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--ink-2);
    transition:
      background-color 80ms ease-out,
      color 80ms ease-out;
  }
  .win-btn:hover {
    color: var(--ink);
    background: color-mix(in oklab, var(--ink) 10%, transparent);
  }
  .win-btn:active {
    background: color-mix(in oklab, var(--ink) 17%, transparent);
    transition: none;
  }
  .win-btn.close:hover,
  .win-btn.close:active {
    color: #fff;
    background: var(--danger);
  }
  .win-btn:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
</style>
