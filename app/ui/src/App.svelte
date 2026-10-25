<!--
  Root: theme, global listeners (vault-locked, jobs, idle), screen routing.
  Hooks for the designer on #app > .app:
    data-world="locked|open"  (documents on the board vs the open passport)
    data-screen="welcome|create|unlock|vault|details|passwords|settings|device"
  On <html>: data-theme (light|dark, absent = OS) and data-accent (styles/accents.css).
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from './lib/api';
  import { applyAccent } from './lib/accents';
  import { applySwitches } from './lib/bootstrap';
  import { switches } from './lib/switches';
  import { accent } from './lib/stores/accent.svelte';
  import { sidebar } from './lib/stores/sidebar.svelte';
  import { appLook } from './lib/stores/applook.svelte';
  import { app } from './lib/stores/app.svelte';
  import { drives } from './lib/stores/drives.svelte';
  import { idle, lockAllIdle } from './lib/stores/idle.svelte';
  import { jobs } from './lib/stores/jobs.svelte';
  import { prefs } from './lib/stores/prefs.svelte';
  import { update } from './lib/stores/update.svelte';
  import { vaults } from './lib/stores/vaults.svelte';
  import { installInk } from './ornament/ink.svelte';
  import { warmUp } from './lib/warm';
  import WarmUp from './components/WarmUp.svelte';
  import { installShellGuards } from './lib/shell';
  import Toasts from './components/Toasts.svelte';
  import RollbackNotice from './components/RollbackNotice.svelte';
  import SpaceDialog from './components/SpaceDialog.svelte';
  import CreateWizard from './screens/CreateWizard.svelte';
  import DeviceWizard from './screens/DeviceWizard.svelte';
  import Unlock from './screens/Unlock.svelte';
  import VaultScreen from './screens/VaultScreen.svelte';
  import Welcome from './screens/Welcome.svelte';
  import Settings from './screens/Settings.svelte';
  import { i18n, installLang } from './lib/i18n.svelte';

  let ready = $state(false);

  // Theme: OS by default; data-theme overrides (URL switch wins for screenshots).
  $effect(() => {
    const t = switches.theme ?? (prefs.theme === 'system' ? null : prefs.theme);
    if (t) document.documentElement.dataset.theme = t;
    else delete document.documentElement.dataset.theme;
  });

  // Accent preset: the saved setting; ?accent= wins for screenshots.
  $effect(() => applyAccent(switches.accent ?? accent.current));

  $effect(() => {
    document.title = vaults.active && app.world === 'open' ? `${vaults.active.name} · NiceVault` : 'NiceVault';
  });

  onMount(() => {
    installInk();
    warmUp(WarmUp);
    const disposers: (() => void)[] = [];
    disposers.push(installLang());
    disposers.push(installShellGuards(window, { app: api.isTauri, devMenu: api.isTauri && import.meta.env.DEV }));
    disposers.push(idle.install(() => void lockAllIdle()));
    (async () => {
      try {
        // first: the app's look (theme, ink, size) and the UI language saved in Impostazioni
        const g = await api.appSettingsGet();
        appLook.adopt(g);
        sidebar.adopt(g);
        i18n.adopt(g.language);
      } catch {
        /* the defaults and the OS language */
      }
      await jobs.listen();
      disposers.push(await api.onVaultLocked((e) => vaults.handleLocked(e)));
      disposers.push(await drives.listen());
      try {
        const s = await api.settingsGet();
        idle.setMinutes(s.auto_lock_minutes);
        accent.adopt(s.accent);
      } catch {
        /* keep default */
      }
      await app.loadRecents();
      await applySwitches();
      ready = true;
      const takePath = () => void app.takeLaunchPath(vaults.open, (id) => vaults.activate(id));
      disposers.push(await api.onOpenPath(takePath));
      takePath();
      // at most once a day, silent, never blocks the start
      void update.autoCheck();
    })();
    return () => disposers.forEach((d) => d());
  });
</script>

<svelte:window
  onkeydown={(e) => {
    // Impostazioni: Ctrl+, (Cmd+, on macOS), from any screen
    const mod = /Mac/i.test(navigator.platform) ? e.metaKey : e.ctrlKey;
    if (mod && !e.altKey && !e.shiftKey && e.key === ',' && ready) {
      e.preventDefault();
      app.openSettings();
    }
    // the volume's sidebar folded to a rail and back: Ctrl/Cmd+B (not while typing; a page that
    // uses the key itself, Password's "copy username", prevents it and wins)
    if (mod && !e.altKey && !e.shiftKey && e.code === 'KeyB' && ready && app.vaultMounted && !app.covered) {
      const t = e.target as HTMLElement | null;
      if (!t?.closest?.('input, textarea, [contenteditable]:not([contenteditable="false"])')) setTimeout(() => !e.defaultPrevented && sidebar.toggle());
    }
    // the interface size: Ctrl/Cmd+Shift+= / - / 0 (by key position: Shift changes the character)
    if (mod && e.shiftKey && !e.altKey && ready) {
      const dir = { Equal: 1, NumpadAdd: 1, Minus: -1, NumpadSubtract: -1, Digit0: 0, Numpad0: 0 }[e.code] as 1 | -1 | 0 | undefined;
      if (dir !== undefined) {
        e.preventDefault();
        appLook.step(dir);
      }
    }
  }}
/>

<div class="app" data-world={app.world} data-screen={app.screen} data-ready={ready}>
  {#if app.screen === 'welcome'}
    <Welcome />
  {:else if app.screen === 'create'}
    <CreateWizard />
  {:else if app.screen === 'unlock'}
    <Unlock />
  {:else if app.screen === 'device'}
    <DeviceWizard />
  {/if}
  {#if app.vaultMounted}
    <VaultScreen covered={app.covered} />
  {/if}
  {#if app.screen === 'settings'}
    <Settings />
  {/if}
  <Toasts />
  <RollbackNotice />
  <SpaceDialog />
</div>

<style>
  /* screens share one cell, so the document can turn over the open volume
     during the ceremony (ceremony.ts); the root colour shows only in between */
  .app {
    height: 100%;
    display: grid;
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    overflow: hidden;
    background: var(--ground-lo);
    color: var(--ink);
  }
  .app > :global(*) {
    grid-area: 1 / 1;
    min-width: 0;
    min-height: 0;
  }
</style>
