<!--
  "Working": an engraved rosette whose two lathes turn against each other, slowly (the same
  plates as the unlock's rosette, ornament/Rosette.svelte). Transform-only animation on three
  pre-drawn canvases: the compositor turns them, nothing is redrawn. Reduced motion: still.
-->
<script lang="ts">
  import Rosette from '../../ornament/Rosette.svelte';

  let { size = 18, kind = 'mark' }: { size?: number; kind?: 'mark' | 'seal' | 'stamp' } = $props();
</script>

<span class="lathes" data-kind={kind} aria-hidden="true"><Rosette {size} {kind} /></span>

<style>
  .lathes {
    display: inline-grid;
    flex: none;
    color: currentColor;
  }
  .lathes :global(.lathe-a) {
    animation: lathe-a 7.2s linear infinite;
  }
  .lathes :global(.lathe-b) {
    animation: lathe-b 4.8s linear infinite;
  }
  /* the small mark has only one lathe: it turns a little faster so it reads as moving */
  .lathes[data-kind='mark'] :global(.lathe-a) {
    animation-duration: 2.4s;
  }
  @keyframes lathe-a {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes lathe-b {
    to {
      transform: rotate(-360deg);
    }
  }
</style>
