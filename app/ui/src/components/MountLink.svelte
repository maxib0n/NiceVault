<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { openLink } from '../viewers/actions';

  let { url, on = 'cloth' }: { url: string; on?: 'cloth' | 'paper' } = $props();
  let asking = $state(false);
  let cancelBtn = $state<HTMLButtonElement | null>(null);
  const site = $derived.by(() => {
    try {
      const u = new URL(url);
      return u.host + (u.pathname === '/' ? '' : u.pathname.replace(/\/$/, ''));
    } catch {
      return url;
    }
  });

  function ask() {
    asking = true;
    queueMicrotask(() => cancelBtn?.focus());
  }
  async function go() {
    asking = false;
    await openLink(url);
  }
</script>

<div class="mount-link" data-on={on} data-testid="mount-link">
  {#if asking}
    <p class="q">{m.mount_link_confirm({ site })}</p>
    <div class="acts">
      <button type="button" class="btn ghost sm" bind:this={cancelBtn} onclick={() => (asking = false)}>{m.mount_link_cancel()}</button>
      <button type="button" class="btn sm" onclick={go}>{m.mount_link_open()}</button>
    </div>
  {:else}
    <button type="button" class="btn ghost sm" onclick={ask}>{m.mount_get_winfsp()}</button>
  {/if}
</div>

<style>
  .mount-link {
    margin-top: 6px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .q {
    margin: 0;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }
  .acts {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
</style>
