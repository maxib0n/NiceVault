<script lang="ts">
  import { formatBytes } from '../../lib/format';
  import { OS_NAMES, type Stick, type StickSpace } from '../../lib/stick';
  import type { StickRun } from '../../lib/stores/stick.svelte';
  import { m } from '../../paraglide/messages.js';

  let { stick, run }: { stick: Stick; run: StickRun | null } = $props();

  const space = $derived<StickSpace | null>(run?.space ?? stick.space ?? null);
  const largest = $derived(space ? [...space.programs].sort((a, b) => b.next - a.next)[0] : undefined);
  const nofit = $derived(space?.programs.filter((p) => p.mode === null) ?? []);
</script>

{#if space && (space.tight || !space.fits)}
  <div class="space" data-testid="stick-space" data-tight={space.tight} data-fits={space.fits}>
    {#if space.tight}<p class="lead">{m.stk_space_tight()}</p>{/if}
    <p class="nums">
      {m.stk_space_nums({ size: space.size ? formatBytes(space.size) : '?', free: space.free !== null ? formatBytes(space.free) : '?' })}{#if space.reclaim > 0}
        · {m.stk_space_reclaim({ n: formatBytes(space.reclaim) })}{/if}{#if largest}
        · {m.stk_space_largest({ os: OS_NAMES[largest.os], n: formatBytes(largest.next) })}{/if}
    </p>
    {#if space.tight}<p class="how">{m.stk_space_how()}</p>{/if}
    {#each nofit as p (p.os)}
      <p class="nofit">{m.stk_space_nofit({ os: OS_NAMES[p.os], need: formatBytes(p.next) })}</p>
    {/each}
  </div>
{/if}

<style>
  .space {
    grid-column: 2 / -1;
    margin-top: var(--space-2);
    padding: 8px 10px;
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    font-size: var(--text-xs);
    color: var(--ink-2);
  }
  .space p {
    margin: 0;
  }
  .space p + p {
    margin-top: 3px;
  }
  .lead {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--ink);
  }
  .nums {
    font-variant-numeric: tabular-nums;
  }
  .how {
    color: var(--ink-3);
  }
  .nofit {
    color: var(--danger);
  }
</style>
