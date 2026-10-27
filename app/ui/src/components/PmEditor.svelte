<script lang="ts">
  import { Eye, EyeOff, Plus, Wand2, X } from '@lucide/svelte';
  import * as api from '../lib/api';
  import type { PmEntry, PmEntryInput, PmTotpCheck } from '../lib/api';
  import { errorText } from '../lib/errors';
  import type { PmStore } from '../lib/stores/pm.svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';
  import PasswordField from './PasswordField.svelte';
  import PmFolderPicker from './PmFolderPicker.svelte';
  import PmGenerator from './PmGenerator.svelte';

  let { open, store, entry, onclose }: { open: boolean; store: PmStore; entry: PmEntry | null; onclose: () => void } = $props();

  interface FieldDraft {
    name: string;
    value: string;
    hidden: boolean;
    keep: boolean;
    reveal: boolean;
  }

  let title = $state('');
  let username = $state('');
  let password = $state('');
  let urls = $state<string[]>([]);
  let folder = $state('');
  let tags = $state('');
  let favorite = $state(false);
  let notes = $state('');
  let totp = $state('');
  let removeTotp = $state(false);
  let fields = $state<FieldDraft[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let titleTouched = $state(false);
  let genOpen = $state(false);
  let check = $state<PmTotpCheck | null>(null);
  let checkError = $state<string | null>(null);
  let wasOpen = false;

  $effect(() => {
    if (open && !wasOpen) {
      const e = entry;
      title = e?.title ?? '';
      username = e?.username ?? '';
      password = '';
      urls = e ? [...e.urls] : [''];
      folder = e?.folder ?? (store.filter.kind === 'folder' ? store.filter.folder : '');
      tags = e?.tags.join(', ') ?? '';
      favorite = e?.favorite ?? false;
      notes = e?.notes ?? '';
      totp = '';
      removeTotp = false;
      fields = (e?.fields ?? []).map((f) => ({ name: f.name, value: f.value ?? '', hidden: f.hidden, keep: f.hidden && f.value === null, reveal: false }));
      busy = false;
      error = null;
      titleTouched = false;
      check = null;
      checkError = null;
    }
    wasOpen = open;
  });

  let checkSeq = 0;
  $effect(() => {
    const t = totp.trim();
    const mine = ++checkSeq;
    if (!t) {
      check = null;
      checkError = null;
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const r = await api.pmTotpCheck(t);
        if (mine === checkSeq) {
          check = r;
          checkError = null;
        }
      } catch (e) {
        if (mine === checkSeq) {
          check = null;
          checkError = errorText(e);
        }
      }
    }, 150);
    return () => clearTimeout(timer);
  });

  const titleMissing = $derived(title.trim() === '');
  const canSave = $derived(!busy && !checkError);

  async function save(e?: Event) {
    e?.preventDefault();
    titleTouched = true;
    if (!canSave || titleMissing) return;
    busy = true;
    const input: PmEntryInput = {
      title: title.trim(),
      username,
      urls: urls.map((u) => u.trim()).filter(Boolean),
      folder,
      tags: tags.split(',').map((t) => t.trim()).filter(Boolean),
      favorite,
      notes,
      fields: fields.filter((f) => f.name.trim()).map((f) => (f.keep ? { name: f.name.trim(), hidden: f.hidden } : { name: f.name.trim(), value: f.value, hidden: f.hidden })),
    };
    if (password || !entry) input.password = password;
    if (totp.trim()) input.totp = totp.trim();
    else if (removeTotp) input.totp = '';
    error = await store.save(input, entry?.id ?? null);
    busy = false;
    if (!error) onclose();
  }

  const fieldId = (i: number, k: string) => `pm-field-${i}-${k}`;
</script>

