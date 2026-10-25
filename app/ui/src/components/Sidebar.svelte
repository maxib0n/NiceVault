<!--
  The open passport's cover, seen from inside: its lining (anthracite suede with a
  breath of the volume's ink; book cloth in the ink on paper), the emblem and name
  stamped in foil. On it: the open volumes, the two sections, the system-drive
  switch, the auto-lock countdown and Blocca (the clasp).
-->
<script lang="ts">
  import UpdateNotice from './UpdateNotice.svelte';
  import StickNotice from './device/StickNotice.svelte';
  import { Files, HardDrive, KeyRound, Lock, PanelLeftClose, PanelLeftOpen, Plus, Settings, SlidersHorizontal } from '@lucide/svelte';
  import { sidebar } from '../lib/stores/sidebar.svelte';
  import { isMobile } from '../lib/platform';
  import { m } from '../paraglide/messages.js';
  import { backendReason, nativeBackend } from '../lib/backends';
  import MountLink from './MountLink.svelte';
  import { formatCountdown } from '../lib/format';
  import { app } from '../lib/stores/app.svelte';
  import { idle } from '../lib/stores/idle.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import Emblem from './Emblem.svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import { placeOf } from '../ornament/document';

  const active = $derived(vaults.active);
  const mounted = $derived(!!active?.mountpoint);
  const mountReason = $derived(
    vaults.backends.length === 0
      ? null
      : vaults.canMount
        ? null
        : ((nativeBackend(vaults.backends) && backendReason(nativeBackend(vaults.backends)!)) ?? m.mount_not_here()),
  );
  const mountLink = $derived(vaults.canMount ? null : (nativeBackend(vaults.backends)?.link ?? null));
  const showCountdown = $derived(idle.timeoutMs > 0 && Number.isFinite(idle.remainingMs));
  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
  // short windows: the stamped seal steps down so the cover rarely needs to scroll
  let winH = $state(800);
  const idSize = $derived(winH < 700 ? 64 : 92);
  const isMobileShell = isMobile;
  const modB = isMac ? '⌘B' : 'Ctrl+B';
  const toggleLabel = $derived(sidebar.rail ? m.sidebar_expand() : m.sidebar_collapse());
</script>

<svelte:window bind:innerHeight={winH} />

