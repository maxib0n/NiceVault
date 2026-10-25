<!--
  Idle-time warm-up (lib/warm.ts): the printed ornaments Impostazioni opens with, engraved once
  out of sight so the page's first opening copies them back (InkCanvas cache) instead of cutting
  14 ink chips in the frame it appears. Invisible, inert, no text, no test ids; unmounted as soon
  as it has drawn.
-->
<script lang="ts">
  import { ACCENTS } from '../lib/accents';
  import InkChip from './InkChip.svelte';
</script>

<div class="warm-up sheet" aria-hidden="true" inert>
  {#each ACCENTS as a (a.id)}
    <span class="swatch" data-accent={a.id}><InkChip size={38} /></span>
  {/each}
</div>

<style>
  .warm-up {
    position: fixed;
    left: 0;
    top: 0;
    display: flex;
    gap: 4px;
    visibility: hidden;
    pointer-events: none;
    z-index: -1;
    contain: layout paint;
  }
  .swatch {
    display: inline-grid;
  }
</style>
