<script lang="ts">
  import { onMount } from 'svelte';
  import { Copy, KeyRound } from '@lucide/svelte';
  import * as api from '../lib/api';
  import { errorText } from '../lib/errors';
  import { i18n } from '../lib/i18n.svelte';
  import { preloadStrength, type Strength } from '../lib/strength';
  import { switches } from '../lib/switches';
  import { groupFingerprint, myKey } from '../lib/stores/mykey.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';
  import PasswordField from './PasswordField.svelte';
  import StrengthMeter from './StrengthMeter.svelte';

  type Kind = 'create' | 'import' | 'remove';
  let dialog = $state<Kind | null>(null);
  let pw = $state('');
  let pw2 = $state('');
  let file = $state<string | null>(null);
  let strength = $state<Strength | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const key = $derived(myKey.key);
  const pub = $derived(key?.public ?? null);
  const short = $derived(pub ? `${pub.slice(0, 22)}…${pub.slice(-10)}` : '');

  onMount(() => {
    void myKey.load(true).then(() => {
      const d = !api.isTauri ? switches.dialog : null;
      if (d === 'mykey-create' || d === 'mykey-import' || d === 'mykey-remove') open(d.slice(6) as Kind);
    });
  });

  function open(k: Kind) {
    dialog = k;
    pw = pw2 = '';
    file = null;
    error = null;
    strength = null;
    if (k === 'create') preloadStrength();
  }
  function close() {
    if (busy) return;
    dialog = null;
    pw = pw2 = '';
  }

  const mismatch = $derived(pw2.length > 0 && pw2 !== pw);
  const canCreate = $derived(!!strength?.acceptable && pw === pw2 && !busy);
  const canImport = $derived(!!file && pw.length > 0 && !busy);

  async function run(e: Event | undefined, f: () => Promise<void>) {
    e?.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    try {
      await f();
      busy = false;
      close();
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  const create = (e?: Event) =>
    canCreate &&
    run(e, async () => {
      myKey.set(await api.myKeyCreate(pw));
      toasts.push(m.mk_created(), 'success');
    });
  const doImport = (e?: Event) =>
    canImport &&
    run(e, async () => {
      myKey.set(await api.myKeyImport(file!, pw));
      toasts.push(m.mk_imported(), 'success');
    });
  const remove = () =>
    run(undefined, async () => {
      myKey.set(await api.myKeyRemove());
      toasts.push(m.mk_removed(), 'success');
    });

  async function chooseFile() {
    const p = await api.pickFile(m.mk_secret_filter(), ['nvkey']);
    if (p) file = p;
  }

  async function copy() {
    if (!pub) return;
    try {
      await navigator.clipboard.writeText(pub);
      toasts.push(m.mk_copied(), 'success');
    } catch {
    }
  }

  async function save(kind: 'public' | 'secret') {
    const pub = kind === 'public';
    const dest = await api.pickSavePath(pub ? m.mk_public_filter() : m.mk_secret_filter(), pub ? 'la-mia-chiave.nvpub' : 'la-mia-chiave.nvkey', [pub ? 'nvpub' : 'nvkey']);
    if (!dest) return;
    try {
      await (pub ? api.myKeyExportPublic(dest) : api.myKeyExport(dest));
      toasts.push(m.mk_saved({ path: dest }), 'success');
    } catch (e) {
      toasts.error(e);
    }
  }

  const when = (ms: number) => new Intl.DateTimeFormat(i18n.tag, { dateStyle: 'long' }).format(ms);
  const fileName = $derived(file?.split(/[\\/]/).pop() ?? '');
</script>

<div class="mykey" data-testid="mykey" data-state={pub ? 'ready' : 'none'}>
  <p class="hint">{m.mk_hint()}</p>
  {#if key && !key.persisted}<p class="hint warn" data-testid="mykey-portable">{m.mk_portable()}</p>{/if}
  {#if !key}
    <!-- loading -->
  {:else if !pub}
    <p class="none">{m.mk_none()}</p>
    <div class="row">
      <button type="button" class="btn sm primary" onclick={() => open('create')} data-testid="mykey-create"><KeyRound size={14} aria-hidden="true" /> {m.mk_create()}</button>
      <button type="button" class="btn sm" onclick={() => open('import')} data-testid="mykey-import">{m.mk_import()}</button>
    </div>
  {:else}
    <h3 class="sub caps" id="mk-pub">{m.mk_public()}</h3>
    <p class="key mono" title={pub} aria-describedby="mk-pub" data-testid="mykey-public" data-public={pub}>{short}</p>
    <div class="row">
      <button type="button" class="btn sm" onclick={copy} data-testid="mykey-copy"><Copy size={14} aria-hidden="true" /> {m.mk_copy()}</button>
      <button type="button" class="btn sm" onclick={() => save('public')} data-testid="mykey-save-public">{m.mk_save_public()}</button>
    </div>
    <p class="hint">{m.mk_share_hint()}</p>
    <h3 class="sub caps">{m.mk_fingerprint()}</h3>
    <p class="fpr mono" data-testid="mykey-fingerprint">{groupFingerprint(key.fingerprint ?? '')}</p>
    {#if key.created_ms}<p class="hint">{m.mk_created_on({ date: when(key.created_ms) })}</p>{/if}
    <div class="row secret">
      <button type="button" class="btn sm" onclick={() => save('secret')} data-testid="mykey-export">{m.mk_export()}</button>
      <button type="button" class="btn sm ghost" onclick={() => open('remove')} data-testid="mykey-remove">{m.mk_remove()}</button>
    </div>
    <p class="hint">{m.mk_export_hint()}</p>
  {/if}
</div>

<Dialog open={dialog === 'create'} title={m.mk_create()} onclose={close} describedby="mk-create-what">
  <form class="dialog-form" id="mk-create-form" onsubmit={create} data-testid="mykey-create-form">
    <p id="mk-create-what" class="hint">{m.mk_create_what()}</p>
    <PasswordField bind:value={pw} label={m.mk_password()} autocomplete="new-password" autofocus describedby="mk-strength" disabled={busy} />
    <StrengthMeter id="mk-strength" password={pw} userInputs={[]} bind:strength />
    <PasswordField bind:value={pw2} label={m.det_repeat()} autocomplete="new-password" invalid={mismatch} disabled={busy} />
    {#if mismatch}<p class="error-text">{m.passwords_mismatch()}</p>{/if}
    {#if error}<p class="error-text" role="alert">{error}</p>{/if}
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={close} disabled={busy}>{m.action_cancel()}</button>
    <button type="submit" form="mk-create-form" class="btn primary" disabled={!canCreate} data-testid="mykey-create-submit">{busy ? m.action_saving() : m.mk_create()}</button>
  {/snippet}
</Dialog>

<Dialog open={dialog === 'import'} title={m.mk_import_title()} onclose={close} describedby="mk-import-what">
  <form class="dialog-form" id="mk-import-form" onsubmit={doImport} data-testid="mykey-import-form">
    <p id="mk-import-what" class="hint">{m.mk_import_what()}</p>
    <div class="file-row">
      <button type="button" class="btn sm" onclick={chooseFile} disabled={busy} data-testid="mykey-import-file">{m.mk_choose_file()}</button>
      {#if file}<span class="mono file" title={file}>{fileName}</span>{/if}
    </div>
    <PasswordField bind:value={pw} label={m.mk_password()} disabled={busy} invalid={!!error} />
    {#if error}<p class="error-text" role="alert">{error}</p>{/if}
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={close} disabled={busy}>{m.action_cancel()}</button>
    <button type="submit" form="mk-import-form" class="btn primary" disabled={!canImport} data-testid="mykey-import-submit">{busy ? m.action_saving() : m.mk_import_go()}</button>
  {/snippet}
</Dialog>

<Dialog open={dialog === 'remove'} title={m.mk_remove_title()} variant="danger" onclose={close} describedby="mk-remove-desc">
  <p id="mk-remove-desc">{m.mk_remove_desc()}</p>
  {#if error}<p class="error-text" role="alert">{error}</p>{/if}
  {#snippet actions()}
    <button type="button" class="btn" onclick={close} disabled={busy}>{m.action_cancel()}</button>
    <button type="button" class="btn danger" onclick={remove} disabled={busy} data-testid="mykey-remove-confirm">{m.action_remove()}</button>
  {/snippet}
</Dialog>

<style>
  .mykey {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .none {
    font-size: var(--text-base);
    color: var(--ink-2);
  }
  .warn {
    color: var(--danger, var(--ink));
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .row.secret {
    margin-top: var(--space-2);
  }
  .sub {
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .key,
  .fpr {
    font-size: var(--text-sm);
    color: var(--ink);
    overflow-wrap: anywhere;
    user-select: all;
  }
  .fpr {
    letter-spacing: 0.04em;
  }
  .dialog-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .file-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .file {
    min-width: 0;
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
