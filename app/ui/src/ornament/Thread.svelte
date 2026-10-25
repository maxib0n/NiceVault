<!--
  The foil stripe: a diffractive foil laid across the sheet from edge to edge.
  Its colours shift as the light moves (the pointer stands in for tilting the
  document; still under reduced motion). Fine grating lines and a repeated
  emblem are embossed into the foil. Pastel on paper; a dark silver on anthracite,
  the colours barely there (tokens: --thread-*).
-->
<script lang="ts">
  import { onMount } from 'svelte';

  let el: HTMLSpanElement | undefined = $state();

  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    let raf = 0;
    let tx = 0.5;
    let ty = 0.5;
    const move = (e: PointerEvent) => {
      tx = e.clientX / Math.max(1, innerWidth);
      ty = e.clientY / Math.max(1, innerHeight);
      if (!raf)
        raf = requestAnimationFrame(() => {
          raf = 0;
          el?.style.setProperty('--tilt', (tx * 0.65 + ty * 0.35).toFixed(3));
        });
    };
    addEventListener('pointermove', move, { passive: true });
    return () => {
      removeEventListener('pointermove', move);
      cancelAnimationFrame(raf);
    };
  });
</script>

<span class="thread" bind:this={el} aria-hidden="true"><span class="grating"></span><span class="glint"></span></span>

<style>
  .thread {
    --tilt: 0.5;
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background-color: var(--thread-base);
    background-image:
      /* diffraction: the hue shifts along the stripe and with the light */
      linear-gradient(180deg, var(--thread-rainbow));
    background-size: 100% 320%;
    background-position: 50% calc(var(--tilt) * 100%);
    box-shadow:
      inset 1px 0 0 var(--thread-lip),
      inset -1px 0 0 var(--thread-shade),
      1px 0 0 rgb(var(--cast) / 0.12),
      -1px 0 0 var(--deboss-lip);
  }
  /* the emblem embossed into the foil, repeated along the stripe */
  .thread::before {
    content: '';
    position: absolute;
    inset: 0;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='18' viewBox='0 0 16 18'%3E%3Cg fill='none' stroke-width='0.45'%3E%3Ccircle cx='8' cy='9' r='4.2' stroke='%23fff' stroke-opacity='.42'/%3E%3Cpath d='M8 4.8c1.1 1.8 1.1 6.6 0 8.4M3.8 9c1.8-1.1 6.6-1.1 8.4 0' stroke='%23fff' stroke-opacity='.36'/%3E%3Ccircle cx='8.4' cy='9.4' r='4.2' stroke='%23000' stroke-opacity='.08'/%3E%3C/g%3E%3C/svg%3E");
    background-size: 100% 18px;
    opacity: var(--thread-emboss);
  }
  /* grating lines: the rainbow comes from these */
  .grating {
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(118deg, rgb(255 255 255 / 0.55) 0 0.5px, transparent 0.5px 1.7px);
    mix-blend-mode: soft-light;
    opacity: var(--thread-grating);
  }
  /* the specular glint travels against the colours */
  .glint {
    position: absolute;
    inset: -20% 0;
    background: linear-gradient(180deg, transparent 0%, rgb(255 255 255 / var(--thread-glint)) 48%, transparent 56%);
    background-size: 100% 240%;
    background-position: 50% calc((1 - var(--tilt)) * 100%);
    mix-blend-mode: soft-light;
  }
</style>
