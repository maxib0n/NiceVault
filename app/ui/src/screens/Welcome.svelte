<!--
  Benvenuto: the volumes found here, as passports lying on the board, each bound
  in its own dyed suede (cloth on paper) when its look is visible while locked,
  saddle-stitched and stamped in foil; a volume that keeps its look encrypted is
  bound in plain anthracite with a neutral rosette. Under each, where it was found.
  Open one, open a file, or issue a new one. An open volume carries a silk ribbon;
  an unreachable one is only the impression it left in the board.
-->
<script lang="ts">
  import { FolderOpen, HardDrive, History, Monitor, Plus, Settings, Usb, X } from '@lucide/svelte';
  import { m } from '../paraglide/messages.js';
  import * as api from '../lib/api';
  import { formatAgo, formatBytes } from '../lib/format';
  import { app } from '../lib/stores/app.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import type { RecentEntry } from '../lib/types';
  import { lockedAccent, lockedFrame, lockedView } from '../lib/emblem';
  import Emblem from '../components/Emblem.svelte';
  import Brand from '../components/Brand.svelte';
  import UpdateNotice from '../components/UpdateNotice.svelte';
  import StickNotice from '../components/device/StickNotice.svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import Stitch from '../ornament/Stitch.svelte';
  import { placeOf } from '../ornament/document';
  import { decode } from '../lib/reveal';

  function choose(r: RecentEntry) {
    const open = vaults.open.find((v) => v.path === r.path);
    if (open) vaults.activate(open.id);
    else {
      vaults.resetUnlock();
      app.showUnlock(r);
    }
  }

  async function openOther() {
    const path = await api.pickVolumeFile();
    if (!path) return;
    const open = vaults.open.find((v) => v.path === path);
    if (open) vaults.activate(open.id);
    else {
      vaults.resetUnlock();
      app.showUnlockPath(path);
    }
  }

  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
  const isOpen = (r: RecentEntry) => vaults.open.some((v) => v.path === r.path);
  /** the frame round the emblem: the open volume's own, else what this computer last saw */
  const frameFor = (r: RecentEntry) => {
    const v = vaults.open.find((x) => x.path === r.path);
    return v && v.show_when_locked !== false ? v.emblem_frame : lockedFrame(r);
  };

  // where the scan found it (RecentEntry.source)
  const SOURCE = {
    here: { label: m.source_here, icon: HardDrive },
    removable: { label: m.source_removable, icon: Usb },
    known: { label: m.source_known, icon: Monitor },
    session: { label: m.source_session, icon: History },
  } as const;

  // the books grow a little with the window (the board is a table, not a list)
  let winW = $state(1280);
  // a phone (styles/mobile.css: the passports are a deck you thumb through) holds one book large in the middle
  const bookW = $derived(winW <= 640 ? Math.round(Math.min(236, winW * 0.58)) : Math.round(Math.min(252, Math.max(158, winW * 0.148))));
  const empty = $derived(app.recentsLoaded && app.recents.length === 0);
</script>

<svelte:window bind:innerWidth={winW} />

<!-- on app start the page decodes from glyphs, once per session: the covers assemble, their foil intact -->
<main
  class="welcome ground scroller"
  class:is-empty={empty}
  aria-labelledby="welcome-title"
  style:--book-w="{bookW}px"
  use:decode={{ key: 'start:welcome', ready: app.recentsLoaded, speed: 0.95, surfaces: '.cover, .slot', fades: '.actions .btn, .forget, .where svg' }}
