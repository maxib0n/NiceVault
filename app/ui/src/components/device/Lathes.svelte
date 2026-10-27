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
