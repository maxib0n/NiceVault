<script lang="ts">
  let { size = 112 }: { size?: number } = $props();
  const uid = `pwl${Math.random().toString(36).slice(2, 8)}`;
</script>

<svg class="pw-ledger" width={size} height={size * 1.18} viewBox="0 0 120 142" aria-hidden="true" focusable="false">
  <defs>
    <linearGradient id="{uid}-c" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="color-mix(in oklab, var(--accent-deep) 82%, white)" />
      <stop offset="1" stop-color="color-mix(in oklab, var(--accent-deep) 78%, black)" />
    </linearGradient>
    <linearGradient id="{uid}-f" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="color-mix(in oklab, var(--foil) 70%, white)" />
      <stop offset="0.55" stop-color="var(--foil)" />
      <stop offset="1" stop-color="color-mix(in oklab, var(--foil) 60%, black)" />
    </linearGradient>
    <linearGradient id="{uid}-s" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="rgb(0 0 0 / 0.38)" />
      <stop offset="0.6" stop-color="rgb(0 0 0 / 0.06)" />
      <stop offset="1" stop-color="rgb(255 255 255 / 0.1)" />
    </linearGradient>
    <linearGradient id="{uid}-r" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="color-mix(in oklab, var(--foil) 55%, black)" />
      <stop offset="0.5" stop-color="var(--foil)" />
      <stop offset="1" stop-color="color-mix(in oklab, var(--foil) 60%, black)" />
    </linearGradient>
  </defs>
  <ellipse cx="62" cy="131" rx="46" ry="5" fill="rgb(0 0 0 / 0.38)" />
  <rect x="20" y="12" width="90" height="116" rx="3" fill="color-mix(in oklab, var(--paper-hi, #eee) 80%, var(--accent-deep))" opacity="0.9" />
  <path d="M20 125.5h88M22 127h84" stroke="rgb(0 0 0 / 0.16)" stroke-width="0.6" fill="none" />
  <path d="M108 14v112M105.5 14v112M103 14v112" stroke="rgb(0 0 0 / 0.16)" stroke-width="0.6" fill="none" />
  <path d="M84 100V138l5.5-4.5 5.5 4.5V100z" fill="url(#{uid}-r)" />
  <path d="M84 120v18l5.5-4.5 5.5 4.5v-18" fill="none" stroke="rgb(0 0 0 / 0.35)" stroke-width="0.7" />
  <path d="M84 123h11" stroke="rgb(0 0 0 / 0.28)" stroke-width="1.6" />
  <rect x="12" y="6" width="94" height="116" rx="5" fill="url(#{uid}-c)" />
  <rect x="12" y="6" width="94" height="116" rx="5" fill="none" stroke="rgb(0 0 0 / 0.45)" stroke-width="1" />
  <rect x="12.6" y="6.6" width="92.8" height="114.8" rx="4.5" fill="none" stroke="rgb(255 255 255 / 0.22)" stroke-width="0.8" />
  <path d="M12 11a5 5 0 0 1 5-5h9v116h-9a5 5 0 0 1-5-5z" fill="url(#{uid}-s)" />
  <path d="M26 6v116" stroke="rgb(0 0 0 / 0.35)" stroke-width="1" />
  <path d="M27 6v116" stroke="rgb(255 255 255 / 0.14)" stroke-width="0.7" />
  {#each [24, 42, 78, 96] as y (y)}
    <path d="M12.5 {y}h13.5" stroke="url(#{uid}-f)" stroke-width="1.4" stroke-linecap="round" opacity="0.85" />
  {/each}
  <rect x="33" y="14" width="66" height="100" rx="3" fill="none" stroke="var(--foil)" stroke-width="0.9" stroke-dasharray="3 2.6" opacity="0.6" />
  <g transform="translate(66 54)">
    <circle r="24" fill="rgb(0 0 0 / 0.28)" transform="translate(0 1.5)" />
    <circle r="24" fill="url(#{uid}-f)" />
    <circle r="24" fill="none" stroke="rgb(0 0 0 / 0.35)" stroke-width="0.8" />
    <circle r="21.2" fill="none" stroke="rgb(0 0 0 / 0.28)" stroke-width="0.7" />
    <circle r="19" fill="none" stroke="rgb(255 255 255 / 0.4)" stroke-width="0.5" stroke-dasharray="1.2 1.8" />
    <circle r="15.5" fill="color-mix(in oklab, var(--accent-deep) 55%, black)" />
    <circle r="15.5" fill="none" stroke="rgb(255 255 255 / 0.3)" stroke-width="0.6" />
    <path d="M0-8.2a4.3 4.3 0 0 1 2.2 8 L4 9.5h-8L-2.2-.2A4.3 4.3 0 0 1 0-8.2z" fill="url(#{uid}-f)" />
  </g>
  <path d="M46 96h40M52 102h28" stroke="var(--foil)" stroke-width="1.3" stroke-linecap="round" opacity="0.55" />
</svg>

<style>
  .pw-ledger {
    display: block;
    overflow: visible;
    filter: drop-shadow(0 6px 8px rgb(var(--cast, 0 0 0) / 0.35));
  }
</style>
