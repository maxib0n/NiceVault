<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { skeleton } from '../lib/reveal';

  let { url }: { url: string } = $props();
  const LIMIT = 256 * 1024;
  let text = $state<string | null>(null);
  let truncated = $state(false);
  let error = $state(false);

  $effect(() => {
    const u = url;
    text = null;
    error = false;
    const ctl = new AbortController();
    fetch(u, { signal: ctl.signal })
      .then((r) => (r.ok ? r.blob() : Promise.reject(new Error(String(r.status)))))
      .then(async (b) => {
        truncated = b.size > LIMIT;
        text = await b.slice(0, LIMIT).text();
      })
      .catch(() => {
        if (!ctl.signal.aborted) error = true;
      });
    return () => ctl.abort();
  });
</script>

<div class="text-view">
  {#if error}
    <p class="hint state">{m.text_error()}</p>
  {:else if text === null}
    <div class="sheet loading nv-enter" data-state="loading">
      <span class="visually-hidden">{m.fv_reading()}</span>
      <span class="skeleton" aria-hidden="true" use:skeleton={{ lines: 9, seed: 5, widths: ['64%', '92%', '100%', '86%', '40%', '96%', '100%', '78%', '52%'] }}></span>
    </div>
  {:else}
    <div class="sheet">
      <pre>{text}</pre>
      {#if truncated}<p class="hint more">{m.text_truncated()}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .text-view {
    width: 100%;
    height: 100%;
    overflow: auto;
    background: transparent;
    color: var(--ink);
    padding: var(--space-5) var(--space-4);
    user-select: text;
    -webkit-user-select: text;
  }
  .sheet {
    max-width: 820px;
    min-height: 100%;
    margin: 0 auto;
    padding: var(--space-6) var(--space-6) var(--space-7);
    border-radius: 3px;
    background: var(--paper-hi);
    box-shadow:
      0 0 0 1px rgb(var(--cast) / 0.08),
      0 2px 4px rgb(var(--cast) / 0.08),
      0 18px 36px -16px rgb(var(--cast) / 0.36);
  }
  pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-md);
    line-height: 1.65;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .more {
    margin-top: var(--space-5);
  }
  .state {
    padding-top: var(--space-6);
    text-align: center;
  }
  .loading {
    animation: nv-enter 1ms linear 300ms backwards;
  }
  .skeleton {
    display: block;
    font-size: var(--text-md);
    --ascii-skeleton: var(--ink-4);
  }
</style>
