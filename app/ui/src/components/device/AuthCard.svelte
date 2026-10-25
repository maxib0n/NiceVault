<!--
  While the privileged steps run (unmount, partition table, formatting) the system may be asking
  for the administrator password in its own window (polkit / UAC / macOS): say so plainly, with
  the progress still on screen next to it.
-->
<script lang="ts">
  import { KeyRound } from '@lucide/svelte';
  import { m } from '../../paraglide/messages.js';
  import Lathes from './Lathes.svelte';
</script>

<aside class="auth" data-testid="device-auth" aria-live="polite">
  <span class="seal"><Lathes size={64} kind="seal" /><span class="key"><KeyRound size={18} strokeWidth={1.5} aria-hidden="true" /></span></span>
  <div class="words">
    <p class="title display">{m.dev_auth_title()}</p>
    <p class="body">{m.dev_auth_body()}</p>
  </div>
</aside>

<style>
  .auth {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 14px 18px;
    border-radius: var(--radius-md);
    background: var(--accent-soft);
    box-shadow:
      inset 0 0 0 1px var(--accent-line),
      inset 0 1px 2px var(--field-shade);
  }
  .seal {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
    color: var(--accent);
  }
  .seal > :global(*) {
    grid-area: 1 / 1;
  }
  .key {
    display: grid;
  }
  .title {
    font-size: 22px;
    line-height: 1.2;
    color: var(--title-ink);
  }
  .body {
    margin-top: 4px;
    font-size: var(--text-md);
    color: var(--ink-2);
  }
</style>
