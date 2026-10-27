<script lang="ts">
  import Rosette from '../ornament/Rosette.svelte';
  import Stitch from '../ornament/Stitch.svelte';

  let { size = 40 }: { size?: number } = $props();
</script>

<span class="ink-chip cloth" style:--chip="{size}px" aria-hidden="true">
  <Stitch inset={3.5} len={2.4} gap={1.7} width={0.9} slant={0.25} thread="var(--stitch-cloth)" />
  <Rosette size={Math.round(size * 0.62)} kind="swatch" finish="foil" />
</span>

<style>
  .ink-chip {
    display: grid;
    place-items: center;
    flex: none;
    width: var(--chip);
    height: var(--chip);
    border-radius: calc(var(--chip) * 0.24);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.14),
      inset 0 -1px 0 rgb(0 0 0 / 0.3),
      0 1px 1px rgb(var(--cast) / 0.3),
      0 3px 6px -3px rgb(var(--cast) / 0.5);
  }
</style>