<Dialog {open} title={entry ? m.pm_edit_title() : m.pm_new_title()} variant="wide" onclose={() => !busy && onclose()}>
  <form class="editor" id="pm-editor-form" onsubmit={save} data-testid="pm-editor" novalidate>
    <div class="field">
      <label for="pm-title">{m.pm_f_title()}</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input id="pm-title" class="input" bind:value={title} autocomplete="off" autofocus aria-invalid={titleTouched && titleMissing} />
      {#if titleTouched && titleMissing}<p class="error-text">{m.pm_f_title_needed()}</p>{/if}
    </div>
    <div class="field">
      <label for="pm-user">{m.pm_f_username()}</label>
      <input id="pm-user" class="input" bind:value={username} autocomplete="off" spellcheck="false" />
    </div>
    <div>
      <div class="pw">
        <PasswordField bind:value={password} label={m.pm_f_password()} autocomplete="new-password" placeholder={entry?.has_password ? m.pm_f_password_keep() : undefined} />
        <button type="button" class="btn gen-btn" onclick={() => (genOpen = true)} data-testid="pm-generate"><Wand2 size={15} aria-hidden="true" /> {m.pm_f_generate()}</button>
      </div>
    </div>

    <fieldset class="group">
      <legend class="field-label">{m.pm_f_urls()}</legend>
      {#each urls as _, i (i)}
        <div class="line">
          <input class="input mono" bind:value={urls[i]} placeholder="https://" spellcheck="false" aria-label="{m.pm_f_urls()} {i + 1}" />
          <button type="button" class="btn ghost icon" aria-label={m.pm_f_url_remove()} onclick={() => (urls = urls.filter((_, j) => j !== i))}><X size={14} /></button>
        </div>
      {/each}
      <button type="button" class="btn sm ghost add" onclick={() => (urls = [...urls, ''])}><Plus size={14} aria-hidden="true" /> {m.pm_f_url_add()}</button>
    </fieldset>

    <div class="two">
      <div class="field">
        <label for="pm-folder">{m.pm_f_folder()}</label>
        <PmFolderPicker id="pm-folder" folders={store.folders} bind:value={folder} label={m.pm_f_folder()} />
      </div>
      <div class="field">
        <label for="pm-tags">{m.pm_f_tags()}</label>
        <input id="pm-tags" class="input" bind:value={tags} placeholder={m.pm_f_tags_ph()} autocomplete="off" />
      </div>
    </div>

    <div class="field">
      <label for="pm-totp">{m.pm_f_totp()}</label>
      <input id="pm-totp" class="input mono" bind:value={totp} placeholder={m.pm_f_totp_ph()} spellcheck="false" autocomplete="off" aria-invalid={!!checkError} aria-describedby="pm-totp-hint" data-testid="totp-input" />
      <p id="pm-totp-hint" class={checkError ? 'error-text' : 'hint'} data-testid="totp-check" data-state={checkError ? 'error' : check ? 'ok' : 'empty'}>
        {#if checkError}
          {checkError}
        {:else if check}
          {m.pm_f_totp_ok({ issuer: check.issuer ?? check.account ?? 'TOTP', digits: String(check.digits), period: String(check.period), code: check.code })}
        {:else if entry?.has_totp && !removeTotp}
          {m.pm_f_totp_keep()}
        {:else if removeTotp}
          {m.pm_f_totp_removed()}
        {:else}
          {m.pm_f_totp_hint()}
        {/if}
      </p>
      {#if entry?.has_totp && !totp.trim()}
        <label class="check"><input type="checkbox" bind:checked={removeTotp} /> {m.pm_f_totp_remove()}</label>
      {/if}
    </div>

    <fieldset class="group">
      <legend class="field-label">{m.pm_f_fields()}</legend>
      {#each fields as f, i (i)}
        <div class="custom">
          <input class="input" bind:value={f.name} placeholder={m.pm_f_field_name()} aria-label={m.pm_f_field_name()} id={fieldId(i, 'n')} />
          <input
            class="input"
            type={f.hidden && !f.reveal ? 'password' : 'text'}
            bind:value={f.value}
            placeholder={f.keep ? m.pm_f_field_keep() : m.pm_f_field_value()}
            aria-label={m.pm_f_field_value()}
            oninput={() => (f.keep = false)}
            autocomplete="off"
          />
          {#if f.hidden && !f.keep}
            <button type="button" class="btn ghost icon" aria-label={f.reveal ? m.pm_hide() : m.pm_reveal()} aria-pressed={f.reveal} onclick={() => (f.reveal = !f.reveal)}>
              {#if f.reveal}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
            </button>
          {/if}
          <label class="check small"><input type="checkbox" bind:checked={f.hidden} /> {m.pm_f_field_hidden()}</label>
          <button type="button" class="btn ghost icon" aria-label={m.pm_f_field_remove()} onclick={() => (fields = fields.filter((_, j) => j !== i))}><X size={14} /></button>
        </div>
      {/each}
      <button type="button" class="btn sm ghost add" onclick={() => (fields = [...fields, { name: '', value: '', hidden: false, keep: false, reveal: false }])}><Plus size={14} aria-hidden="true" /> {m.pm_f_field_add()}</button>
    </fieldset>

    <div class="field">
      <label for="pm-notes">{m.pm_f_notes()}</label>
      <textarea id="pm-notes" class="input notes" bind:value={notes} rows="3"></textarea>
    </div>
    <div class="switch-row">
      <span class="switch-label" id="pm-fav">{m.pm_f_favorite()}</span>
      <button type="button" class="switch" role="switch" aria-checked={favorite} aria-labelledby="pm-fav" onclick={() => (favorite = !favorite)}><span class="knob"></span></button>
    </div>
    {#if error}<p class="error-text" role="alert">{error}</p>{/if}
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={onclose} disabled={busy}>{m.action_cancel()}</button>
    <button type="submit" form="pm-editor-form" class="btn primary" disabled={!canSave} data-testid="pm-save">{busy ? m.action_saving() : m.action_save()}</button>
  {/snippet}
</Dialog>

<PmGenerator
  open={genOpen}
  onclose={() => (genOpen = false)}
  onuse={(v) => {
    password = v;
    genOpen = false;
  }}
/>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: var(--space-3);
    align-items: start;
  }
  .pw {
    display: flex;
    align-items: flex-end;
    gap: 6px;
  }
  .pw > :global(.password-field) {
    flex: 1;
    min-width: 0;
  }
  .gen-btn {
    flex: none;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  .group legend {
    padding: 0;
    margin-bottom: 6px;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--ink);
  }
  .line {
    display: flex;
    gap: 4px;
  }
  .line .input {
    flex: 1;
  }
  .custom {
    display: grid;
    grid-template-columns: minmax(0, 0.8fr) minmax(0, 1.2fr) auto auto auto;
    gap: 6px;
    align-items: center;
  }
  .add {
    align-self: flex-start;
  }
  .notes {
    height: auto;
    min-height: 72px;
    padding: 8px 10px;
    resize: vertical;
    line-height: 1.45;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .check:hover {
    color: var(--ink);
  }
  .check.small {
    white-space: nowrap;
  }
</style>
