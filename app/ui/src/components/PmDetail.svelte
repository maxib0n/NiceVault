<!--
  One password-manager entry, as a record card: username, password (shown only on request),
  live 2FA code, websites, other fields, notes, tags, history. Every secret is read on demand
  (pm_reveal) or copied by the backend (pm_copy, wiped after its countdown). Revealed values are
  forgotten when another entry is chosen.
  Motion (glyph decode, lib/reveal.ts): the card decodes when it is opened with the pointer
  (`opened` counts those opens; never on the arrow keys), a value decodes as it is revealed,
  the 2FA code decodes when it turns over. Copied: the field's key holds a check until the
  clipboard is wiped.
  Hooks: [data-testid=pm-detail], [data-field=password|username|totp|field:i] rows.
-->
<script lang="ts">
  import { Check, Copy, Eye, EyeOff, FolderInput, History, Pencil, Star, Trash } from '@lucide/svelte';
  import * as api from '../lib/api';
  import type { PmEntry, PmHistoryItem, PmSecretField } from '../lib/api';
  import { formatDateTime, formatWhen } from '../lib/format';
  import { decode, type DecodeOptions } from '../lib/reveal';
  import type { PmStore } from '../lib/stores/pm.svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';
  import TotpRing from './TotpRing.svelte';

  let {
    store,
    entry,
    onedit,
    ondelete,
    onmove,
    opened = 0,
    historyOpen = $bindable(false),
  }: {
    store: PmStore;
    entry: PmEntry;
    onedit: () => void;
    ondelete: () => void;
    onmove?: () => void;
    /** how many times an entry was opened with the pointer (the card decodes each time) */
    opened?: number;
    historyOpen?: boolean;
  } = $props();

  // values shown on request, keyed by field; dropped for another entry
  let shown = $state<Record<string, string>>({});
  let revealed = $state(0);
  let history = $state<PmHistoryItem[]>([]);
  $effect(() => {
    void entry.id;
    shown = {};
    history = [];
  });

  async function toggle(field: PmSecretField) {
    if (shown[field] !== undefined) {
      const { [field]: _, ...rest } = shown;
      shown = rest;
      return;
    }
    const v = await store.reveal(field);
    if (v !== null) {
      revealed++;
      shown = { ...shown, [field]: v };
    }
  }

  $effect(() => {
    if (!historyOpen) return;
    void api.pmHistory(store.vault, entry.id).then((h) => (history = h)).catch(() => (history = []));
  });

  /** A decode only when asked for (an action can't be added conditionally). */
  function decodeWhen(node: HTMLElement, o: DecodeOptions | null) {
    let d: ReturnType<typeof decode> | null = o ? decode(node, o) : null;
    return {
      update(n: DecodeOptions | null) {
        if (!n) {
          d?.destroy();
          d = null;
        } else if (d) d.update(n);
        else d = decode(node, n);
      },
      destroy: () => d?.destroy(),
    };
  }
  const openDecode = $derived<DecodeOptions | null>(opened > 0 ? { key: `pm-open:${entry.id}:${opened}`, speed: 2.3, pointerOnly: true, fades: '.tools .btn, .badge' } : null);

  const isCopied = (f: PmSecretField) => store.copied?.field === f && store.selectedId === entry.id && (store.copyLeft === null || store.copyLeft > 0);
  const MASK = '••••••••••••';
</script>

