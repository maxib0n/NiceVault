<script lang="ts">
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';

  let { text }: { text: string } = $props();

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const fs = env.dpr >= 2 ? 3.5 : 3.9;
    ctx.font = `500 ${fs}px "IBM Plex Mono", monospace`;
    ctx.textBaseline = 'middle';
    ctx.fillStyle = env.ink('color-mix(in srgb, var(--engrave) 82%, transparent)');
    const unit = ctx.measureText(text).width || 60;
    for (let x = 0; x < w; x += unit) ctx.fillText(text, x, h / 2 + 0.25);
  }
</script>

<span class="microline" aria-hidden="true"><InkCanvas class="micro-ink" {draw} cache="micro|{text}" /></span>

<style>
  .microline {
    display: block;
    height: 6px;
    overflow: hidden;
  }
  .microline > :global(.micro-ink) {
    width: 100%;
    height: 100%;
  }
</style>
