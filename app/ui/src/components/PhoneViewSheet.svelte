<script lang="ts">
  import { LayoutGrid, List } from '@lucide/svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';
  import { radioKeys } from '../lib/radio';
  import { prefs, type ViewMode } from '../lib/stores/prefs.svelte';
  import { DENSITIES, GRID_SIZES, viewSize } from '../lib/stores/viewsize.svelte';
  import type { GridSize, ListDensity, VaultId } from '../lib/types';

  let { open, vault, results = false, onclose }: { open: boolean; vault: VaultId; results?: boolean; onclose: () => void } = $props();

  const layout = $derived<ViewMode>(results ? 'list' : prefs.view);
  const grid = $derived(viewSize.grid(vault));
  const density = $derived(viewSize.density(vault));
  const SIZE_LABEL: Record<GridSize, () => string> = { s: m.size_s, m: m.size_m, l: m.size_l, xl: m.size_xl };
  const DENSITY_LABEL: Record<ListDensity, () => string> = { compact: m.density_compact, normal: m.density_normal, comfortable: m.density_comfortable };
  const MODES: ViewMode[] = ['grid', 'list'];
</script>

<Dialog {open} title={m.tb_view()} {onclose}>
  <div class="vs" data-testid="phone-view-sheet">
    <div class="segmented modes" role="radiogroup" aria-label={m.tb_view()} tabindex="-1" onkeydown={(e) => !results && radioKeys(e, MODES, layout, (v) => prefs.setView(v))}>
      {#each MODES as v (v)}
        <button type="button" role="radio" aria-checked={layout === v} tabindex={layout === v ? 0 : -1} disabled={results && v === 'grid'} onclick={() => prefs.setView(v)}>
          {#if v === 'grid'}<LayoutGrid size={16} aria-hidden="true" />{m.tb_grid()}{:else}<List size={16} aria-hidden="true" />{m.tb_list()}{/if}
        </button>
      {/each}
    </div>
    {#if layout === 'grid'}
      <p class="vs-label caps" id="vs-size">{m.tb_size_grid()}</p>
      <div class="segmented" role="radiogroup" aria-labelledby="vs-size" tabindex="-1" onkeydown={(e) => radioKeys(e, GRID_SIZES, grid, (g) => viewSize.setGrid(vault, g))}>
        {#each GRID_SIZES as g (g)}
          <button type="button" role="radio" aria-checked={grid === g} tabindex={grid === g ? 0 : -1} onclick={() => viewSize.setGrid(vault, g)}>{SIZE_LABEL[g]()}</button>
        {/each}
      </div>
    {:else}
      <p class="vs-label caps" id="vs-size">{m.tb_density()}</p>
      <div class="segmented" role="radiogroup" aria-labelledby="vs-size" tabindex="-1" onkeydown={(e) => radioKeys(e, DENSITIES, density, (d) => viewSize.setDensity(vault, d))}>
        {#each DENSITIES as d (d)}
          <button type="button" role="radio" aria-checked={density === d} tabindex={density === d ? 0 : -1} onclick={() => viewSize.setDensity(vault, d)}>{DENSITY_LABEL[d]()}</button>
        {/each}
      </div>
    {/if}
  </div>
  {#snippet actions()}
    <button type="button" class="btn primary" onclick={onclose}>{m.pick_close()}</button>
  {/snippet}
</Dialog>

<style>
  .vs {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .vs .segmented {
    display: flex;
    width: 100%;
  }
  .vs .segmented > :global(button) {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 44px;
    line-height: 1;
  }
  .vs .segmented > :global(button) :global(svg) {
    flex: none;
    margin: 0;
  }
  .vs-label {
    margin: 6px 0 0;
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-3);
  }
</style>
