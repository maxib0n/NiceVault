<!--
  "Non c'è abbastanza spazio: servono X, liberi Y" before an import (stores/space.svelte.ts).
  A fixed-size volume offers Estendi il volume… (ResizeDialog, prefilled to a size that fits);
  a growable one on a full drive only says so. Mounted once, in App.
-->
<script lang="ts">
  import { formatBytes } from '../lib/format';
  import { space } from '../lib/stores/space.svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';
  import ResizeDialog from './ResizeDialog.svelte';

  const ask = $derived(space.ask);
  let resizing = $state<{ vault: string; suggest: number } | null>(null);
  const close = () => (space.ask = null);
  const params = $derived(ask ? { need: formatBytes(ask.need), free: formatBytes(ask.free) } : { need: '', free: '' });
</script>

<Dialog open={!!ask} title={m.space_title()} onclose={close}>
  <p data-testid="space-body">{ask?.fixed ? m.space_body_fixed(params) : m.space_body_growable(params)}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={close} data-testid="space-cancel">{ask?.fixed ? m.space_cancel() : m.space_ok()}</button>
    {#if ask?.fixed && ask.suggest !== null}
      <button
        type="button"
        class="btn primary"
        data-testid="space-extend"
        onclick={() => {
          resizing = { vault: ask.vault, suggest: ask.suggest! };
          close();
        }}>{m.space_extend()}</button>
    {/if}
  {/snippet}
</Dialog>

{#if resizing}
  <ResizeDialog open={true} vault={resizing.vault} suggest={resizing.suggest} onclose={() => (resizing = null)} />
{/if}