>
  <header class="masthead">
    <Brand />
    <button type="button" class="btn ghost sm settings" title="{m.app_settings()} ({isMac ? '⌘,' : 'Ctrl+,'})" data-testid="open-settings" onclick={() => app.openSettings()}>
      <Settings size={15} aria-hidden="true" /> {m.app_settings()}
    </button>
  </header>

  <section class="intro">
    <div class="intro-text">
      <h1 id="welcome-title" class="title display">{m.welcome_title_pre()} <em>{m.welcome_title_em()}</em></h1>
      <p class="sub">{empty ? m.welcome_sub_empty() : m.welcome_sub()}</p>
    </div>
    <div class="actions">
      <button type="button" class="btn ghost" onclick={() => app.goto('device')} data-testid="open-device">
        <Usb size={16} aria-hidden="true" /> {m.dev_entry_short()}
      </button>
      <button type="button" class="btn" onclick={openOther}>
        <FolderOpen size={16} aria-hidden="true" /> {m.welcome_open()}
      </button>
      <button type="button" class="btn primary" onclick={() => app.goto('create')}>
        <Plus size={16} aria-hidden="true" /> {m.welcome_new()}
      </button>
    </div>
  </section>

  <section class="shelf" aria-labelledby="recents-title">
    <h2 id="recents-title" class="visually-hidden">{m.welcome_recents()}</h2>
    {#if empty}
      <div class="empty" data-state="empty">
        <!-- the place where the first volume will lie: pressing it makes one -->
        <button type="button" class="slot" data-testid="welcome-first" onclick={() => app.goto('create')}>
          <Rosette size={86} kind="cover" finish="intaglio" />
          <span class="slot-label">{m.welcome_first()}</span>
        </button>
      </div>
    {:else}
      <ul class="passports">
        {#each app.recents as r, i (r.path)}
          {@const open = isOpen(r)}
          {@const place = placeOf(r.path, r.name)}
          {@const where = r.source ? SOURCE[r.source] : null}
          <li
            class="passport"
            data-testid="passport"
            data-name={r.name}
            class:missing={!r.exists}
            class:open
            class:neutral={r.exists && r.show_when_locked === false}
            data-accent={lockedAccent(r) ?? undefined}
          >
            <button
              type="button"
              class="book"
              disabled={!r.exists}
              onclick={() => choose(r)}
              aria-describedby={where ? `recent-meta-${i} recent-where-${i}` : `recent-meta-${i}`}
              title="{r.name} · {r.path}"
            >
              <span class="state" role="img" aria-label={open ? m.welcome_state_open() : m.welcome_state_closed()}></span>
              {#if open}
                <!-- the bookmark: a silk ribbon from inside the book, under the cover; only its
                     tail shows below the boards, in the cover's shadow where it comes out -->
                <svg class="ribbon" viewBox="0 0 12 48" aria-hidden="true" data-testid="ribbon">
                  <defs>
                    <linearGradient id="silk-{i}" x1="0" x2="1" y1="0" y2="0.18">
                      <stop offset="0" style="stop-color: var(--foil-mid)" />
                      <stop offset="0.38" style="stop-color: var(--foil-hi)" />
                      <stop offset="0.62" style="stop-color: var(--foil)" />
                      <stop offset="1" style="stop-color: var(--foil-lo, #9c7c3c)" />
                    </linearGradient>
                    <linearGradient id="edge-{i}" x1="0" x2="0" y1="0" y2="1">
                      <stop offset="0.56" stop-color="#000" stop-opacity="0.5" />
                      <stop offset="0.64" stop-color="#000" stop-opacity="0.1" />
                      <stop offset="0.72" stop-color="#000" stop-opacity="0" />
                    </linearGradient>
                  </defs>
                  <!-- a hair of curve as it hangs, the end cut in a V -->
                  <path d="M1.6 0H10.4C10.6 16 10.9 30 10.2 47.4L6.2 42.6L1.9 47.6C1.2 30 1.3 16 1.6 0Z" fill="url(#silk-{i})" />
                  <!-- the half-turn of the silk catching the light -->
                  <path d="M2 33C4.5 35.5 7.5 37.5 10.4 38.6V40.4C7.4 39.4 4.4 37.4 1.9 35Z" fill="#fff" opacity="0.22" />
                  <path d="M1.6 0H10.4V48H1.6Z" fill="url(#edge-{i})" />
                </svg>
              {/if}
              <span class="cover cloth">
                <!-- saddle-stitched round the board, tone on tone, following its corners -->
                <Stitch inset={[6, 6, 6, 20]} radius={[3, 4, 4, 3]} len={3.6} gap={2.5} width={1.3} thread="var(--stitch-cloth)" />
                <span class="issuer foil fine" aria-hidden="true" data-ascii="skip">{m.volume_encrypted()}</span>
                <span class="mark">
                  <Rosette size={Math.round(bookW * 0.5)} kind="cover" finish={r.exists ? 'foil' : 'flat'} frame={frameFor(r)}>
                    <Emblem emblem={lockedView(r)} name={r.name} size={Math.round(bookW * 0.175)} finish={r.exists ? 'foil' : 'flat'} decorative />
                  </Rosette>
                </span>
                <span class="name display foil" data-ascii="skip">{r.name}</span>
                {#if place}<span class="place foil fine" aria-hidden="true" data-ascii="skip">{place}</span>{/if}
              </span>
            </button>
            <button
              type="button"
              class="btn sm icon forget"
              aria-label={m.welcome_forget({ name: r.name })}
              title={m.welcome_forget_title()}
              onclick={() => app.forgetRecent(r.path)}
            >
              <X size={13} />
            </button>
            <p class="caption" id="recent-meta-{i}">
              {#if !r.exists}
                {m.welcome_unreachable()}
              {:else if open}
                <span class="open-now">{m.welcome_open_now()}</span> · <span class="num">{formatBytes(r.size_bytes)}</span>
              {:else}
                <span class="num">{formatBytes(r.size_bytes)}</span> · {formatAgo(r.last_opened)}
              {/if}
            </p>
            {#if where}
              {@const Icon = where.icon}
              <p class="where" id="recent-where-{i}"><Icon size={12} strokeWidth={1.7} aria-hidden="true" />{where.label()}</p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>


  <footer class="hint">{m.welcome_hint()}</footer>
  <UpdateNotice />
  <StickNotice />
</main>

<style>
  /* the board is the scroll container, so the scrollbar sits at the window edge */
  .welcome {
    position: relative;
    z-index: 2;
    height: 100%;
    overflow: auto;
    display: flex;
    flex-direction: column;
    padding: var(--space-5) clamp(24px, 7vw, 120px) var(--space-5);
  }
  .masthead {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .settings {
    color: var(--ink-2);
  }

  /* ---- title and the two keys ---------------------------------------- */
  .intro {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-4) var(--space-6);
    margin-top: clamp(28px, 7vh, 84px);
  }
  .title {
    font-size: clamp(44px, 6vw, 76px);
    font-weight: 400;
    letter-spacing: -0.015em;
    color: var(--title-ink);
    text-shadow:
      -0.5px -0.5px 0 var(--title-lip),
      0.5px 0.8px 0.6px var(--title-shade);
  }
  .title em {
    font-style: italic;
    color: var(--accent);
  }
  .sub {
    margin-top: 12px;
    font-size: var(--text-lg);
    color: var(--ink-3);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding-bottom: 6px;
  }

  /* ---- the shelf of passports ------------------------------------------ */
  .shelf {
    margin-top: clamp(28px, 6vh, 64px);
  }
  .passports {
    list-style: none;
    margin: 0;
    padding: 4px 0 var(--space-5);
    display: flex;
    flex-wrap: wrap;
    gap: 36px clamp(26px, 2.8vw, 40px);
  }
  .passport {
    position: relative;
    width: var(--book-w);
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .book {
    position: relative;
    display: block;
    width: 100%;
    padding: 0;
    border: 0;
    border-radius: 10px;
    background: none;
    color: inherit;
    text-align: center;
  }
  .book:focus-visible {
    outline-offset: 5px;
    outline-color: var(--accent);
  }
  .state {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  /* the cover: book cloth over board, the book block showing at the fore-edge */
  .cover {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    aspect-ratio: 0.705;
    /* the text lies on the stitched panel (Stitch inset: 20 px at the spine, 6 px elsewhere),
       centred on it, a hair inside the thread; the cover is its own size container, so every
       label is sized against the room it really has (window, interface size, language) */
    padding: 18px 10px 0 24px;
    container-type: inline-size;
    border-radius: 6px 10px 10px 6px;
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.14),
      inset 0 -1px 0 rgb(0 0 0 / 0.35),
      inset -1px 0 0 rgb(0 0 0 / 0.25),
      1px 1px 0 var(--paper-lo),
      2px 2px 0 var(--paper-edge),
      3px 3px 0 var(--paper-lo),
      4px 4px 0 -0.5px var(--paper-edge-lo),
      0 3px 4px -1px rgb(var(--cast) / 0.3),
      0 18px 30px -14px rgb(var(--cast) / 0.55);
    /* only the cover moves: the button's hit area stays put, so the lift never slides out
       from under the pointer (no hover on/off bounce) */
    transition:
      transform 180ms cubic-bezier(0.2, 0.7, 0.3, 1),
      box-shadow 180ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  /* the hinge: the board is scored near the spine */
  .cover::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 13px;
    width: 3px;
    border-left: 1px solid rgb(0 0 0 / 0.28);
    border-right: 1px solid rgb(255 255 255 / 0.08);
    background: linear-gradient(90deg, rgb(0 0 0 / 0.14), transparent);
    pointer-events: none;
  }
  .issuer {
    max-width: 100%;
    font-family: var(--font-mono);
    /* "Encrypted volume", 16 mono letters spaced 0.24 em: 13.5 em; never wider than the panel */
    font-size: min(9.5px, 100cqi / 13.8);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
    letter-spacing: 0.24em;
    text-transform: uppercase;
  }
  .mark {
    margin: 18px 0 14px;
    display: grid;
  }
  .name {
    max-width: 100%;
    /* on a narrow book the size the room allows: a long word (Dichiarazioni) still fits a line */
    font-size: min(calc(var(--book-w) * 0.122), 100cqi / 6.9);
    font-weight: 500;
    line-height: 1.08;
    /* a word longer than the line breaks inside it (then two lines and the ellipsis; the whole
       name is in the book's title) */
    overflow-wrap: anywhere;
    text-wrap: balance;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    /* Bodoni's ink reaches past its line box (descenders 0.27 em under the baseline, accents
       0.99 em over it): the clamp's box (and the foil, painted inside it) gets that room as padding, given back as margin */
    padding-block: 0.12em 0.16em;
    margin-block: -0.12em -0.16em;
  }
  .place {
    max-width: 100%;
    margin-top: 9px;
    font-family: var(--font-mono);
    font-size: min(10px, 100cqi / 15);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
    letter-spacing: 0.22em;
    text-transform: uppercase;
  }

  /* hover lifts the book off the board, pressing sets it down */
  .book:hover:not(:disabled) .cover {
    transform: translateY(-4px);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.16),
      inset 0 -1px 0 rgb(0 0 0 / 0.35),
      inset -1px 0 0 rgb(0 0 0 / 0.25),
      1px 1px 0 var(--paper-lo),
      2px 2px 0 var(--paper-edge),
      3px 3px 0 var(--paper-lo),
      4px 4px 0 -0.5px var(--paper-edge-lo),
      0 6px 8px -2px rgb(var(--cast) / 0.28),
      0 30px 40px -18px rgb(var(--cast) / 0.6);
  }
  .book:active:not(:disabled) .cover {
    transform: translateY(1px);
  }

  /* open now: the bookmark comes out of the book from under the cover (it lies between the
     cover and the pages: the cover, its edge and the page block are all drawn over it) */
  .cover {
    z-index: 1;
  }
  .ribbon {
    position: absolute;
    z-index: 0;
    right: 30%;
    /* 48 px of ribbon, the top 28 under the boards (and the page block): a 20 px tail */
    bottom: -20px;
    width: 12px;
    height: 48px;
    overflow: visible;
    filter: drop-shadow(0.6px 1.4px 0.8px rgb(var(--cast) / 0.45));
    transform: rotate(-2.5deg);
    transform-origin: 50% 0;
    transition: transform 180ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  /* it lifts with the book */
  .book:hover:not(:disabled) .ribbon {
    transform: translateY(-4px) rotate(-2.5deg);
  }
  .book:active:not(:disabled) .ribbon {
    transform: translateY(1px) rotate(-2.5deg);
  }

  /* unreachable: only the impression the book left in the board */
  .passport.missing .cover {
    background: none;
    color: var(--ink-3);
    box-shadow:
      inset 1px 1px 2px rgb(var(--cast) / 0.22),
      inset -1px -1px 0 var(--deboss-lip),
      inset 0 0 0 1px rgb(var(--cast) / 0.12);
  }
  /* no stitching, no hinge: nothing to keep the text off the spine, so the header, the seal
     and the name stand on the impression's own centre (not 7 px towards the fore-edge) */
  .passport.missing .cover {
    padding-inline: 20px;
  }
  .passport.missing .cover::before,
  .passport.missing .cover::after,
  .passport.missing .cover :global(.stitch) {
    display: none;
  }
  /* its look kept encrypted: bound in undyed material, a neutral rosette */
  .passport.neutral .cover {
    --cloth: var(--cloth-neutral);
    --cloth-hi: var(--cloth-neutral-hi);
    --cloth-lo: var(--cloth-neutral-lo);
    --stitch-cloth: color-mix(in oklab, var(--cloth-ink) 30%, var(--cloth-neutral));
  }
  .passport.missing .foil {
    color: var(--ink-3);
    background: none;
    -webkit-background-clip: border-box;
    background-clip: border-box;
    filter: none;
  }
  .passport.missing {
    --engrave: var(--ink-4);
  }
  .passport.missing :global(.rosette) {
    opacity: 0.55;
  }

  .forget {
    position: absolute;
    top: -9px;
    right: -9px;
    z-index: 2;
    border-radius: 50%;
    opacity: 0;
    color: var(--ink-2);
  }
  .passport:hover .forget,
  .passport:focus-within .forget,
  .passport.missing .forget {
    opacity: 1;
  }
  .caption {
    max-width: 100%;
    overflow-wrap: anywhere;
    margin-top: 26px;
    font-size: var(--text-sm);
    color: var(--ink-3);
    text-align: center;
  }
  .open-now {
    font-weight: 600;
    color: var(--accent);
  }
  /* where the scan found it: one quiet line under the caption */
  /* (inline, so a long place wraps under its own first word, the glyph leading it) */
  .where {
    max-width: 100%;
    overflow-wrap: anywhere;
    margin-top: 5px;
    font-size: var(--text-xs);
    line-height: 1.35;
    color: var(--ink-3);
    text-align: center;
    text-wrap: balance;
  }
  .where :global(svg) {
    display: inline;
    margin-right: 5px;
    vertical-align: -2px;
    color: var(--ink-4);
  }

  /* ---- empty: the place where the first volume will lie ----------------- */
  .empty {
    display: flex;
  }
  .slot {
    display: grid;
    place-items: center;
    align-content: center;
    gap: var(--space-4);
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-3);
    font: inherit;
    cursor: pointer;
    transition: opacity var(--dur-ui) var(--ease-out), color var(--dur-ui) var(--ease-out);
    width: var(--book-w);
    aspect-ratio: 0.705;
    border-radius: 6px 10px 10px 6px;
    box-shadow:
      inset 1px 1px 3px rgb(var(--cast) / 0.2),
      inset -1px -1px 0 var(--deboss-lip),
      inset 0 0 0 1px rgb(var(--cast) / 0.1);
    opacity: 0.8;
  }
  .slot:hover,
  .slot:focus-visible {
    opacity: 1;
    color: var(--ink);
  }
  .slot-label {
    font-size: var(--text-sm);
    font-weight: 500;
  }

  .hint {
    margin-top: auto;
    padding-top: var(--space-5);
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
</style>
