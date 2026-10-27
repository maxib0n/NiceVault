<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '../lib/api';
  import { applyAccent } from '../lib/accents';
  import { browserUnlockCancel, type CompactSpec } from '../lib/browserLink';
  import { errorCode, errorText } from '../lib/errors';
  import { i18n } from '../lib/i18n.svelte';
  import type { RecentEntry } from '../lib/types';
  import { m } from '../paraglide/messages.js';
  import PasswordField from '../components/PasswordField.svelte';

  let { spec }: { spec: CompactSpec } = $props();

  let path = $state('');
  let recents = $state<RecentEntry[]>([]);
  let password = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let done = $state(false);
  let input = $state<HTMLInputElement>();

  const name = $derived(recents.find((r) => r.path === path)?.name ?? path.split(/[\\/]/).pop()?.replace(/\.nvault$/i, '') ?? '');

  onMount(() => {
    path = spec.path;
    (window as unknown as { __nvCompactSet: (s: CompactSpec) => void }).__nvCompactSet = (s) => {
      path = s.path;
      password = '';
      error = null;
      input?.focus();
    };
    void (async () => {
      try {
        const g = await api.appSettingsGet();
        i18n.adopt(g.language);
        if (g.app_theme !== 'system') document.documentElement.dataset.theme = g.app_theme;
        applyAccent(g.app_accent);
      } catch {
      }
      recents = await api.recentList().catch(() => []);
      document.title = m.brw_unlock_title({ name });
    })();
  });

  async function unlock() {
    if (busy || !password) return;
    busy = true;
    error = null;
    try {
      await api.vaultOpen(path, password);
      password = '';
      done = true;
    } catch (e) {
      error = errorCode(e) === 'wrong_password' ? m.err_wrong_password() : errorText(e);
      input?.select();
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    password = '';
    await browserUnlockCancel().catch(() => {});
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && void cancel()} />

<main class="compact" data-testid="unlock-compact" data-path={path}>
  <h1 class="display">{m.brw_unlock_title({ name })}</h1>
  <p class="hint">{m.brw_unlock_hint()}</p>
  {#if recents.filter((r) => r.exists).length > 1}
    <label class="other">
      <span class="caps">{m.brw_unlock_other()}</span>
      <select class="input" bind:value={path} disabled={busy}>
        {#each recents.filter((r) => r.exists) as r (r.path)}
          <option value={r.path}>{r.name}</option>
        {/each}
      </select>
    </label>
  {/if}
  <PasswordField bind:value={password} bind:input label={m.brw_unlock_password()} autofocus invalid={!!error} disabled={busy || done} onkeydown={(e) => e.key === 'Enter' && void unlock()} />
  <p class="error" role="alert" aria-live="assertive">{error ?? ''}</p>
  <div class="actions">
    <button type="button" class="btn" onclick={cancel} disabled={busy}>{m.brw_unlock_cancel()}</button>
    <button type="button" class="btn primary" onclick={unlock} disabled={busy || !password || done} data-testid="unlock-compact-go">{m.brw_unlock_go()}</button>
  </div>
</main>

<style>
  .compact {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5);
    background: var(--paper, var(--ground-lo));
    color: var(--ink);
  }
  h1 {
    margin: 0;
    font-size: var(--display-s);
    font-weight: 500;
  }
  .other {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .error {
    min-height: 1.2em;
    margin: 0;
    color: var(--danger, #b71c1c);
    font-size: var(--text-sm, 13px);
  }
  .actions {
    margin-top: auto;
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
