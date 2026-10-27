<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import * as api from '../lib/api';

  const URL = 'https://github.com/maxib0n';

  function open() {
    if (api.isTauri) void api.openLink(URL).catch(() => undefined);
    else window.open(URL, '_blank', 'noopener');
  }
</script>

<button type="button" class="maker" onclick={open} title={m.maker_title()} aria-label={m.maker_title()} data-testid="maker">
  <svg viewBox="0 0 32 32" width="28" height="28" shape-rendering="crispEdges" fill="currentColor" aria-hidden="true"><rect x="9" y="3" width="4" height="1" /><rect x="19" y="3" width="4" height="1" /><rect x="9" y="4" width="4" height="1" /><rect x="19" y="4" width="4" height="1" /><rect x="7" y="5" width="8" height="1" /><rect x="17" y="5" width="8" height="1" /><rect x="7" y="6" width="8" height="1" /><rect x="17" y="6" width="8" height="1" /><rect x="7" y="7" width="8" height="1" /><rect x="17" y="7" width="8" height="1" /><rect x="7" y="8" width="8" height="1" /><rect x="17" y="8" width="8" height="1" /><rect x="7" y="9" width="8" height="1" /><rect x="17" y="9" width="8" height="1" /><rect x="7" y="10" width="8" height="1" /><rect x="17" y="10" width="8" height="1" /><rect x="7" y="11" width="8" height="1" /><rect x="17" y="11" width="8" height="1" /><rect x="7" y="12" width="8" height="1" /><rect x="17" y="12" width="8" height="1" /><rect x="7" y="13" width="8" height="1" /><rect x="17" y="13" width="8" height="1" /><rect x="7" y="14" width="8" height="1" /><rect x="17" y="14" width="8" height="1" /><rect x="7" y="15" width="18" height="1" /><rect x="7" y="16" width="18" height="1" /><rect x="5" y="17" width="22" height="1" /><rect x="5" y="18" width="22" height="1" /><rect x="3" y="19" width="8" height="1" /><rect x="13" y="19" width="6" height="1" /><rect x="21" y="19" width="8" height="1" /><rect x="3" y="20" width="8" height="1" /><rect x="13" y="20" width="6" height="1" /><rect x="21" y="20" width="8" height="1" /><rect x="3" y="21" width="8" height="1" /><rect x="13" y="21" width="6" height="1" /><rect x="21" y="21" width="8" height="1" /><rect x="3" y="22" width="8" height="1" /><rect x="13" y="22" width="6" height="1" /><rect x="21" y="22" width="8" height="1" /><rect x="3" y="23" width="12" height="1" /><rect x="17" y="23" width="12" height="1" /><rect x="3" y="24" width="12" height="1" /><rect x="17" y="24" width="12" height="1" /><rect x="5" y="25" width="22" height="1" /><rect x="5" y="26" width="22" height="1" /><rect x="7" y="27" width="18" height="1" /><rect x="7" y="28" width="18" height="1" /></svg>
</button>

<style>
  .maker {
    position: fixed;
    right: calc(12px + env(safe-area-inset-right, 0px));
    bottom: calc(10px + env(safe-area-inset-bottom, 0px));
    z-index: 2;
    display: grid;
    place-items: center;
    padding: 6px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--ink);
    opacity: 0.5;
    cursor: pointer;
    transition: opacity var(--dur-ui) var(--ease-out);
  }
  .maker:hover,
  .maker:focus-visible {
    opacity: 1;
  }
</style>
