<script lang="ts">
  import { prng } from './lathe';

  let { size = 26, seed = 1, broken = false }: { size?: number; seed?: number; broken?: boolean } = $props();

  const uid = `wax-${Math.random().toString(36).slice(2, 8)}`;

  const blob = $derived.by(() => {
    const r = prng(seed * 7919 + 17);
    const n = 64;
    const lobes = Array.from({ length: 5 }, () => ({ a: r() * Math.PI * 2, w: 0.25 + r() * 0.5, h: 0.03 + r() * 0.07 }));
    let d = '';
    for (let i = 0; i <= n; i++) {
      const t = (i / n) * Math.PI * 2;
      let rad = 44;
      for (const l of lobes) {
        const dt = Math.atan2(Math.sin(t - l.a), Math.cos(t - l.a));
        rad += 44 * l.h * Math.exp(-((dt / l.w) ** 2));
      }
      rad += 0.8 * Math.sin(t * 9 + seed);
      const x = 50 + rad * Math.cos(t);
      const y = 50 + rad * Math.sin(t);
      d += `${i ? 'L' : 'M'}${x.toFixed(2)} ${y.toFixed(2)}`;
    }
    return d + 'Z';
  });

  const crack = $derived.by(() => {
    const r = prng(seed * 104729 + 5);
    let d = '';
    const n = 9;
    for (let i = 0; i <= n; i++) {
      const x = 4 + (92 * i) / n;
      const y = 61 + (r() - 0.5) * 9 + 3 * Math.sin(i * 1.7 + seed);
      d += `${i ? 'L' : 'M'}${x.toFixed(2)} ${y.toFixed(2)}`;
    }
    return d;
  });

  const rosette = $derived.by(() => {
    let d = '';
    const k = 8;
    for (let j = 0; j <= 160; j++) {
      const t = (j / 160) * Math.PI * 2;
      const rr = 13 + 7 * Math.cos(k * t);
      d += `${j ? 'L' : 'M'}${(50 + rr * Math.cos(t)).toFixed(2)} ${(50 + rr * Math.sin(t)).toFixed(2)}`;
    }
    return d + 'Z';
  });
</script>

<svg class="wax" width={size} height={size} viewBox="0 0 100 100" aria-hidden="true">
  <defs>
    <radialGradient id="{uid}-g" cx="38%" cy="32%" r="72%">
      <stop offset="0" style="stop-color: var(--wax-hi)" />
      <stop offset="0.38" style="stop-color: var(--wax)" />
      <stop offset="1" style="stop-color: var(--wax-lo)" />
    </radialGradient>
    <radialGradient id="{uid}-s" cx="34%" cy="28%" r="30%">
      <stop offset="0" stop-color="#fff" stop-opacity="0.55" />
      <stop offset="1" stop-color="#fff" stop-opacity="0" />
    </radialGradient>
  </defs>
  <path d={blob} transform="translate(2.5 4)" fill="rgb(0 0 0 / 0.28)" />
  <path d={blob} fill="url(#{uid}-g)" />
  <circle cx="50" cy="50" r="29" fill="none" stroke="rgb(0 0 0 / 0.28)" stroke-width="2.6" transform="translate(-0.8 -1)" />
  <circle cx="50" cy="50" r="29" fill="none" stroke="rgb(255 255 255 / 0.32)" stroke-width="1.6" transform="translate(0.9 1.1)" />
  <path d={rosette} fill="none" stroke="rgb(0 0 0 / 0.3)" stroke-width="2" transform="translate(-0.6 -0.7)" />
  <path d={rosette} fill="none" stroke="rgb(255 255 255 / 0.3)" stroke-width="1.4" transform="translate(0.6 0.7)" />
  <path d={blob} fill="url(#{uid}-s)" />
  {#if broken}
    <path d={crack} fill="none" stroke="rgb(0 0 0 / 0.55)" stroke-width="3.2" stroke-linejoin="round" />
    <path d={crack} fill="none" stroke="rgb(255 255 255 / 0.28)" stroke-width="1.2" stroke-linejoin="round" transform="translate(0 2.2)" />
  {/if}
</svg>

<style>
  .wax {
    display: block;
    overflow: visible;
  }
</style>
