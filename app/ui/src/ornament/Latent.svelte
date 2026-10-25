<!--
  An anti-copy line screen with a latent image: a field of fine diagonal lines in
  which a word is set in lines of the opposite slant, same weight, same pitch. At
  reading distance the field is an even tint; tilt it (or zoom, or copy it) and the
  word appears. Fills its positioned parent.
-->
<script lang="ts">
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';
  import { hair } from './ink.svelte';

  let { word }: { word: string } = $props();

  function lines(c: CanvasRenderingContext2D, w: number, h: number, pitch: number, slant: 1 | -1) {
    c.beginPath();
    const span = w + h;
    for (let x = -h; x < span; x += pitch) {
      c.moveTo(x, slant > 0 ? h : 0);
      c.lineTo(x + h, slant > 0 ? 0 : h);
    }
    c.stroke();
  }

  function layer(env: InkEnv, w: number, h: number, paint: (c: CanvasRenderingContext2D) => void) {
    const c = document.createElement('canvas');
    c.width = Math.max(1, Math.round(w * env.dpr));
    c.height = Math.max(1, Math.round(h * env.dpr));
    const x = c.getContext('2d')!;
    x.setTransform(c.width / w, 0, 0, c.height / h, 0, 0);
    paint(x);
    return c;
  }

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const ink = env.ink('color-mix(in srgb, var(--engrave) 52%, transparent)');
    const pitch = env.dpr >= 2 ? 2 : 2.6;
    const lw = hair(env.dpr) * 0.8;
    const fs = h * 0.92;
    const setText = (c: CanvasRenderingContext2D) => {
      c.font = `700 ${fs}px "IBM Plex Sans", sans-serif`;
      c.textAlign = 'center';
      c.textBaseline = 'middle';
      c.fillText(word, w / 2, h / 2 + fs * 0.04);
    };
    const field = layer(env, w, h, (c) => {
      c.strokeStyle = ink;
      c.lineWidth = lw;
      lines(c, w, h, pitch, 1);
      c.globalCompositeOperation = 'destination-out';
      setText(c);
    });
    const latent = layer(env, w, h, (c) => {
      c.strokeStyle = ink;
      c.lineWidth = lw;
      lines(c, w, h, pitch, -1);
      c.globalCompositeOperation = 'destination-in';
      setText(c);
    });
    ctx.drawImage(field, 0, 0, w, h);
    ctx.drawImage(latent, 0, 0, w, h);
  }
</script>

<span class="latent" aria-hidden="true"><InkCanvas class="latent-ink" {draw} /></span>

<style>
  .latent {
    display: block;
    width: 100%;
    height: 100%;
  }
  .latent > :global(.latent-ink) {
    width: 100%;
    height: 100%;
  }
</style>
