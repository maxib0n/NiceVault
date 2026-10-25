<!--
  "Mancano i programmi per …" on a finished device (made offline, or a file failed) with
  "Scarica ora": device_fetch_missing downloads just those programs (release packs opened and
  signatures verified by the backend) and adds them to the stick. The outcome is bound back.
-->
<script lang="ts">
  import { m } from '../../paraglide/messages.js';
  import * as dev from '../../lib/device';
  import type { DeviceOutcome } from '../../lib/device';
  import { errorText } from '../../lib/errors';

  let { job, outcome = $bindable(), list }: { job: string | null; outcome: DeviceOutcome; list: string } = $props();

  let busy = $state(false);
  let error = $state<string | null>(null);
  let added = $state(false);

  async function fetchNow() {
    if (busy || !job) return;
    busy = true;
    error = null;
    try {
      outcome = await dev.deviceFetchMissing(job);
      added = outcome.missing.length === 0;
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if outcome.missing.length}
  <p class="error-text" data-testid="device-missing">{m.dev_missing({ list })}</p>
  {#if job}
    <button type="button" class="btn sm" disabled={busy} aria-busy={busy} onclick={fetchNow} data-testid="device-fetch-missing">
      {busy ? m.dev_fetch_missing_busy() : m.dev_fetch_missing()}
    </button>
  {/if}
  {#if error}<p class="error-text" role="alert" data-testid="device-fetch-error">{error}</p>{/if}
{:else if added}
  <p class="hint" role="status" data-testid="device-fetch-done">{m.dev_fetch_missing_done()}</p>
{/if}