<article class="detail" data-testid="pm-detail" data-entry={entry.id} use:decodeWhen={openDecode}>
  <header class="head">
    <div class="title-block">
      <h2 class="title display">{entry.title}</h2>
      {#if entry.folder || entry.tags.length}
        <p class="meta">
          {#if entry.folder}<span class="folder">{entry.folder.split('/').join(' › ')}</span>{/if}
          {#each entry.tags as t (t)}<span class="badge" data-tone="quiet">{t}</span>{/each}
        </p>
      {/if}
    </div>
    <div class="actions">
      <button
        type="button"
        class="btn ghost icon fav"
        aria-pressed={entry.favorite}
        aria-label={entry.favorite ? m.pm_unmark_favorite() : m.pm_mark_favorite()}
        title={entry.favorite ? m.pm_unmark_favorite() : m.pm_mark_favorite()}
        onclick={() => store.toggleFavorite(entry.id)}
        data-testid="pm-favorite"
      >
        <Star size={16} fill={entry.favorite ? 'currentColor' : 'none'} />
      </button>
      {#if onmove}<button type="button" class="btn ghost icon" aria-label={m.pm_move_to()} title={m.pm_move_to()} onclick={onmove} data-testid="pm-move-one"><FolderInput size={15} /></button>{/if}
      <button type="button" class="btn edit" onclick={onedit} data-testid="pm-edit"><Pencil size={15} aria-hidden="true" /> {m.action_edit()}</button>
      <button type="button" class="btn ghost icon" aria-label={m.action_delete()} title={m.action_delete()} onclick={ondelete} data-testid="pm-delete"><Trash size={15} /></button>
    </div>
  </header>

  <dl class="rows">
    {#if entry.username}
      <div class="row" data-field="username">
        <dt class="caps">{m.pm_username()}</dt>
        <dd class="value">{entry.username}</dd>
        <dd class="tools">
          <button type="button" class="btn ghost icon" class:copied={isCopied('username')} aria-label={m.pm_copy_field({ field: m.pm_username() })} title={m.pm_copy_field({ field: m.pm_username() })} onclick={() => store.copy('username', m.pm_username())}>
            {#if isCopied('username')}<Check size={15} />{:else}<Copy size={15} />{/if}
          </button>
        </dd>
      </div>
    {/if}
    {#if entry.has_password}
      <div class="row secret-row" data-field="password" data-shown={shown.password !== undefined}>
        <dt class="caps">{m.pm_password()}</dt>
        <dd class="value mono secret" data-testid="pm-password" data-shown={shown.password !== undefined}>{#if shown.password !== undefined}<span class="plain" use:decode={{ key: `pm-reveal:${entry.id}:password:${revealed}`, speed: 2.6 }}>{shown.password}</span>{:else}<span class="mask" aria-hidden="true">{MASK}</span><span class="visually-hidden">{m.pm_hidden()}</span>{/if}</dd>
        <dd class="tools">
          <button type="button" class="btn ghost icon" aria-pressed={shown.password !== undefined} aria-label={shown.password !== undefined ? m.pm_hide() : m.pm_reveal()} title={shown.password !== undefined ? m.pm_hide() : m.pm_reveal()} onclick={() => toggle('password')} data-testid="pm-reveal">
            {#if shown.password !== undefined}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
          </button>
          <button type="button" class="btn ghost icon" class:copied={isCopied('password')} aria-label={m.pm_copy_field({ field: m.pm_password() })} title={m.pm_copy_field({ field: m.pm_password() })} onclick={() => store.copy('password', m.pm_password())} data-testid="pm-copy-password">
            {#if isCopied('password')}<Check size={15} />{:else}<Copy size={15} />{/if}
          </button>
        </dd>
      </div>
    {/if}
    {#if entry.has_totp}
      <div class="row totp-row" data-field="totp">
        <dt class="caps">{m.pm_totp()}</dt>
        <dd class="value"><TotpRing vault={store.vault} id={entry.id} />{#if entry.totp?.issuer}<span class="issuer">{entry.totp.issuer}</span>{/if}</dd>
        <dd class="tools">
          <button type="button" class="btn ghost icon" class:copied={isCopied('totp_code')} aria-label={m.pm_copy_field({ field: m.pm_totp() })} title={m.pm_copy_field({ field: m.pm_totp() })} onclick={() => store.copy('totp_code', m.pm_totp())} data-testid="pm-copy-totp">
            {#if isCopied('totp_code')}<Check size={15} />{:else}<Copy size={15} />{/if}
          </button>
        </dd>
      </div>
    {/if}
    {#if entry.urls.length}
      <div class="row" data-field="urls">
        <dt class="caps">{m.pm_urls()}</dt>
        <dd class="value urls">
          {#each entry.urls as u (u)}<span class="mono url">{u}</span>{/each}
        </dd>
        <dd class="tools"></dd>
      </div>
    {/if}
    {#each entry.fields as f, i (i)}
      {@const key = `field:${i}` as PmSecretField}
      <div class="row" data-field={key}>
        <dt class="caps">{f.name}</dt>
        <dd class="value" class:mono={f.hidden} class:secret={f.hidden} data-shown={shown[key] !== undefined}>{#if !f.hidden}{f.value}{:else if shown[key] !== undefined}<span class="plain" use:decode={{ key: `pm-reveal:${entry.id}:${key}:${revealed}`, speed: 2.6 }}>{shown[key]}</span>{:else}<span class="mask" aria-hidden="true">{MASK}</span><span class="visually-hidden">{m.pm_hidden()}</span>{/if}</dd>
        <dd class="tools">
          {#if f.hidden}
            <button type="button" class="btn ghost icon" aria-pressed={shown[key] !== undefined} aria-label={shown[key] !== undefined ? m.pm_hide() : m.pm_reveal()} onclick={() => toggle(key)}>
              {#if shown[key] !== undefined}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
            </button>
          {/if}
          <button type="button" class="btn ghost icon" class:copied={isCopied(key)} aria-label={m.pm_copy_field({ field: f.name })} onclick={() => store.copy(key, f.name)}>
            {#if isCopied(key)}<Check size={15} />{:else}<Copy size={15} />{/if}
          </button>
        </dd>
      </div>
    {/each}
    {#if entry.notes}
      <div class="row notes" data-field="notes">
        <dt class="caps">{m.pm_notes()}</dt>
        <dd class="value"><p class="note">{entry.notes}</p></dd>
        <dd class="tools"></dd>
      </div>
    {/if}
  </dl>

  <footer class="foot">
    <span class="dates" data-testid="pm-dates"
      ><span title={formatDateTime(entry.created)} data-when={entry.created}>{m.pm_created({ date: formatWhen(entry.created) })}</span><span class="dot" aria-hidden="true"> · </span><span title={formatDateTime(entry.updated)} data-when={entry.updated}>{m.pm_updated({ date: formatWhen(entry.updated) })}</span></span
    >
    {#if entry.history_count > 0}
      <button type="button" class="btn ghost sm" onclick={() => (historyOpen = true)} data-testid="pm-history">
        <History size={14} aria-hidden="true" /> {m.pm_history_n({ count: entry.history_count, n: String(entry.history_count) })}
      </button>
    {/if}
  </footer>
</article>

<Dialog open={historyOpen} title={m.pm_history_title({ name: entry.title })} onclose={() => (historyOpen = false)}>
  {#if history.length === 0}
    <p class="hint">{m.pm_history_empty()}</p>
  {:else}
    <ul class="history" data-testid="pm-history-list">
      {#each history as h (h.index)}
        {@const key = `history:${h.index}` as PmSecretField}
        <li>
          <span class="when" title={formatDateTime(h.changed)}>{m.pm_history_changed({ date: formatWhen(h.changed) })}</span>
          <span class="mono secret" data-shown={shown[key] !== undefined}>{#if shown[key] !== undefined}<span class="plain" use:decode={{ key: `pm-reveal:${entry.id}:${key}:${revealed}`, speed: 2.6 }}>{shown[key]}</span>{:else}{MASK}{/if}</span>
          <button type="button" class="btn ghost icon" aria-label={shown[key] !== undefined ? m.pm_hide() : m.pm_reveal()} onclick={() => toggle(key)}>
            {#if shown[key] !== undefined}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
          </button>
          <button type="button" class="btn ghost icon" class:copied={isCopied(key)} aria-label={m.action_copy()} onclick={() => store.copy(key, m.pm_password())}>
            {#if isCopied(key)}<Check size={14} />{:else}<Copy size={14} />{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (historyOpen = false)}>{m.action_close()}</button>
  {/snippet}
</Dialog>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
    max-width: 880px;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .title-block {
    min-width: 0;
    padding-top: 2px;
  }
  .title {
    font-size: clamp(26px, 2.6vw, 34px);
    font-weight: 500;
    line-height: 1.1;
    color: var(--title-ink);
    overflow-wrap: anywhere;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
  .folder {
    margin-right: 4px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
  }
  .edit {
    margin: 0 4px;
  }
  .fav[aria-pressed='true'] {
    color: var(--accent);
  }

  /* the record: a ruled ledger, printed labels on the left, the values set on the line */
  .rows {
    margin: 0;
    border-top: 1px solid var(--paper-rule-2);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(6.5em, 10em) minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-3);
    min-height: 52px;
    padding: 8px 2px;
    border-bottom: 1px solid var(--paper-rule);
  }
  .row dt {
    margin: 0;
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .issuer {
    display: block;
    margin-top: 3px;
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .row dd {
    margin: 0;
    min-width: 0;
  }
  .value {
    font-size: var(--text-base);
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  /* a secret: the mask is a row of printed bullets, the value is set in the mono */
  .secret {
    letter-spacing: 0.04em;
  }
  .mask {
    display: inline-block;
    letter-spacing: 0.18em;
    color: var(--ink-3);
    white-space: nowrap;
    overflow: hidden;
    max-width: 100%;
    vertical-align: middle;
  }
  .plain {
    color: var(--title-ink);
  }
  /* a revealed value sits on a pressed slip, so it reads as the thing shown on request */
  .secret-row[data-shown='true'] .secret {
    justify-self: start;
    padding: 3px 8px;
    margin: -3px -8px;
    border-radius: var(--radius-sm);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .tools {
    display: flex;
    gap: 2px;
    justify-content: flex-end;
  }
  /* copied: the key holds a check in the ink until the clipboard is wiped */
  .tools :global(.btn.copied) {
    color: var(--accent);
    --face-hi: var(--accent-soft);
    --face-lo: var(--accent-soft);
    --edge: color-mix(in oklab, var(--accent-line) 50%, transparent);
  }
  .urls {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .url {
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }
  .note {
    white-space: pre-wrap;
    line-height: 1.5;
  }
  .row.notes {
    align-items: start;
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  /* the line is a flex row (its spaces collapse): the dot keeps its own room */
  .dot {
    margin: 0 0.5em;
    color: var(--ink-4);
  }
  .history {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .history li {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    align-items: center;
    gap: 6px;
    padding: 6px 0;
    border-bottom: 1px solid var(--paper-rule);
  }
  .when {
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
</style>
