<!-- The maker's mark: a small rosette in the volume's ink, "Nice" in the grotesk, "Vault" in Bodoni italic. -->
<script lang="ts">
  import Rosette from '../ornament/Rosette.svelte';

  let { size = 22 }: { size?: number } = $props();
</script>

<span class="brand" aria-label="NiceVault" role="img" style:--mark="{size}px">
  <Rosette {size} kind="mark" />
  <span class="word" aria-hidden="true">Nice<i>Vault</i></span>
</span>

<style>
  /* one lockup: both words on one baseline, cap heights matched (Plex Sans caps = 0.71 em,
     Bodoni Moda italic caps = 0.75 em, so 15 px and 14.2 px), the mark centred on the cap band */
  .brand {
    display: inline-flex;
    align-items: baseline;
    gap: 9px;
    color: var(--ink);
    line-height: 1;
  }
  .brand > :global(.rosette) {
    align-self: baseline;
    /* baseline-aligned, its bottom edge sits on the baseline: lower it by half its overhang
       of the cap band so its centre is the caps' centre */
    position: relative;
    top: calc((var(--mark) - 10.65px) / 2);
  }
  .word {
    font-family: var(--font-ui);
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.005em;
  }
  .word i {
    margin-left: 1.5px;
    font-family: var(--font-display);
    font-size: 14.2px;
    font-weight: 500;
    letter-spacing: 0.005em;
    color: var(--accent);
  }
</style>
