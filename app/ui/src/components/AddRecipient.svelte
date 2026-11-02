<script lang="ts">
  import { FileUp, KeyRound } from '@lucide/svelte';
  import * as api from '../lib/api';
  import { errorText } from '../lib/errors';
  import { groupFingerprint, myKey } from '../lib/stores/mykey.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import type { RecipientCheck, SlotInfo, VaultId } from '../lib/types';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';

  let { open, vault, slots, onclose, onadded }: { open: boolean; vault: VaultId; slots: SlotInfo[]; onclose: () => void; onadded: (s: SlotInfo[]) => void } = $props();

  let text = $state('');
  let label = $state('');
  let checked = $state<RecipientCheck | null>(null);
  let keyError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let over = $state(false);
  let seq = 0;

  $effect(() => {
    if (!open) return;
    text = label = '';
    checked = null;
    keyError = error = null;
    over = false;
    void myKey.load();
    let off: (() => void) | undefined;
    let live = true;
    void api
      .onOsDragDrop((e) => {
        if (e.type === 'leave') over = false;
        else if (e.type === 'enter' || e.type === 'over') over = true;
        else if (e.type === 'drop') {
          over = false;
          if (e.paths[0]) void fromFile(e.paths[0]);
        }
      })
      .then((u) => (live ? (off = u) : u()));
    return () => {
      live = false;
      off?.();
    };
  });

  const mine = $derived(myKey.key?.public ?? null);
  const mineAdded = $derived(!!myKey.fingerprint && slots.some((s) => s.fingerprint === myKey.fingerprint));
  const isMine = $derived(!!checked && checked.fingerprint === myKey.fingerprint);

  async function settle(p: Promise<RecipientCheck>) {
    const n = ++seq;
    try {
      const c = await p;
      if (n !== seq) return;
      checked = c;
      keyError = null;
    } catch (e) {
      if (n !== seq) return;
      checked = null;
      keyError = errorText(e);
    }
  }

  function typed() {
    error = null;
    if (!text.trim()) {
      seq++;
      checked = null;
      keyError = null;
      return;
    }
    void settle(api.recipientCheck(text));
  }

  async function fromFile(path: string) {
    error = null;
    text = '';
    await settle(api.recipientReadFile(path));
    if (checked) text = checked.public;
  }

  async function choose() {
    const p = await api.pickFile(m.mk_public_filter(), ['nvpub', 'txt', 'pub']);
    if (p) await fromFile(p);
  }

  async function htmlDrop(e: DragEvent) {
    e.preventDefault();
    over = false;
    const f = e.dataTransfer?.files?.[0];
    if (!f) return;
    error = null;
    const head = await f.slice(0, 64 * 1024).text();
    await settle(api.recipientCheck(head));
    if (checked) text = checked.public;
  }

  function useMine() {
    if (!mine) return;
    text = mine;
    if (!label.trim()) label = m.rcp_mine_label();
    typed();
  }

  async function submit(e?: Event) {
    e?.preventDefault();
    if (!checked || busy) return;
    busy = true;
    error = null;
    try {
      const s = await api.slotAddRecipient(vault, checked.public, label.trim() || undefined);
      toasts.push(m.rcp_added(), 'success');
      busy = false;
      onadded(s);
      onclose();
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  function close() {
    if (!busy) onclose();
  }
</script>

<Dialog {open} title={m.rcp_title()} onclose={close} describedby="rcp-what" variant="wide">
  <form
    class="rcp"
    id="rcp-form"
    onsubmit={submit}
    data-testid="rcp-form"
    data-over={over}
    ondragover={(e) => {
      e.preventDefault();
      over = true;
    }}
    ondragleave={() => (over = false)}
    ondrop={htmlDrop}
  >
    <p id="rcp-what" class="hint">{m.rcp_what()}</p>
    <div class="field">
      <label for="rcp-key">{m.rcp_key()}</label>
      <textarea
        id="rcp-key"
        class="input mono key"
        rows="3"
        spellcheck="false"
        autocomplete="off"
        placeholder="nvpk1…"
        bind:value={text}
        oninput={typed}
        disabled={busy}
        aria-invalid={!!keyError}
        aria-describedby="rcp-state"
        data-testid="rcp-key"
      ></textarea>
    </div>
    <div class="sources">
      <button type="button" class="btn sm" onclick={choose} disabled={busy} data-testid="rcp-file"><FileUp size={14} aria-hidden="true" /> {m.rcp_file()}</button>
      {#if mine && !mineAdded}
        <button type="button" class="btn sm ghost" onclick={useMine} disabled={busy} data-testid="rcp-use-mine"><KeyRound size={14} aria-hidden="true" /> {m.rcp_use_mine()}</button>
      {/if}
      <span class="drop hint" aria-hidden="true">{m.rcp_drop()}</span>
    </div>
    <div id="rcp-state" class="state" aria-live="polite">
      {#if keyError}
        <p class="error-text" data-testid="rcp-key-error">{keyError}</p>
      {:else if checked}
        <p class="fpr" data-testid="rcp-fingerprint">{m.rcp_fpr({ fpr: groupFingerprint(checked.fingerprint) })}{#if isMine}{' · '}{m.rcp_you()}{/if}</p>
        {#if !isMine}<p class="hint">{m.rcp_check()}</p>{/if}
      {/if}
    </div>
    <div class="field">
      <label for="rcp-label">{m.rcp_label()}</label>
      <input id="rcp-label" class="input" bind:value={label} placeholder={m.rcp_label_ph()} maxlength="64" disabled={busy} data-testid="rcp-label" />
    </div>
    {#if error}<p class="error-text" role="alert" data-testid="rcp-error">{error}</p>{/if}
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={close} disabled={busy}>{m.action_cancel()}</button>
    <button type="submit" form="rcp-form" class="btn primary" disabled={!checked || busy} data-testid="rcp-submit">{busy ? m.action_saving() : m.action_add()}</button>
  {/snippet}
</Dialog>

<style>
  .rcp {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border-radius: var(--radius-md);
    outline: 2px dashed transparent;
    outline-offset: 6px;
    transition: outline-color var(--dur-micro) var(--ease-out);
  }
  .rcp[data-over='true'] {
    outline-color: var(--accent);
  }
  .key {
    resize: vertical;
    min-height: 4.5em;
    font-size: var(--text-xs);
    line-height: 1.4;
    word-break: break-all;
  }
  .sources {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .drop {
    margin-left: auto;
  }
  .state {
    min-height: 1.4em;
  }
  .fpr {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--ink);
    overflow-wrap: anywhere;
  }
</style>
