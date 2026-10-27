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
