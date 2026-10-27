<script lang="ts">
  import { ArrowLeft, FilePlus2, Globe, HardDrive, Info, KeyRound, Languages, Palette, Power, RefreshCw, ScrollText } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import * as api from '../lib/api';
  import type { AppInfo, AppSettings } from '../lib/api';
  import { ACCENTS, type AccentId } from '../lib/accents';
  import { i18n, systemLocale, type LangChoice } from '../lib/i18n.svelte';
  import { radioKeys } from '../lib/radio';
  import { switches } from '../lib/switches';
  import { isMobile } from '../lib/platform';
  import { app } from '../lib/stores/app.svelte';
  import { appLook } from '../lib/stores/applook.svelte';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { update } from '../lib/stores/update.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import type { LockScope, Settings, ThemeId } from '../lib/types';
  import { m } from '../paraglide/messages.js';
  import sources from '../../SOURCES.md?raw';
  import Brand from '../components/Brand.svelte';
  import Maker from '../components/Maker.svelte';
  import Dialog from '../components/Dialog.svelte';
  import Band from '../ornament/Band.svelte';
  import InkChip from '../components/InkChip.svelte';
  import Stitch from '../ornament/Stitch.svelte';
  import BrowserSettings from '../components/BrowserSettings.svelte';

  let settings = $state<AppSettings | null>(null);
  let info = $state<AppInfo | null>(null);
  let licences = $state(false);
  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

  onMount(async () => {
    try {
      [settings, info] = await Promise.all([api.appSettingsGet(), api.appInfo()]);
    } catch (e) {
      toasts.error(e);
    }
    if (!api.isTauri && switches.dialog === 'licences') licences = true;
    if (!update.prefs) void update.loadPrefs();
  });

  const LANGS: { id: LangChoice; label: () => string }[] = [
    { id: 'system', label: () => m.set_lang_system({ lang: systemLocale() === 'it' ? m.lang_it() : m.lang_en() }) },
    { id: 'it', label: m.lang_it },
    { id: 'en', label: m.lang_en },
  ];
  async function setLanguage(l: LangChoice) {
    const prev = i18n.choice;
    i18n.choice = l;
    try {
      settings = await api.appSettingsSet({ language: l });
      i18n.adopt(settings.language);
    } catch (e) {
      i18n.choice = prev;
      toasts.error(e);
    }
  }

  async function setDefaults(patch: Partial<Settings>) {
    if (!settings) return;
    const prev = settings;
    settings = { ...settings, defaults: { ...settings.defaults, ...patch } };
    try {
      settings = await api.appSettingsSet({ defaults: patch });
    } catch (e) {
      settings = prev;
      toasts.error(e);
    }
  }

  const VIDEO_ARROWS = ['seek', 'step'] as const;
  const APP_THEMES: { id: ThemeId; label: () => string }[] = [
    { id: 'dark', label: m.theme_dark },
    { id: 'light', label: m.theme_light },
    { id: 'system', label: m.theme_system },
  ];
  async function setApp(patch: Parameters<typeof appLook.set>[0]) {
    const s = await appLook.set(patch);
    if (s) settings = s;
  }

  let scroller = $state<HTMLElement>();
  async function setScale(z: number, e: MouseEvent | KeyboardEvent) {
    if (z === appLook.scale) return;
    const btn = e.currentTarget as HTMLElement;
    const z0 = appLook.scale / 100;
    const r0 = btn.getBoundingClientRect();
    const y0 = e instanceof MouseEvent && e.clientY > 0 ? e.clientY : r0.top + r0.height / 2;
    const resized = new Promise<void>((res) => {
      addEventListener('resize', () => res(), { once: true });
      setTimeout(res, 250);
    });
    await setApp({ ui_scale: z });
    await resized;
    await new Promise((r) => requestAnimationFrame(r));
    const target = document.querySelector<HTMLElement>(`[data-scale="${z}"]`);
    if (!target || !scroller) return;
    const r1 = target.getBoundingClientRect();
    const y1 = (y0 * z0) / (z / 100);
    scroller.scrollTop += r1.top + r1.height / 2 - y1;
    target.focus({ preventScroll: true });
  }
  async function setStartup(patch: { start_at_login?: boolean; start_minimized?: boolean; close_to_tray?: boolean }) {
    if (!settings) return;
    const prev = settings;
    settings = { ...settings, ...patch };
    try {
      settings = await api.appSettingsSet(patch);
    } catch (e) {
      settings = prev;
      toasts.error(e);
    }
  }
  async function setGallery(on: boolean) {
    if (!settings) return;
    const prev = settings;
    settings = { ...settings, gallery_save: on };
    try {
      settings = await api.appSettingsSet({ gallery_save: on });
    } catch (e) {
      settings = prev;
      toasts.error(e);
    }
  }
  async function setSiteIcons(on: boolean) {
    if (!settings) return;
    const prev = settings;
    settings = { ...settings, site_icons: on };
    try {
      settings = await api.appSettingsSet({ site_icons: on });
    } catch (e) {
      settings = prev;
      toasts.error(e);
    }
  }

  const scaleKeys = $derived(isMac ? '⌘⇧= / ⌘⇧− / ⌘⇧0' : 'Ctrl+Shift+= / − / 0');

  const THEMES: { id: ThemeId; label: () => string }[] = [
    { id: 'system', label: m.theme_system },
    { id: 'light', label: m.theme_light },
    { id: 'dark', label: m.theme_dark },
  ];
  const MINUTES: { min: number; label: () => string }[] = [
    { min: 5, label: () => m.det_minutes({ n: '5' }) },
    { min: 15, label: () => m.det_minutes({ n: '15' }) },
    { min: 30, label: () => m.det_minutes({ n: '30' }) },
    { min: 60, label: m.det_hour },
    { min: 0, label: m.det_never },
  ];
  const SCOPES: { id: LockScope; label: () => string }[] = [
    { id: 'volume', label: m.det_scope_volume },
    { id: 'all', label: m.det_scope_all },
  ];
  const d = $derived(settings?.defaults);
  const upPct = $derived(update.progress === null ? null : Math.round(update.progress * 100));
  const mounted = $derived(vaults.open.filter((v) => v.mountpoint));

  async function unmount(id: string) {
    await vaults.toggleMount(id);
  }

  const editable = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
  function keydown(e: KeyboardEvent) {
    if (document.querySelector('dialog[open]')) return;
    const back = e.key === 'Escape' || e.key === 'BrowserBack' || (e.altKey && e.key === 'ArrowLeft' && !editable(e.target));
    if (back) {
      e.preventDefault();
      app.closeSettings();
    }
  }
  function mouseup(e: MouseEvent) {
    if (e.button === 3 && !document.querySelector('dialog[open]')) {
      e.preventDefault();
      app.closeSettings();
    }
  }