<aside class="sidebar cloth lining" aria-label={m.sidebar_label()} data-rail={sidebar.rail ? '' : undefined}>
  {#if !isMobileShell}
    <button type="button" class="fold" data-testid="sidebar-toggle" aria-expanded={!sidebar.rail} aria-label={toggleLabel} title="{toggleLabel} ({modB})" onclick={() => sidebar.toggle()}>
      {#if sidebar.rail}<PanelLeftOpen size={15} aria-hidden="true" />{:else}<PanelLeftClose size={15} aria-hidden="true" />{/if}
    </button>
  {/if}
  {#if sidebar.rail}
    <!-- the cover folded to its spine: the seal, the three sections, the clasp, Impostazioni -->
    <div class="rail nv-enter">
      {#if active}
        <button type="button" class="rail-seal" title={active.name} aria-label={active.name} onclick={() => sidebar.toggle()}>
          <Rosette size={44} kind="cover" finish="foil" frame={active.emblem_frame}>
            <Emblem emblem={active.emblem} name={active.name} size={17} finish="foil" decorative />
          </Rosette>
        </button>
        {#if vaults.open.length > 1}
          <ul class="rail-vaults" aria-label={m.sidebar_open_vaults()}>
            {#each vaults.open as v (v.id)}
              <li>
                <button type="button" class="rail-btn" class:active={v.id === vaults.activeId} aria-current={v.id === vaults.activeId ? 'true' : undefined} aria-label={v.name} title={v.name} onclick={() => vaults.activate(v.id)}>
                  <Emblem emblem={v.emblem} name={v.name} size={18} finish="foil" decorative />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        <nav class="rail-nav" aria-label={m.sidebar_sections()}>
          <button type="button" class="rail-btn" data-testid="nav-files" aria-label={m.nav_files()} title={m.nav_files()} aria-current={app.page === 'vault' ? 'page' : undefined} onclick={() => app.goto('vault')}><Files size={18} aria-hidden="true" /></button>
          <button type="button" class="rail-btn" data-testid="nav-passwords" aria-label={m.nav_passwords()} title={m.nav_passwords()} aria-current={app.page === 'passwords' ? 'page' : undefined} onclick={() => app.goto('passwords')}><KeyRound size={18} aria-hidden="true" /></button>
          <button type="button" class="rail-btn" data-testid="nav-details" aria-label={m.nav_details()} title={m.nav_details()} aria-current={app.page === 'details' ? 'page' : undefined} onclick={() => app.goto('details')}><SlidersHorizontal size={18} aria-hidden="true" /></button>
        </nav>
        <div class="rail-bottom">
          {#if showCountdown}<span class="rail-readout" title={m.autolock_in()}>{formatCountdown(idle.remainingMs)}</span>{/if}
          <button type="button" class="btn lock rail-lock" aria-label={m.lock()} title={m.lock()} onclick={() => vaults.lock()}><Lock size={16} aria-hidden="true" /></button>
          <button type="button" class="rail-btn" data-testid="open-settings" aria-label={m.app_settings()} title="{m.app_settings()} ({isMac ? '⌘,' : 'Ctrl+,'})" onclick={() => app.openSettings()}><Settings size={17} aria-hidden="true" /></button>
        </div>
      {/if}
    </div>
  {:else}
  {#if active}
    <div class="identity">
      <Rosette size={idSize} kind="cover" finish="foil" frame={active.emblem_frame}>
        <Emblem emblem={active.emblem} name={active.name} size={34} finish="foil" decorative />
      </Rosette>
      <p class="id-name display foil" title={active.name} aria-hidden="true">{active.name}</p>
      {#if placeOf(active.path, active.name)}<p class="id-place foil fine" aria-hidden="true">{placeOf(active.path, active.name)}</p>{/if}
    </div>
  {/if}

  <section class="vault-list" aria-labelledby="open-vaults-title">
    <h2 class="sidebar-title" id="open-vaults-title">{m.sidebar_open_vaults()}</h2>
    <ul>
      {#each vaults.open as v (v.id)}
        <li>
          <button
            type="button"
            class="row vault-btn"
            class:active={v.id === vaults.activeId}
            aria-current={v.id === vaults.activeId ? 'true' : undefined}
            onclick={() => vaults.activate(v.id)}
          >
            <Emblem emblem={v.emblem} name={v.name} size={18} finish="foil" decorative />
            <span class="row-name">{v.name}</span>
            {#if v.mountpoint}<span class="mounted-glyph" title={m.sidebar_mounted()}><HardDrive size={14} aria-label={m.sidebar_mounted_short()} /></span>{/if}
          </button>
        </li>
      {/each}
    </ul>
    <button type="button" class="btn ghost sm add" onclick={() => app.goto('welcome')}>
      <Plus size={14} aria-hidden="true" /> {m.sidebar_open_another()}
    </button>
  </section>

  {#if active}
    <nav class="nav" aria-label={m.sidebar_sections()}>
      <button type="button" class="row nav-btn" data-testid="nav-files" aria-current={app.page === 'vault' ? 'page' : undefined} onclick={() => app.goto('vault')}>
        <Files size={16} aria-hidden="true" /> {m.nav_files()}
      </button>
      <button type="button" class="row nav-btn" data-testid="nav-passwords" aria-current={app.page === 'passwords' ? 'page' : undefined} onclick={() => app.goto('passwords')}>
        <KeyRound size={16} aria-hidden="true" /> {m.nav_passwords()}
      </button>
      <button type="button" class="row nav-btn" data-testid="nav-details" aria-current={app.page === 'details' ? 'page' : undefined} onclick={() => app.goto('details')}>
        <SlidersHorizontal size={16} aria-hidden="true" /> {m.nav_details()}
      </button>
    </nav>

    <section class="mount" aria-labelledby="mount-label" data-state={mounted ? 'mounted' : vaults.canMount ? 'unmounted' : 'unavailable'}>
      <div class="mount-row">
        <span id="mount-label" class="mount-label">{m.mount_title()}</span>
        <button
          type="button"
          class="switch"
          role="switch"
          aria-checked={mounted}
          aria-labelledby="mount-label"
          aria-describedby="mount-desc"
          disabled={!vaults.canMount || vaults.mountBusy}
          onclick={() => vaults.toggleMount()}
        >
          <span class="knob"></span>
        </button>
      </div>
      <p class="mount-desc" id="mount-desc">
        {#if vaults.mountBusy}
          {mounted ? m.mount_unmounting() : m.mount_mounting()}
        {:else if mounted}
          <span class="mono path" title={active.mountpoint}>{#each (active.mountpoint ?? '').split(/(?<=[\\/])/) as part, i (i)}{#if i > 0}<wbr />{/if}{part}{/each}</span>
        {:else if mountReason}
          {mountReason}
        {:else}
          {m.mount_off()}
        {/if}
      </p>
      {#if mountLink && !mounted && !vaults.mountBusy}<MountLink url={mountLink} />{/if}
    </section>

    <div class="bottom">
      <UpdateNotice on="cloth" />
      <StickNotice on="cloth" />
      {#if showCountdown}
        <p class="autolock" aria-live="off">
          <span class="autolock-label">{m.autolock_in()}</span>
          <span class="readout">{formatCountdown(idle.remainingMs)}</span>
        </p>
      {/if}
      <div class="clasp">
        <button type="button" class="btn lock" onclick={() => vaults.lock()}>
          <Lock size={15} aria-hidden="true" /> {m.lock()}
        </button>
      </div>
      <!-- this computer's settings, not the volume's: a quiet line under the clasp -->
      <button type="button" class="row settings-row" title="{m.app_settings()} ({isMac ? '⌘,' : 'Ctrl+,'})" data-testid="open-settings" onclick={() => app.openSettings()}>
        <Settings size={15} aria-hidden="true" />
        <span class="row-name">{m.app_settings()}</span>
        <kbd class="keys">{isMac ? '⌘,' : 'Ctrl+,'}</kbd>
      </button>
    </div>
  {/if}
  {/if}
</aside>

<style>
  .sidebar {
    width: var(--sidebar-w);
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    /* symmetric about the visible cover: the stitched seam runs 8px inside the right edge
       (VaultScreen .seam), so the right side keeps those 8px more */
    padding: var(--space-5) 22px 14px 14px;
    overflow-y: auto;
    z-index: 2;
    /* the board's cut edge catches the light; the page curls away into the binding */
    box-shadow:
      inset -1px 0 0 rgb(255 255 255 / 0.1),
      inset -2px 0 0 var(--cloth-edge),
      3px 0 6px -2px rgb(0 0 0 / 0.32);
  }

  /* ---- the stamped identity -------------------------------------------- */
  .identity {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 2px 0 2px;
  }
  .id-name {
    max-width: 100%;
    margin-top: calc(12px - 0.12em);
    margin-bottom: -0.16em;
    font-size: 24px;
    line-height: 1.08;
    overflow-wrap: break-word;
    text-wrap: balance;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    /* Bodoni's ink reaches past its line box (descenders 0.27 em under the baseline, accents
       0.99 em over it): the clamp's box (and the foil, painted inside it) gets that room as padding, given back as margin */
    padding-block: 0.12em 0.16em;
  }
  .id-place {
    margin-top: 7px;
    font-family: var(--font-mono);
    font-size: 9.5px;
    font-weight: 500;
    letter-spacing: 0.22em;
    text-transform: uppercase;
  }

  .sidebar-title {
    padding: 0 8px;
    margin-bottom: 6px;
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    font-weight: 500;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--cloth-ink-3);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* rows: plain cloth at rest, a lighter nap under the pointer; the current one is
     pressed into the cloth (a debossed bed) */
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 34px;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: var(--cloth-ink-2);
    font-size: var(--text-md);
    font-weight: 500;
    text-align: left;
  }
  .row :global(svg) {
    color: var(--cloth-ink-3);
  }
  .row:hover {
    background: color-mix(in oklab, var(--cloth-ink) 8%, transparent);
    color: var(--cloth-ink);
  }
  .row:active,
  .nav-btn[aria-current='page'] {
    background: var(--cloth-bed);
    box-shadow:
      inset 0 1px 3px rgb(0 0 0 / 0.42),
      inset 0 0 0 1px rgb(0 0 0 / 0.12),
      0 1px 0 rgb(255 255 255 / 0.08);
  }
  .vault-btn.active,
  .nav-btn[aria-current='page'] {
    color: var(--cloth-ink);
    font-weight: 600;
  }
  /* the volume on the page: a lighter nap of the cloth (no tick in the margin) */
  .vault-btn.active {
    background: color-mix(in oklab, var(--cloth-ink) 7%, transparent);
  }
  .nav-btn[aria-current='page'] :global(svg) {
    color: var(--foil);
  }
  .row:focus-visible {
    outline-offset: -2px;
  }
  .row-name {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    line-height: 1.3;
  }
  .mounted-glyph {
    display: inline-flex;
  }
  .add {
    width: 100%;
    justify-content: flex-start;
    margin-top: 4px;
    padding: 0 8px;
    min-height: 30px;
  }
  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* ---- system drive: label, switch, one line of truth -------------------- */
  .mount {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 0 8px;
  }
  .mount-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .mount-label {
    font-size: var(--text-md);
    font-weight: 500;
    color: var(--cloth-ink);
  }
  .mount-desc {
    font-size: var(--text-sm);
    line-height: 1.4;
    color: var(--cloth-ink-3);
    overflow-wrap: anywhere;
  }
  /* a path breaks after its separators, a segment only when it alone is wider than the column */
  .mount-desc .path {
    overflow-wrap: break-word;
  }
  .mount[data-state='mounted'] .mount-desc {
    color: var(--cloth-ink-2);
  }

  /* the switch: a groove pressed into the cloth, an ivory knob; foil-lined when on */
  .switch {
    position: relative;
    flex: none;
    width: 38px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-pill);
    background: var(--cloth-bed);
    box-shadow:
      inset 0 1px 3px rgb(0 0 0 / 0.55),
      inset 0 0 0 1px rgb(0 0 0 / 0.2),
      0 1px 0 rgb(255 255 255 / 0.09);
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: linear-gradient(180deg, #fffaf2, #e9e0d2);
    box-shadow:
      0 1px 2px rgb(0 0 0 / 0.45),
      inset 0 -1px 0 rgb(0 0 0 / 0.08);
    /* styles/motion.css: slides on a registered property, not on its transform */
    translate: var(--nv-dx) 0;
    transition: --nv-dx var(--dur-micro) var(--ease-out);
  }
  .switch:hover:not(:disabled) .knob {
    box-shadow:
      0 2px 4px rgb(0 0 0 / 0.5),
      inset 0 -1px 0 rgb(0 0 0 / 0.08);
  }
  .switch[aria-checked='true'] {
    background: linear-gradient(180deg, var(--foil-mid), var(--foil) 60%, var(--foil-hi));
    box-shadow:
      inset 0 1px 3px rgb(0 0 0 / 0.4),
      inset 0 0 0 1px rgb(0 0 0 / 0.18),
      0 1px 0 rgb(255 255 255 / 0.09);
  }
  .switch[aria-checked='true'] .knob {
    --nv-dx: 16px;
  }
  .switch:disabled {
    opacity: 0.55;
  }
  .switch:disabled .knob {
    background: color-mix(in oklab, var(--cloth-ink) 55%, var(--cloth));
    box-shadow: 0 1px 1px rgb(0 0 0 / 0.3);
  }

  /* ---- the clasp ---------------------------------------------------------- */
  .bottom {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 6px 0;
    border-top: 1px solid rgb(0 0 0 / 0.22);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.06);
  }
  .autolock {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--cloth-ink-3);
  }
  /* the real countdown, set in mono */
  .readout {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--cloth-ink);
  }
  .clasp {
    display: flex;
  }
  .lock {
    flex: 1;
  }
  .settings-row {
    min-height: 30px;
    padding: 4px 8px;
    margin-bottom: -4px;
    font-size: var(--text-sm);
  }
  .keys {
    margin-left: auto;
    font-size: var(--text-2xs);
    color: var(--cloth-ink-3);
    opacity: 0.85;
  }
  /* ---- folded to a rail (Ctrl/Cmd+B; desktop) ------------------------------
     The cover keeps its full width and slides under the window's left edge until only its
     spine shows (a transform: nothing is laid out again while it moves; VaultScreen slides the
     page and the seam with it). Its own --nv-dx (styles/motion.css), never the compositor's. */
  .sidebar {
    position: relative;
    translate: var(--nv-dx) 0;
    transition: --nv-dx var(--dur-rail, 280ms) var(--ease-out);
  }
  .sidebar[data-rail] {
    width: var(--sidebar-full);
    margin-right: calc(var(--sidebar-w) - var(--sidebar-full));
    padding: 12px 18px 12px calc(var(--sidebar-full) - var(--sidebar-w) + 6px);
    gap: 10px;
    align-items: center;
    --nv-dx: calc(var(--sidebar-w) - var(--sidebar-full));
  }
  .fold {
    position: absolute;
    top: 10px;
    right: 18px;
    z-index: 1;
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: var(--cloth-ink-3);
  }
  .fold:hover {
    background: color-mix(in oklab, var(--cloth-ink) 8%, transparent);
    color: var(--cloth-ink);
  }
  .fold:focus-visible {
    outline-offset: -2px;
  }
  .sidebar[data-rail] .fold {
    position: static;
    flex: none;
  }
  .rail {
    flex: 1;
    min-height: 0;
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    animation: nv-enter var(--dur-rail, 280ms) var(--ease-out) backwards;
    --enter-from: translateX(-6px);
    --enter-to: translateX(0);
  }
  .rail-seal {
    padding: 0;
    border: 0;
    background: none;
    border-radius: 50%;
  }
  .rail-vaults,
  .rail-nav,
  .rail-bottom {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }
  .rail-vaults {
    padding-top: 8px;
    border-top: 1px solid rgb(0 0 0 / 0.22);
  }
  .rail-btn {
    display: inline-grid;
    place-items: center;
    width: 40px;
    height: 38px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: var(--cloth-ink-3);
  }
  .rail-btn:hover {
    background: color-mix(in oklab, var(--cloth-ink) 8%, transparent);
    color: var(--cloth-ink);
  }
  .rail-btn:active,
  .rail-btn[aria-current='page'],
  .rail-btn[aria-current='true'] {
    background: var(--cloth-bed);
    box-shadow:
      inset 0 1px 3px rgb(0 0 0 / 0.42),
      inset 0 0 0 1px rgb(0 0 0 / 0.12),
      0 1px 0 rgb(255 255 255 / 0.08);
  }
  .rail-btn[aria-current='page'] {
    color: var(--foil);
  }
  .rail-btn:focus-visible {
    outline-offset: -2px;
  }
  .rail-bottom {
    margin-top: auto;
    width: 100%;
    padding-top: 10px;
    gap: 8px;
    border-top: 1px solid rgb(0 0 0 / 0.22);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.06);
  }
  .rail-lock {
    width: 40px;
    min-width: 0;
    padding: 0;
    justify-content: center;
  }
  .rail-readout {
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    color: var(--cloth-ink-2);
  }
  /* the phone draws its own flap / rail */
  :global(:root[data-platform='mobile']) .fold {
    display: none;
  }
</style>
