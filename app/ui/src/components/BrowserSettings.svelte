<script lang="ts">
  import { onMount } from 'svelte';
  import { browserInfo, browserSet, browserUnpair, onBrowser, type BrowserInfo } from '../lib/browserLink';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { m } from '../paraglide/messages.js';

  let info = $state<BrowserInfo | null>(null);

  async function load() {
    try {
      info = await browserInfo();
    } catch (e) {
      toasts.error(e);
    }
  }

  onMount(() => {
    void load();
    let off: (() => void) | null = null;
    void onBrowser('browser-changed', () => void load()).then((u) => (off = u));
    return () => off?.();
  });

  async function set(patch: { link?: boolean; default_volume?: string }) {
    try {
      info = await browserSet(patch);
    } catch (e) {
      toasts.error(e);
    }
  }

  async function unpair(id: string) {
    try {
      info = await browserUnpair(id);
    } catch (e) {
      toasts.error(e);
    }
  }

  const when = (ms: number) => new Intl.DateTimeFormat(i18n.tag, { dateStyle: 'medium', timeStyle: 'short' }).format(ms);
  const time = (ms: number) => new Intl.DateTimeFormat(i18n.tag, { timeStyle: 'short' }).format(ms);
  const KIND: Record<string, () => string> = { fill: m.brw_kind_fill, totp: m.brw_kind_totp, copy: m.brw_kind_copy };
</script>

<div class="browser" data-testid="browser-settings">
  <p class="hint">{m.brw_hint()}</p>
  {#if info?.portable}
    <p class="hint" data-testid="browser-portable">{m.brw_portable()}</p>
  {:else if info}
    <div class="switch-row">
      <span class="switch-label" id="brw-link">{m.brw_link()}</span>
      <button type="button" class="switch" role="switch" aria-checked={info.link} aria-labelledby="brw-link" aria-describedby="brw-link-hint" onclick={() => set({ link: !info?.link })} data-testid="browser-link"><span class="knob"></span></button>
    </div>
    <p class="hint" id="brw-link-hint">
      {#if info.link && info.error}{m.brw_reg_failed({ error: info.error })}
      {:else if info.link && info.registered.length}{m.brw_registered({ list: [...new Set(info.registered.map((r) => r.browser))].join(', ') })}
      {:else}{m.brw_link_hint()}{/if}
    </p>
  {/if}
  {#if info?.sandboxed.length}
    <p class="hint warn" data-testid="browser-sandboxed">{m.brw_sandboxed({ list: info.sandboxed.join(', ') })}</p>
  {/if}

  <h3 class="sub caps">{m.brw_paired()}</h3>
  {#if !info?.paired.length}
    <p class="none">{m.brw_paired_none()}</p>
  {:else}
    <ul class="rows" data-testid="browser-paired">
      {#each info.paired as p (p.id)}
        <li>
          <span class="main"><span class="name">{p.label}</span><span class="meta">{m.brw_last_used({ when: when(p.last_used) })}</span></span>
          <button type="button" class="btn sm" onclick={() => unpair(p.id)} data-testid="browser-unpair">{m.brw_unpair()}</button>
        </li>
      {/each}
    </ul>
  {/if}

  <h3 class="sub caps" id="brw-default">{m.brw_default_volume()}</h3>
  <select class="input" aria-labelledby="brw-default" aria-describedby="brw-default-hint" value={info?.default_volume ?? ''} onchange={(e) => set({ default_volume: (e.currentTarget as HTMLSelectElement).value })} disabled={!info} data-testid="browser-default-volume">
    <option value="">{m.brw_default_none()}</option>
    {#each info?.volumes ?? [] as v (v.path)}
      <option value={v.path} title={v.path}>{v.name}</option>
    {/each}
  </select>
  <p class="hint" id="brw-default-hint">{m.brw_default_hint()}</p>

  <h3 class="sub caps">{m.brw_accesses()}</h3>
  {#if !info?.accesses.length}
    <p class="none">{m.brw_accesses_none()}</p>
  {:else}
    <ul class="rows accesses" data-testid="browser-accesses">
      {#each info.accesses as a, i (i)}
        <li class:refused={a.refused}>
          <span class="mono t">{time(a.at)}</span>
          <span class="main"><span class="name">{a.title || a.site}</span><span class="meta">{a.site} · {KIND[a.kind]?.() ?? a.kind}{a.refused ? ` · ${m.brw_refused()}` : ''}</span></span>
        </li>
      {/each}
    </ul>
  {/if}
  <p class="hint">{m.brw_rule()}</p>
</div>

<style>
  .browser {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .sub {
    margin-top: var(--space-3);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .none {
    font-size: var(--text-base);
    color: var(--ink-2);
  }
  .warn {
    color: var(--danger, var(--ink));
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--paper-rule-2);
  }
  .rows li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 8px 0;
    border-bottom: 1px solid var(--paper-rule-2);
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta,
  .t {
    font-size: var(--text-xs);
    color: var(--ink-2);
  }
  .refused .name {
    text-decoration: line-through;
  }
  select.input {
    align-self: flex-start;
    max-width: 100%;
  }
</style>
