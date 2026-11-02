<script lang="ts">
  import * as api from '../lib/api';
  import { bioLabel, bioReason, bioTitle } from '../lib/biometric';
  import { errorCode } from '../lib/errors';
  import { toasts } from '../lib/stores/toasts.svelte';
  import type { BioStatus, SlotInfo, VaultInfo } from '../lib/types';
  import { m } from '../paraglide/messages.js';

  let { vault, slots, onslots }: { vault: VaultInfo; slots: SlotInfo[]; onslots: (s: SlotInfo[]) => void } = $props();

  let status = $state<BioStatus | null>(null);
  let busy = $state(false);

  $effect(() => {
    void slots;
    const path = vault.path;
    api.bioStatus(path).then(
      (s) => vault.path === path && (status = s),
      () => (status = null),
    );
  });

  async function toggle() {
    if (!status || busy) return;
    busy = true;
    try {
      status = status.enabled ? await api.bioDisable(vault.id) : await api.bioEnable(vault.id, bioLabel(status));
      toasts.push(status.enabled ? m.bio_on() : m.bio_off(), 'success');
      onslots(await api.slotsList(vault.id));
    } catch (e) {
      if (errorCode(e) !== 'cancelled') toasts.error(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if status}
  <div class="bio" data-testid="bio" data-enabled={status.enabled} data-stale={status.stale}>
    <div class="switch-row">
      <span class="switch-label" id="bio-label">{bioTitle(status)}</span>
      <button
        type="button"
        class="switch"
        role="switch"
        aria-checked={status.enabled}
        aria-labelledby="bio-label"
        aria-describedby="bio-hint"
        disabled={busy || (!status.available && !status.enabled)}
        data-testid="bio-switch"
        onclick={toggle}
      >
        <span class="knob"></span>
      </button>
    </div>
    <p class="hint" id="bio-hint">{status.stale ? m.bio_stale() : status.available || status.enabled ? m.bio_hint() : bioReason(status.reason)}</p>
  </div>
{/if}

<style>
  .bio {
    margin-top: var(--space-3);
  }
</style>