</script>

<svelte:window onkeydown={keydown} onmouseup={mouseup} />

<main class="settings ground scroller" aria-labelledby="settings-title" data-testid="settings" bind:this={scroller}>
  <header class="bar">
    <button type="button" class="btn ghost back" onclick={() => app.closeSettings()} data-testid="settings-back">
      <ArrowLeft size={16} aria-hidden="true" /> {m.action_back()}
    </button>
    <div class="head">
      <p class="caps">{isMobile ? m.set_kicker_phone() : m.set_kicker()}</p>
      <h1 id="settings-title" class="title display">{m.set_title()}</h1>
    </div>
    <p class="hint shortcut">{m.set_shortcut({ keys: isMac ? '⌘,' : 'Ctrl+,' })}</p>
  </header>

  <div class="layout">
    <article class="pages sheet">
      <Stitch inset={6} />
      <Band height={9} />
      <div class="sections">
        <div class="col">
      <section class="block" aria-labelledby="set-app" data-testid="settings-app">
        <h2 id="set-app" class="block-title display"><span class="mark" aria-hidden="true"><Palette size={15} strokeWidth={1.6} /></span>{m.set_app()}</h2>
        <p class="hint">{m.set_app_hint()}</p>
        <h3 id="set-app-theme" class="sub caps">{m.set_theme()}</h3>
        <div class="segmented themes" role="radiogroup" aria-labelledby="set-app-theme" tabindex="-1" data-testid="app-theme" onkeydown={(e) => radioKeys(e, APP_THEMES.map((t) => t.id), appLook.theme, (v) => setApp({ app_theme: v }))}>
          {#each APP_THEMES as t (t.id)}
            <button type="button" role="radio" data-theme-id={t.id} aria-checked={appLook.theme === t.id} tabindex={appLook.theme === t.id ? 0 : -1} onclick={() => setApp({ app_theme: t.id })}>
              <span class="chip" data-chip={t.id} aria-hidden="true"></span>{t.label()}
            </button>
          {/each}
        </div>
        <h3 id="set-app-ink" class="sub caps">{m.set_ink()}</h3>
        <div class="swatches" role="radiogroup" aria-labelledby="set-app-ink" tabindex="-1" data-testid="app-accent" onkeydown={(e) => radioKeys(e, ACCENTS.map((a) => a.id), appLook.accent, (v) => setApp({ app_accent: v }))}>
          {#each ACCENTS as a (a.id)}
            <button type="button" role="radio" class="swatch" data-accent={a.id} aria-checked={appLook.accent === a.id} aria-label={a.name()} title={a.name()} tabindex={appLook.accent === a.id ? 0 : -1} onclick={() => setApp({ app_accent: a.id })}>
              <InkChip size={38} />
            </button>
          {/each}
        </div>
        {#if !isMobile}
        <h3 id="set-scale" class="sub caps">{m.set_scale()}</h3>
        <div class="segmented scales" role="radiogroup" aria-labelledby="set-scale" tabindex="-1" data-testid="app-scale" onkeydown={(e) => radioKeys(e, [...api.UI_SCALES], appLook.scale, (v) => setScale(v, e))}>
          {#each api.UI_SCALES as z (z)}
            <button type="button" role="radio" class="mono" data-scale={z} aria-label="{z}%" aria-checked={appLook.scale === z} tabindex={appLook.scale === z ? 0 : -1} style:--aa="{Math.round((z / 100) * 13)}px" onclick={(e) => setScale(z, e)}>{z}%</button>
          {/each}
        </div>
        <p class="hint">{m.set_scale_keys({ keys: scaleKeys })}</p>
        {/if}
      </section>

      <section class="block" aria-labelledby="set-startup" data-testid="settings-startup">
        <h2 id="set-startup" class="block-title display"><span class="mark" aria-hidden="true"><Power size={15} strokeWidth={1.6} /></span>{m.set_startup()}</h2>
        {#if info && !info.portable}
          <p class="hint">{m.set_startup_hint()}</p>
          <div class="switch-row" data-testid="startup">
            <span class="switch-label" id="set-at-login">{m.set_start_at_login()}</span>
            <button type="button" class="switch" role="switch" aria-checked={settings?.start_at_login ?? false} aria-labelledby="set-at-login" disabled={!settings} onclick={() => setStartup({ start_at_login: !settings?.start_at_login })} data-testid="start-at-login"><span class="knob"></span></button>
          </div>
          <div class="switch-row">
            <span class="switch-label" id="set-minimized">{m.set_start_minimized()}</span>
            <button type="button" class="switch" role="switch" aria-checked={settings?.start_minimized ?? false} aria-labelledby="set-minimized" aria-describedby="set-minimized-hint" disabled={!settings} onclick={() => setStartup({ start_minimized: !settings?.start_minimized })} data-testid="start-minimized"><span class="knob"></span></button>
          </div>
          <p class="hint" id="set-minimized-hint">{m.set_start_minimized_hint()}</p>
          <div class="switch-row">
            <span class="switch-label" id="set-close-tray">{m.set_close_to_tray()}</span>
            <button type="button" class="switch" role="switch" aria-checked={settings?.close_to_tray ?? false} aria-labelledby="set-close-tray" aria-describedby="set-close-tray-hint" disabled={!settings} onclick={() => setStartup({ close_to_tray: !settings?.close_to_tray })} data-testid="close-to-tray"><span class="knob"></span></button>
          </div>
          <p class="hint" id="set-close-tray-hint">{m.set_close_to_tray_hint()}</p>
        {:else if info}
          <p class="hint" data-testid="startup-portable">{m.set_startup_portable()}</p>
        {/if}
      </section>

      <section class="block" aria-labelledby="set-lang" data-testid="settings-language">
        <h2 id="set-lang" class="block-title display"><span class="mark" aria-hidden="true"><Languages size={15} strokeWidth={1.6} /></span>{m.set_language()}</h2>
        <div class="segmented" role="radiogroup" aria-labelledby="set-lang" tabindex="-1" onkeydown={(e) => radioKeys(e, LANGS.map((l) => l.id), i18n.choice, (v) => setLanguage(v))}>
          {#each LANGS as l (l.id)}
            <button type="button" role="radio" lang={l.id === 'system' ? undefined : l.id} data-lang={l.id} aria-checked={i18n.choice === l.id} tabindex={i18n.choice === l.id ? 0 : -1} onclick={() => setLanguage(l.id)}>{l.label()}</button>
          {/each}
        </div>
        {#if i18n.forced}<p class="hint">{m.set_lang_forced({ lang: i18n.forced })}</p>{/if}
      </section>

      <section class="block" aria-labelledby="set-drives" data-testid="settings-drives">
        <h2 id="set-drives" class="block-title display"><span class="mark" aria-hidden="true"><HardDrive size={15} strokeWidth={1.6} /></span>{m.set_drives()}</h2>
        <p class="hint">{m.set_drives_hint()}</p>
        {#if mounted.length === 0}
          <p class="none">{m.set_drives_none()}</p>
        {:else}
          <ul class="drives">
            {#each mounted as v (v.id)}
              <li class="drive">
                <HardDrive size={15} aria-hidden="true" />
                <span class="d-main"><span class="d-name">{v.name}</span><span class="mono d-point" title={v.mountpoint}>{v.mountpoint}</span></span>
                <button type="button" class="btn sm" aria-label={m.set_unmount_name({ name: v.name })} disabled={vaults.mountBusy} onclick={() => unmount(v.id)}>{m.set_unmount()}</button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section class="block" aria-labelledby="set-device" data-testid="settings-device">
        <h2 id="set-device" class="block-title display">{m.dev_entry()}</h2>
        <p class="hint">{m.dev_entry_hint()}</p>
        <button type="button" class="btn sm device-open" onclick={() => app.goto('device')} data-testid="settings-open-device">{m.dev_entry_short()}…</button>
      </section>

      <section class="block" aria-labelledby="set-pm" data-testid="settings-passwords">
        <h2 id="set-pm" class="block-title display"><span class="mark" aria-hidden="true"><KeyRound size={15} strokeWidth={1.6} /></span>{m.set_passwords()}</h2>
        <div class="switch-row">
          <span class="switch-label" id="set-icons">{m.set_site_icons()}</span>
          <button type="button" class="switch" role="switch" aria-checked={settings?.site_icons ?? true} aria-labelledby="set-icons" aria-describedby="set-icons-hint" disabled={!settings} onclick={() => setSiteIcons(!(settings?.site_icons ?? true))} data-testid="site-icons"><span class="knob"></span></button>
        </div>
        <p class="hint" id="set-icons-hint">{m.set_site_icons_hint()}</p>
      </section>

      {#if !isMobile}
        <section class="block" aria-labelledby="set-browser" data-testid="settings-browser">
          <h2 id="set-browser" class="block-title display"><span class="mark" aria-hidden="true"><Globe size={15} strokeWidth={1.6} /></span>{m.set_browser()}</h2>
          <div><BrowserSettings /></div>
        </section>
      {/if}

      {#if isMobile}
        <section class="block" aria-labelledby="set-gallery-t" data-testid="settings-gallery">
          <h2 id="set-gallery-t" class="block-title display">{m.gallery_action()}</h2>
          <div class="switch-row">
            <span class="switch-label" id="set-gallery">{m.set_gallery()}</span>
            <button type="button" class="switch" role="switch" aria-checked={settings?.gallery_save ?? true} aria-labelledby="set-gallery" aria-describedby="set-gallery-hint" disabled={!settings} onclick={() => setGallery(!(settings?.gallery_save ?? true))} data-testid="gallery-save"><span class="knob"></span></button>
          </div>
          <p class="hint" id="set-gallery-hint">{m.set_gallery_hint()}</p>
        </section>
      {/if}

      <section class="block" aria-labelledby="set-va" data-testid="settings-video-arrows">
        <h2 id="set-va" class="block-title display">{m.set_video_arrows()}</h2>
        <div class="segmented" role="radiogroup" aria-labelledby="set-va" tabindex="-1" data-testid="video-arrows" onkeydown={(e) => radioKeys(e, VIDEO_ARROWS, appLook.videoArrows, (v) => setApp({ video_arrows: v }))}>
          {#each VIDEO_ARROWS as v (v)}
            <button type="button" role="radio" data-value={v} aria-checked={appLook.videoArrows === v} tabindex={appLook.videoArrows === v ? 0 : -1} onclick={() => setApp({ video_arrows: v })}>{v === 'seek' ? m.set_video_arrows_seek() : m.set_video_arrows_step()}</button>
          {/each}
        </div>
      </section>

      <section class="block" aria-labelledby="set-snd" data-testid="settings-sounds">
        <h2 id="set-snd" class="block-title display">{m.set_sounds()}</h2>
        <div class="switch-row">
          <span class="switch-label" id="set-sounds">{m.set_sounds()}</span>
          <button type="button" class="switch" role="switch" aria-checked={appLook.sounds} aria-labelledby="set-sounds" aria-describedby="set-sounds-hint" data-testid="sounds" onclick={() => setApp({ sounds: !appLook.sounds })}><span class="knob"></span></button>
        </div>
        <p class="hint" id="set-sounds-hint">{m.set_sounds_hint()}</p>
      </section>

        </div>
        <div class="col">
      <section class="block" aria-labelledby="set-new" data-testid="settings-defaults">
        <h2 id="set-new" class="block-title display"><span class="mark" aria-hidden="true"><FilePlus2 size={15} strokeWidth={1.6} /></span>{m.set_new_volumes()}</h2>
        <p class="hint">{m.set_new_volumes_hint()}</p>
        <div class="switch-row">
          <span class="switch-label" id="set-like-app">{m.set_new_like_app()}</span>
          <button type="button" class="switch" role="switch" aria-checked={appLook.newLikeApp} aria-labelledby="set-like-app" aria-describedby="set-like-app-hint" data-testid="new-like-app" onclick={() => setApp({ new_like_app: !appLook.newLikeApp })}><span class="knob"></span></button>
        </div>
        <p class="hint" id="set-like-app-hint">{m.set_new_like_app_hint()}</p>
        {#if !appLook.newLikeApp}
        <h3 id="set-theme" class="sub caps">{m.set_theme()}</h3>
        <div class="segmented themes" role="radiogroup" aria-labelledby="set-theme" tabindex="-1" data-testid="default-theme" onkeydown={(e) => radioKeys(e, THEMES.map((t) => t.id), (d?.theme ?? 'dark') as ThemeId, (v) => setDefaults({ theme: v }))}>
          {#each THEMES as t (t.id)}
            <button type="button" role="radio" aria-checked={d?.theme === t.id} tabindex={d?.theme === t.id ? 0 : -1} disabled={!d} onclick={() => setDefaults({ theme: t.id })}>
              <span class="chip" data-chip={t.id} aria-hidden="true"></span>{t.label()}
            </button>
          {/each}
        </div>
        <h3 id="set-ink" class="sub caps">{m.set_ink()}</h3>
        <div class="swatches" role="radiogroup" aria-labelledby="set-ink" tabindex="-1" data-testid="default-accent" onkeydown={(e) => radioKeys(e, ACCENTS.map((a) => a.id), (d?.accent ?? 'velluto') as AccentId, (v) => setDefaults({ accent: v }))}>
          {#each ACCENTS as a (a.id)}
            <button type="button" role="radio" class="swatch" data-accent={a.id} aria-checked={d?.accent === a.id} aria-label={a.name()} title={a.name()} tabindex={d?.accent === a.id ? 0 : -1} disabled={!d} onclick={() => setDefaults({ accent: a.id })}>
              <InkChip size={38} />
            </button>
          {/each}
        </div>
        {/if}
        <h3 id="set-autolock" class="sub caps">{m.set_autolock()}</h3>
        <div class="segmented" role="radiogroup" aria-labelledby="set-autolock" tabindex="-1" data-testid="default-minutes" onkeydown={(e) => radioKeys(e, MINUTES.map((o) => o.min), d?.auto_lock_minutes ?? 15, (v) => setDefaults({ auto_lock_minutes: v }))}>
          {#each MINUTES as o (o.min)}
            <button type="button" role="radio" aria-checked={d?.auto_lock_minutes === o.min} tabindex={d?.auto_lock_minutes === o.min ? 0 : -1} disabled={!d} onclick={() => setDefaults({ auto_lock_minutes: o.min })}>{o.label()}</button>
          {/each}
        </div>
        <div class="switch-row">
          <span class="switch-label" id="set-sleep">{m.det_lock_sleep()}</span>
          <button type="button" class="switch" role="switch" aria-checked={d?.lock_on_sleep ?? true} aria-labelledby="set-sleep" disabled={!d} onclick={() => setDefaults({ lock_on_sleep: !d?.lock_on_sleep })}><span class="knob"></span></button>
        </div>
        <div class="switch-row">
          <span class="switch-label" id="set-screen">{m.det_lock_screen()}</span>
          <button type="button" class="switch" role="switch" aria-checked={d?.lock_on_screen_lock ?? true} aria-labelledby="set-screen" disabled={!d} onclick={() => setDefaults({ lock_on_screen_lock: !d?.lock_on_screen_lock })}><span class="knob"></span></button>
        </div>
        <h3 id="set-scope" class="sub caps">{m.det_scope()}</h3>
        <div class="segmented" role="radiogroup" aria-labelledby="set-scope" tabindex="-1" onkeydown={(e) => radioKeys(e, SCOPES.map((o) => o.id), (d?.lock_scope ?? 'volume') as LockScope, (v) => setDefaults({ lock_scope: v }))}>
          {#each SCOPES as o (o.id)}
            <button type="button" role="radio" aria-checked={d?.lock_scope === o.id} tabindex={d?.lock_scope === o.id ? 0 : -1} disabled={!d} onclick={() => setDefaults({ lock_scope: o.id })}>{o.label()}</button>
          {/each}
        </div>
      </section>

      <section class="block" aria-labelledby="set-updates" data-testid="settings-updates">
        <h2 id="set-updates" class="block-title display"><span class="mark" aria-hidden="true"><RefreshCw size={15} strokeWidth={1.6} /></span>{m.set_updates()}</h2>
        <p class="hint">{m.set_updates_hint()}</p>
        <div class="update-status" aria-live="polite" data-testid="update-status" data-phase={update.phase}>
          {#if update.release && update.phase !== 'up_to_date'}
            <p class="u-title">{m.upd_available({ version: update.release.version })}</p>
            {#if update.phase === 'available'}
              <p class="u-notes">{update.release.notes[i18n.current] || update.release.notes.en || update.release.notes.it}</p>
            {/if}
          {/if}
          {#if update.phase === 'downloading'}
            <p class="u-line">{upPct === null ? m.upd_downloading() : m.upd_downloading_pct({ pct: String(upPct) })}</p>
            <progress max="100" value={upPct ?? undefined}></progress>
          {:else if update.phase === 'installing'}
            <p class="u-line">{m.upd_installing()}</p>
          {:else if update.phase === 'ready'}
            <p class="u-line">{m.upd_ready()}</p>
          {:else if update.phase === 'saved' && update.outcome}
            <p class="u-line">{m.upd_saved()}</p>
            <p class="u-path mono">{update.outcome.path}</p>
          {:else if update.phase === 'up_to_date'}
            <p class="u-line">{m.upd_up_to_date()}</p>
          {:else if update.phase === 'error'}
            <p class="u-line error" role="alert">{update.error}</p>
          {/if}
        </div>
        <div class="u-actions">
          {#if update.phase === 'available'}
            <button type="button" class="btn primary" onclick={() => update.install()} data-testid="install-update">{m.upd_install()}</button>
          {:else if update.phase === 'ready'}
            <button type="button" class="btn primary" onclick={() => update.restart()} data-testid="restart-update">{m.upd_restart()}</button>
          {/if}
          <button type="button" class="btn" disabled={update.busy} onclick={() => update.check()} data-testid="check-updates">
            <RefreshCw size={15} aria-hidden="true" /> {update.phase === 'checking' ? m.upd_checking() : m.set_check_updates()}
          </button>
        </div>
        <div class="switch-row">
          <span class="switch-label" id="set-update-start">{m.upd_check_on_start()}</span>
          <button
            type="button"
            class="switch"
            role="switch"
            aria-checked={update.prefs?.check_on_start ?? false}
            aria-labelledby="set-update-start"
            disabled={!update.prefs}
            onclick={() => update.setCheckOnStart(!update.prefs?.check_on_start)}
            data-testid="update-on-start"><span class="knob"></span></button>
        </div>
        {#if update.prefs?.portable}
          <p class="hint">{m.upd_check_on_start_portable()}</p>
        {/if}
      </section>

      <section class="block" aria-labelledby="set-about" data-testid="settings-about">
        <h2 id="set-about" class="block-title display"><span class="mark" aria-hidden="true"><Info size={15} strokeWidth={1.6} /></span>{m.set_about()}</h2>
        <dl class="facts">
          <div><dt class="caps">{m.set_version()}</dt><dd class="mono" data-testid="app-version">{info?.version ?? '—'}</dd></div>
          <div><dt class="caps">{m.set_mode()}</dt><dd>{info ? (info.portable ? m.set_mode_portable() : m.set_mode_installed()) : '—'}</dd></div>
        </dl>
        {#if settings}
          <p class="hint" data-testid="settings-storage">
            {settings.persisted && info?.config_dir ? m.set_storage_installed({ dir: info.config_dir }) : m.set_storage_portable()}
          </p>
        {/if}
        <button type="button" class="btn sm licences" onclick={() => (licences = true)} data-testid="open-licences"><ScrollText size={14} aria-hidden="true" /> {m.set_licences()}</button>
      </section>
        </div>
      </div>
    </article>
  </div>

  <footer class="imprint"><Brand /></footer>
  <Maker />
</main>

<Dialog open={licences} title={m.set_licences()} variant="wide" onclose={() => (licences = false)}>
  <p class="hint">{m.set_licences_hint()}</p>
  <pre class="sources" data-testid="licences-text">{sources}</pre>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (licences = false)}>{m.action_close()}</button>
  {/snippet}
</Dialog>

<style>
  .settings {
    position: relative;
    z-index: 2;
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    scroll-padding-top: var(--space-5);
  }
  .bar {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: end;
    gap: var(--space-5);
    max-width: var(--page-max);
    margin: 0 auto;
    padding: var(--space-5) var(--space-6) var(--space-4);
  }
  .settings {
    --page-max: 820px;
  }
  @media (min-width: 1440px) {
    .settings {
      --page-max: 1360px;
    }
  }
  .back {
    align-self: center;
    justify-self: start;
  }
  .title {
    margin-top: 6px;
    font-size: clamp(34px, 3.4vw, 46px);
    font-weight: 500;
    line-height: 1.05;
    color: var(--title-ink);
  }
  .shortcut {
    margin-bottom: 6px;
  }
  .layout {
    max-width: var(--page-max);
    margin: 0 auto;
    padding: 0 var(--space-6) var(--space-6);
  }
  .pages {
    --bleed-start: var(--space-6);
    --bleed-end: var(--space-6);
    min-width: 0;
    padding: 18px var(--space-6) var(--space-3);
    border-radius: var(--radius-stitched);
    box-shadow: var(--shadow-sheet);
  }
  .sections {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
  }
  .col + .col {
    border-top: 1px solid var(--paper-rule);
  }
  @media (min-width: 1440px) {
    .sections {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      column-gap: var(--space-7);
    }
    .col + .col {
      border-top: 0;
      border-left: 1px solid var(--paper-rule);
      margin-left: calc(var(--space-7) / -2);
      padding-left: calc(var(--space-7) / 2);
    }
  }
  @media (max-width: 1000px) {
    .bar {
      grid-template-columns: auto minmax(0, 1fr);
      padding-inline: var(--space-5);
    }
    .shortcut {
      display: none;
    }
    .layout {
      padding-inline: var(--space-5);
    }
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-5) 0;
    border-bottom: 1px solid var(--paper-rule);
  }
  .col > .block:last-child {
    border-bottom: 0;
  }
  .block-title {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: var(--display-s);
    font-weight: 500;
    color: var(--ink);
  }
  .mark {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .block > :not(.block-title) {
    margin-left: 42px;
  }
  .sub {
    margin-top: var(--space-3);
  }
  .block > .btn {
    align-self: flex-start;
  }
  .segmented.themes [role='radio'] {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .scales [role='radio'] {
    display: inline-block;
    height: 34px;
    line-height: 34px;
    min-width: 4.8em;
    text-align: center;
    white-space: nowrap;
  }
  .scales [role='radio']::before {
    content: 'A';
    margin-right: 5px;
    font-family: var(--font-display);
    font-size: var(--aa);
    font-weight: 500;
    line-height: 0;
    vertical-align: baseline;
    color: var(--ink-3);
  }
  .scales [role='radio'][aria-checked='true']::before {
    color: var(--accent);
  }
  .chip {
    flex: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    box-shadow:
      inset 0 0 0 1px rgb(0 0 0 / 0.28),
      inset 1px 1px 1px rgb(255 255 255 / 0.18);
  }
  .chip[data-chip='dark'] {
    background: radial-gradient(circle at 30% 30%, #3a3836, #1c1b1a 72%);
  }
  .chip[data-chip='light'] {
    background: radial-gradient(circle at 30% 30%, #fffdf8, #e9e2d5 72%);
  }
  .chip[data-chip='system'] {
    background: linear-gradient(135deg, #f6f1e7 50%, #262523 50%);
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .swatch {
    position: relative;
    display: grid;
    padding: 3px;
    border: 0;
    border-radius: 12px;
    background: none;
  }
  .swatch:hover :global(.ink-chip) {
    transform: translateY(-1px);
  }
  .swatch:active :global(.ink-chip) {
    transform: translateY(1px);
  }
  .swatch[aria-checked='true'] {
    box-shadow:
      0 0 0 2px var(--accent);
  }
  .swatch:focus-visible {
    outline-offset: 2px;
  }
  .none {
    font-size: var(--text-base);
    color: var(--ink-2);
  }
  .drives {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--paper-rule-2);
  }
  .drive {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 10px 2px;
    border-bottom: 1px solid var(--paper-rule);
  }
  .drive :global(svg) {
    color: var(--accent);
    flex: none;
  }
  .d-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .d-name {
    font-weight: 600;
  }
  .d-point {
    font-size: var(--text-xs);
    color: var(--ink-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    margin: 0;
  }
  .facts dd {
    margin: 4px 0 0;
    font-weight: 500;
    color: var(--ink);
  }
  .update-status {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .update-status:empty {
    display: none;
  }
  .u-title {
    font-weight: 600;
    color: var(--ink);
  }
  .u-notes {
    white-space: pre-line;
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .u-line {
    font-size: var(--text-base);
    color: var(--ink-2);
  }
  .u-line.error {
    color: var(--danger);
  }
  .u-path {
    font-size: var(--text-xs);
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .u-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .licences {
    margin-top: var(--space-1);
  }
  .sources {
    max-height: 52vh;
    overflow: auto;
    margin: 0;
    padding: var(--space-3);
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    color: var(--ink-2);
  }
  .imprint {
    display: flex;
    justify-content: center;
    align-items: center;
    gap: var(--space-3);
    padding: 0 0 var(--space-4);
  }
</style>
