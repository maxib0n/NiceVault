<!--
  The active volume opened OLDER than this computer last saw it (VaultInfo.rolled_back):
  the file was replaced by an earlier copy of itself. Stays until "Tieni questa versione"
  (vault_accept_rollback) or "Non ora" (this session only: it comes back at the next unlock).
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { History } from '@lucide/svelte';
  import * as api from '../lib/api';
  import { vaults } from '../lib/stores/vaults.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';

  let later = $state(new Set<string>());
  let busy = $state(false);
  const v = $derived(vaults.active?.rolled_back && !later.has(vaults.active.id) ? vaults.active : null);

  async function keep() {
    if (!v || busy) return;
    busy = true;
    try {
      const info = await api.vaultAcceptRollback(v.id);
      const i = vaults.open.findIndex((o) => o.id === info.id);
      if (i >= 0) vaults.open[i] = info;
    } catch (e) {
      toasts.error(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if v}
  <div class="rollback" role="alertdialog" aria-labelledby="rollback-title" aria-describedby="rollback-body" data-testid="rollback-notice">
    <span class="glyph" aria-hidden="true"><History size={18} /></span>
    <div class="text">
      <strong id="rollback-title">{m.rollback_title()}</strong>
      <p id="rollback-body">{m.rollback_body({ name: v.name })}</p>
      <p class="hint">{m.rollback_hint()}</p>
      <div class="actions">
        <button class="btn" type="button" onclick={() => (later = new Set([...later, v!.id]))}>{m.rollback_later()}</button>
        <button class="btn primary" type="button" disabled={busy} onclick={keep}>{m.rollback_keep()}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .rollback {
    position: fixed;
    top: var(--space-4);
    left: 50%;
    transform: translateX(-50%);
    z-index: var(--z-toast);
    display: flex;
    gap: 12px;
    width: min(520px, calc(100vw - 32px));
    padding: 14px 16px;
    border-radius: var(--radius-md);
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 3px 0 0 var(--danger, var(--ink-4)),
      var(--shadow-pop);
  }
  .glyph {
    color: var(--danger, var(--ink));
    padding-top: 2px;
  }
  .text {
    min-width: 0;
  }
  strong {
    display: block;
    font-size: var(--text-md);
  }
  p {
    margin: 6px 0 0;
    font-size: var(--text-sm, 13px);
    line-height: 1.45;
  }
  .hint {
    color: var(--ink-3, var(--ink));
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }
</style>
