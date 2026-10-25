<!--
  The drive about to be erased holds NiceVault volumes (Plan.volumes, a read-only scan of its
  mounted partitions): name them, say they go for good, and ask for one more explicit "I know".
-->
<script lang="ts">
  import { TriangleAlert } from '@lucide/svelte';
  import { m } from '../../paraglide/messages.js';
  import type { FoundVolume } from '../../lib/device';

  let { volumes, ack = $bindable(false) }: { volumes: FoundVolume[]; ack?: boolean } = $props();
  const list = $derived(volumes.map((v) => v.name).join(', '));
</script>

<section class="volumes" role="alert" aria-labelledby="dev-vol-title" data-testid="device-volumes">
  <p id="dev-vol-title" class="title"><TriangleAlert size={18} aria-hidden="true" /> {m.dev_volumes_title()}</p>
  <ul class="names">
    {#each volumes as v (v.path)}
      <li><span class="name mono">{v.name}</span>{#if v.open}<span class="badge" data-tone="danger">{m.dev_volumes_open()}</span>{/if}<span class="path mono">{v.path}</span></li>
    {/each}
  </ul>
  <p class="body">{m.dev_volumes_body({ list })}</p>
  <label class="ack">
    <input type="checkbox" bind:checked={ack} data-testid="device-volumes-ack" />
    <span>{m.dev_volumes_ack()}</span>
  </label>
</section>

<style>
  .volumes {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: 14px 16px;
    border-radius: var(--radius-md);
    color: var(--ink);
    background: var(--danger-wash);
    box-shadow:
      inset 3px 0 0 var(--danger),
      inset 0 0 0 1px color-mix(in oklab, var(--danger) 40%, transparent);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-md);
    font-weight: 600;
  }
  .title :global(svg) {
    flex: none;
    color: var(--danger);
  }
  .names {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .names li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: var(--space-2);
    padding: 5px 10px;
    border-radius: var(--radius-sm);
    background: color-mix(in oklab, var(--paper) 72%, transparent);
  }
  .name {
    font-size: var(--text-md);
    font-weight: 600;
  }
  .path {
    font-size: var(--text-2xs);
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .body {
    font-size: var(--text-md);
  }
  .ack {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-top: var(--space-1);
    font-weight: 600;
    cursor: pointer;
  }
  .ack input {
    width: 18px;
    height: 18px;
    accent-color: var(--danger);
    flex: none;
  }
</style>
