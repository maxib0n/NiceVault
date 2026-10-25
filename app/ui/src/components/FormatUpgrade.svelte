<!--
  "Aggiorna formato" (Dettagli > Volume): shows the active volume's format and offers the opt-in
  upgrade to the single-file format (api.vaultUpgradeFormat).
  Two steps: the button shows what changes (older builds can no longer open it), then confirm.
  Set like the other rows of the card: a dashed rule over it, the state on the left (a small
  seal: one file or file + journal), the key on the right; the warning is a warm callout.
-->
<script lang="ts">
  import { FileCheck2, Files } from '@lucide/svelte';
  import * as api from '../lib/api';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import { m } from '../paraglide/messages.js';

  const active = $derived(vaults.active);
  let asking = $state(false);
  let busy = $state(false);

  async function upgrade() {
    const v = active;
    if (!v || busy) return;
    busy = true;
    try {
      const info = await api.vaultUpgradeFormat(v.id);
      const i = vaults.open.findIndex((o) => o.id === info.id);
      if (i >= 0) vaults.open[i] = info;
      asking = false;
      toasts.push(m.fmt_done({ name: info.name }), 'success');
    } catch (e) {
      toasts.error(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if active}
  <div class="fmt" data-testid="format-upgrade">
    <span class="fmt-mark" data-single={!!active.single_file} aria-hidden="true">{#if active.single_file}<FileCheck2 size={15} strokeWidth={1.6} />{:else}<Files size={15} strokeWidth={1.6} />{/if}</span>
    <h3 class="sub-title caps">{m.fmt_title()}</h3>
    {#if active.single_file}
      <p class="fmt-note" data-testid="format-single">{m.fmt_single()}</p>
    {:else if active.can_upgrade_format}
      <p class="fmt-note">{m.fmt_old()}</p>
      {#if asking}
        <p class="fmt-warn" role="alert">{m.fmt_warn()}</p>
        <div class="fmt-actions">
          <button type="button" class="btn sm" disabled={busy} onclick={() => (asking = false)}>{m.action_cancel()}</button>
          <button type="button" class="btn sm primary" disabled={busy} onclick={upgrade} data-testid="format-confirm">
            {busy ? m.fmt_busy() : m.fmt_confirm()}
          </button>
        </div>
      {:else}
        <div class="fmt-actions">
          <button type="button" class="btn sm" onclick={() => (asking = true)} data-testid="format-upgrade-open">{m.fmt_upgrade()}</button>
        </div>
      {/if}
    {:else}
      <p class="fmt-note">{m.fmt_fixed()}</p>
    {/if}
  </div>
{/if}

<style>
  .fmt {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    column-gap: var(--space-3);
    row-gap: 4px;
    align-items: center;
    margin-top: var(--space-4);
    padding-top: var(--space-3);
    border-top: 1px dashed var(--paper-rule-2);
  }
  /* a small engraved seal: one file (ink) or a file with its journal beside it (quiet) */
  .fmt-mark {
    grid-row: span 2;
    align-self: start;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    color: var(--ink-3);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .fmt-mark[data-single='true'] {
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  .sub-title {
    grid-column: 2;
    margin: 0;
    color: var(--ink-3);
  }
  .fmt-note {
    grid-column: 2;
    margin: 0;
    max-width: 46ch;
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .fmt-actions {
    grid-column: 3;
    grid-row: 1 / span 2;
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }
  /* asking: the warning spans the row, the keys sit under it on the right */
  .fmt-warn {
    grid-column: 2 / -1;
    margin: var(--space-2) 0 0;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--danger-wash);
    color: var(--ink);
    font-size: var(--text-sm);
    font-weight: 500;
  }
  .fmt-warn + .fmt-actions {
    grid-column: 2 / -1;
    grid-row: auto;
    margin-top: var(--space-2);
  }
</style>
