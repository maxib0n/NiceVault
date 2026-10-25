<!--
  Sblocca: the volume as a security document lying on the board. The seal (a
  rosette cut by two lathes, the emblem in its cartouche), a foil stripe, the
  data panel whose last field is the password, the watermark in the paper and a
  machine-readable zone computed from the volume's real data.
  Hooks: data-phase idle | unlocking | error | success, data-error-seq (restarts the
  one shake), --lathe on the seal's two lathe canvases only (degrees they have turned while
  the key is derived; registered with inherits: false, so a frame restyles two canvases, not
  the page; ceremony.ts runs them into register).
-->
<script lang="ts">
  import { ArrowLeft, LockKeyholeOpen } from '@lucide/svelte';
  import { m } from '../paraglide/messages.js';
  import { onDestroy } from 'svelte';
  import { app, lockReasonText } from '../lib/stores/app.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import { formatAgo, formatBytes } from '../lib/format';
  import { lidClose, lidOpen } from '../components/ceremony';
  import { lockedFrame, lockedView } from '../lib/emblem';
  import Emblem from '../components/Emblem.svelte';
  import PasswordField from '../components/PasswordField.svelte';
  import Brand from '../components/Brand.svelte';
  import Fibres from '../ornament/Fibres.svelte';
  import Frame, { docPitch, docStitch } from '../ornament/Frame.svelte';
  import Ghost from '../ornament/Ghost.svelte';
  import Microline from '../ornament/Microline.svelte';
  import Perforation from '../ornament/Perforation.svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import Stitch from '../ornament/Stitch.svelte';
  import Thread from '../ornament/Thread.svelte';
  import Tint from '../ornament/Tint.svelte';
  import { mrzLines, placeOf } from '../ornament/document';
  import { hashString } from '../ornament/lathe';
  import { decode } from '../lib/reveal';

  let password = $state('');
  let input: HTMLInputElement | undefined = $state();
  let lathe = $state(0);
  let raf = 0;

  const target = $derived(app.unlockTarget);
  const phase = $derived(vaults.unlock.phase);
  const busy = $derived(phase === 'unlocking');
  // the drive holding the volume is gone (pulled out): nothing to unlock until it is back
  const gone = $derived.by(() => {
    if (!target) return false;
    const r = app.recents.find((x) => x.path === target.path);
    // what the recent list knows (a removable drive's volumes drop off it when it is pulled out);
    // the lock reason alone must never leave the field disabled once the file is back
    return r ? !r.exists : app.recentsLoaded && target.source === 'removable';
  });
  $effect(() => {
    if (gone) {
      if (document.activeElement === input) input?.blur();
    } else if (app.lockReason === 'removed' && phase !== 'unlocking') queueMicrotask(() => input?.focus());
  });
  const reasonText = $derived(app.lockReason ? lockReasonText(app.lockReason) : null);

  // the kicker never repeats the name; the Posizione fact always says where it is
  const place = $derived(target ? placeOf(target.path, target.name) : '');
  const location = $derived(target ? placeOf(target.path) : '');
  const fileName = $derived(target?.path.split(/[\\/]/).pop() ?? '');
  const mrz = $derived(target ? mrzLines(target) : ['', '']);
  const seed = $derived(target ? hashString(target.path) : 1);

  // the seal's size follows its panel
  let sealW = $state(0);
  let sealH = $state(0);
  let sealEl: HTMLElement | undefined = $state();
  const sealSize = $derived(Math.max(120, Math.floor(Math.min(sealW * 0.9, sealH - 6))));

  // While the key is derived the two lathes turn at a steady pace (the mechanism is working;
  // no fake percentage).
  $effect(() => {
    if (phase === 'unlocking') {
      const tick = () => {
        lathe = (performance.now() - vaults.unlock.startedAt) * 0.045;
        raf = requestAnimationFrame(tick);
      };
      tick();
      return () => cancelAnimationFrame(raf);
    }
    if (phase !== 'success') lathe = 0;
  });
  // ... set on the lathe canvases themselves (not inherited: nothing else restyles)
  $effect(() => {
    const v = String(lathe);
    void sealSize;
    for (const el of sealEl?.querySelectorAll<HTMLElement>('.lathe-a, .lathe-b') ?? []) el.style.setProperty('--lathe', v);
  });

  onDestroy(() => cancelAnimationFrame(raf));

  // After a wrong password: clear and refocus.
  $effect(() => {
    if (phase === 'error') {
      password = '';
      queueMicrotask(() => input?.focus());
    }
  });

  async function submit(e?: Event) {
    e?.preventDefault();
    if (!target || busy || gone || !password) return;
    const pw = password;
    await vaults.unlockVault(target, pw);
  }

  function back() {
    vaults.resetUnlock();
    app.lockReason = null;
    app.goto(vaults.open.length ? 'vault' : 'welcome');
  }
</script>

<main
  class="unlock scroller"
  data-phase={phase}
  data-error-seq={vaults.unlock.errorSeq}
  aria-labelledby="unlock-title"
  in:lidClose
  out:lidOpen
>
  <div class="board ground" aria-hidden="true"></div>
  <nav class="top" aria-label={m.unlock_nav()}>
    <button type="button" class="btn ghost back" onclick={back} disabled={busy}>
      <ArrowLeft size={16} aria-hidden="true" /> {m.unlock_others()}
    </button>
  </nav>

  <div class="stage">
    <!-- the first time it is shown in a session the document decodes from glyphs (not when a key
         brought it here); the password field takes keys at once -->
    <article class="document sheet" use:decode={{ key: 'start:unlock', speed: 1.35, media: '.seal canvas, .wm-window canvas', fades: '.thread-col' }}>
      <Fibres {seed} density={0.6} quiet={[{ x: 0.35, y: 0.07, w: 0.52, h: 0.62 }, { x: 0.35, y: 0.8, w: 0.62, h: 0.14 }, { x: 0.06, y: 0.82, w: 0.26, h: 0.1 }]} />
      <Frame />
      <div class="face">
        <section class="seal-panel" bind:clientWidth={sealW} aria-label={m.unlock_seal()}>
          <Tint around={sealEl} seed={seed % 97} measure={sealSize} />
          <div class="seal-box" bind:clientHeight={sealH}>
            <div class="seal" bind:this={sealEl}>
              <!-- the emblem's shadow as it rises (ceremony.ts fades it in): drawn once, never animated as a filter -->
              <span class="rise-shadow" aria-hidden="true" style:--d="{Math.round(sealSize * 0.34)}px"></span>
              <Rosette size={sealSize} kind="seal" frame={lockedFrame(target)}>
                {#if target}<Emblem emblem={lockedView(target)} name={target.name} size={Math.round(sealSize * 0.34)} />{/if}
              </Rosette>
            </div>
          </div>
          <p class="file" title={target?.path}>{fileName}</p>
        </section>

        <div class="thread-col"><Thread /></div>

        <section class="data-panel">
          <p class="caps kicker">{m.volume_encrypted()}{place ? ` · ${place}` : ''}</p>
          <h1 id="unlock-title" class="name display">{target?.name ?? m.volume()}</h1>
          <Microline text="NICEVAULT · {target?.name?.toUpperCase() ?? ''} · {m.volume_encrypted().toUpperCase()} · " />
          <dl class="facts">
            <div>
              <dt class="caps">{m.unlock_location()}</dt>
              <dd title={target?.path}>{location || '—'}</dd>
            </div>
            <div>
              <dt class="caps">{m.unlock_size()}</dt>
              <dd class="num">{target?.size_bytes ? formatBytes(target.size_bytes) : '—'}</dd>
            </div>
            <div>
              <dt class="caps">{m.unlock_last_opened()}</dt>
              <dd>{target?.last_opened ? formatAgo(target.last_opened) : '—'}</dd>
            </div>
          </dl>

          <form class="pw-form" onsubmit={submit} aria-busy={busy}>
            <PasswordField
              bind:value={password}
              bind:input
              label={m.password()}
              variant="document"
              autofocus
              disabled={busy || gone}
              invalid={phase === 'error'}
              describedby={phase === 'error' ? 'unlock-error' : undefined}
            />
            <button type="submit" class="btn primary lg submit" disabled={busy || gone || !password}>
              <LockKeyholeOpen size={17} aria-hidden="true" />
              {busy ? m.unlock_busy() : m.unlock_submit()}
            </button>
          </form>

          <div class="messages">
            <p class="status" role="status" aria-live="polite">
              {#if busy}{m.unlock_status()}{:else if gone}{m.unlock_drive_gone()}{/if}
            </p>
            {#if phase === 'error' && vaults.unlock.error}
              {#key vaults.unlock.errorSeq}
                <p class="error-text" id="unlock-error" role="alert">{vaults.unlock.error}</p>
              {/key}
            {/if}
          </div>
        </section>

        <!-- the ghost image (the emblem repeated, small, in a line screen) and the document number
             laser-perforated through the card: the same number as the MRZ's first field -->
        <div class="wm-window">
          {#if target}<Ghost emblem={lockedView(target)} name={target.name} legend="NICEVAULT · {target.name.toUpperCase()} · " />{/if}
          {#if target?.volume_tag}<span class="perforated"><Perforation text={target.volume_tag} /></span>{/if}
        </div>

        <p class="mrz" aria-hidden="true"><span>{mrz[0]}</span><span>{mrz[1]}</span></p>
      </div>
      <!-- saddle-stitched round the edge, over the foil stripe, each corner round its medallion -->
      <Stitch track={docStitch} pitch={docPitch} width={1.5} />
      {#if reasonText}
        <p class="reason" data-reason={app.lockReason}>{reasonText}</p>
      {/if}
    </article>
  </div>

  <footer class="imprint"><Brand /></footer>
</main>

<style>
  /* ---- the board ------------------------------------------------------ */
  .unlock {
    position: relative;
    z-index: 2;
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    overflow: auto;
    overflow-x: hidden;
  }
  .board {
    /* the whole window however far the page is scrolled (.scroller clips it to the screen) */
    position: fixed;
    inset: 0;
    z-index: -1;
  }
  .top {
    display: flex;
    padding: var(--space-3) var(--space-4) 0;
  }
  .imprint {
    display: flex;
    justify-content: center;
    padding: 0 0 var(--space-4);
  }
  .stage {
    display: grid;
    place-items: center;
    min-height: 0;
    padding: var(--space-3) var(--space-5);
  }

  /* ---- the document -------------------------------------------------- */
  .document {
    --w: min(1240px, calc(100vw - 64px), calc((100vh - 124px) * 1.98));
    width: var(--w);
    aspect-ratio: 1.98;
    container-type: inline-size;
    /* the card's corners are centred on the corner medallions (Frame.svelte frameGeom: c = m + b/2),
       like the stitching's turn round them: cut edge, stitching and medallion are concentric */
    border-radius: calc(max(12px, var(--w) * 0.0135) + max(2.5px, var(--w) * 0.0034));
    overflow: hidden;
    box-shadow: var(--shadow-sheet);
  }
  .face {
    position: absolute;
    inset: 4.6cqw 4.4cqw 3.4cqw;
    display: grid;
    grid-template-columns: 29cqw 1.8cqw minmax(0, 1fr) 15cqw;
    grid-template-rows: minmax(0, 1fr) auto;
    column-gap: 0;
  }
  /* a very small window (or a large interface size: 150% of 900x600 is 600x400): the page
     keeps its proportions as long as its contents fit, then grows taller and the board scrolls */
  @media (max-height: 460px), (max-width: 680px) {
    .unlock {
      grid-template-rows: auto minmax(max-content, 1fr) auto;
    }
    .document {
      aspect-ratio: auto;
      min-height: calc(var(--w) / 1.98);
    }
    .face {
      position: relative;
      inset: auto;
      margin: 4.6cqw 4.4cqw 3.4cqw;
      min-height: calc(var(--w) / 1.98 - 8cqw);
    }
  }

  /* ---- seal panel ---------------------------------------------------- */
  .seal-panel {
    grid-column: 1;
    grid-row: 1;
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    min-height: 0;
    padding: 0.4cqw 2cqw 0 0;
  }
  .seal-panel > :global(.tint-ink) {
    inset: -1.2cqw 0.6cqw 0 -1.2cqw;
    width: calc(100% + 0.6cqw);
    height: calc(100% + 1.2cqw);
  }
  .seal-box {
    flex: 1;
    min-height: 0;
    width: 100%;
    display: grid;
    place-items: start center;
  }
  .seal {
    position: relative;
    display: grid;
  }
  /* the emblem rises in relief as the lathes lock (ceremony.ts: a transform, and this shadow,
     a soft disc under the cartouche, fading in) */
  .rise-shadow {
    position: absolute;
    left: 50%;
    top: 50%;
    width: var(--d);
    height: var(--d);
    border-radius: 50%;
    background: radial-gradient(closest-side, rgb(var(--cast) / 0.35), rgb(var(--cast) / 0.12) 70%, transparent);
    transform: translate(calc(-50% + 0.8px), calc(-50% + 1.6px));
    opacity: 0;
    pointer-events: none;
  }
  .file {
    position: relative;
    margin-top: 1.4cqw;
    max-width: 100%;
    font-family: var(--font-display);
    font-style: italic;
    font-size: max(18px, 1.7cqw);
    line-height: 1.15;
    color: var(--ink-2);
    text-align: center;
    overflow-wrap: anywhere;
  }

  .thread-col {
    grid-column: 2;
    grid-row: 1 / -1;
    /* the stripe runs from edge to edge of the sheet */
    margin: -4.6cqw 0 -3.4cqw;
  }

  /* ---- data panel ---------------------------------------------------- */
  .data-panel {
    grid-column: 3;
    grid-row: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 0.6cqw 2.6cqw 0 3.4cqw;
  }
  .kicker {
    color: var(--ink-3);
  }
  .name {
    margin-top: 1.1cqw;
    font-size: clamp(32px, 6.1cqw, 76px);
    font-weight: 500;
    line-height: 1.02;
    color: var(--title-ink);
    overflow-wrap: break-word;
    text-wrap: balance;
    /* intaglio on paper (the ink stands up); seated on anthracite */
    text-shadow:
      -0.5px -0.5px 0 var(--title-lip),
      0.5px 0.8px 0.6px var(--title-shade);
  }
  .data-panel :global(.microline) {
    margin-top: 1.3cqw;
  }
  .facts {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, auto));
    justify-content: start;
    gap: 0 3.2cqw;
    margin: 1.5cqw 0 0;
  }
  .facts dd {
    margin: 6px 0 0;
    font-size: clamp(13px, 1.3cqw, 15px);
    font-weight: 500;
    color: var(--ink);
    overflow-wrap: anywhere;
  }

  .pw-form {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-rows: auto auto auto;
    column-gap: 10px;
    margin-top: 3cqw;
  }
  .pw-form :global(.password-field) {
    display: contents;
  }
  .pw-form :global(.pf-label) {
    grid-row: 1;
    grid-column: 1 / -1;
    margin-bottom: 8px;
  }
  .pw-form :global(.control) {
    grid-row: 2;
    grid-column: 1;
  }
  .pw-form :global(.caps) {
    grid-row: 3;
    grid-column: 1 / -1;
    margin-top: 6px;
  }
  .submit {
    grid-row: 2;
    grid-column: 2;
    min-width: 132px;
  }
  /* the held key: pressed and latched while the lathes turn */
  .unlock[data-phase='unlocking'] .submit:disabled {
    --face-hi: var(--key-accent-press);
    --face-lo: var(--accent-deep);
    color: var(--key-accent-ink);
    transform: translateY(1px);
    box-shadow:
      inset 0 0 0 1px var(--key-accent-edge),
      inset 0 1px 4px rgb(0 0 0 / 0.35);
  }
  .messages {
    display: grid;
    min-height: 22px;
    margin-top: 8px;
  }
  .messages > :global(*) {
    grid-area: 1 / 1;
  }
  .status {
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
  .messages .error-text {
    font-size: var(--text-md);
  }

  /* ---- watermark window: clean paper, the emblem formed in the sheet -- */
  .wm-window {
    grid-column: 4;
    grid-row: 1;
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 0.9cqw;
    min-height: 0;
    padding: 0.5cqw 0 1.6cqw 0.8cqw;
  }
  /* the ghost: a portrait oval with its legend round it, the column's width */
  .wm-window > :global(.ghost) {
    flex: none;
    height: auto;
    aspect-ratio: 0.8;
    max-height: 100%;
  }
  /* the number perforated through the card, just under the ghost */
  .perforated {
    flex: none;
    display: block;
    height: 1.9cqw;
    margin: 0 0.4cqw;
  }

  /* ---- MRZ ------------------------------------------------------------- */
  .mrz {
    grid-column: 3 / -1;
    grid-row: 2;
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: 0.25em;
    /* symmetric margins: engines that round glyph advances overrun into the right one, never past the rule */
    padding: 1.2cqw 3.4cqw 0;
    border-top: 1px solid var(--paper-rule);
    font-family: var(--font-mono);
    font-weight: 500;
    font-size: clamp(11px, 1.22cqw, 15px);
    line-height: 1.25;
    color: var(--ink-2);
    white-space: nowrap;
  }
  .mrz span {
    letter-spacing: calc((100cqw - 26.4em) / 44);
  }

  /* ---- why it was locked: a stamp on the document ---------------------- */
  .reason {
    position: absolute;
    right: 24cqw;
    bottom: 11.5cqw;
    transform: rotate(-6deg);
    padding: 0.5cqw 1.2cqw;
    border: 2px solid currentColor;
    outline: 1px solid currentColor;
    outline-offset: 2px;
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: clamp(10px, 1.05cqw, 13px);
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    white-space: nowrap;
    color: var(--danger);
    /* rubber on paper: a flat ink that lets the sheet show through */
    mix-blend-mode: var(--ink-blend);
    opacity: 0.9;
  }

  /* ---- wrong key: the document shakes once ------------------------------ */
  /* on a registered property, read by translate (styles/motion.css: no compositor transform) */
  .unlock[data-phase='error'] .document {
    translate: var(--nv-dx) 0;
    animation: shake-a 360ms var(--ease-out) 1;
  }
  .unlock[data-phase='error']:is([data-error-seq$='1'], [data-error-seq$='3'], [data-error-seq$='5'], [data-error-seq$='7'], [data-error-seq$='9']) .document {
    animation-name: shake-b;
  }
  @keyframes shake-a {
    0% { --nv-dx: 0px; }
    16% { --nv-dx: -7px; }
    34% { --nv-dx: 6px; }
    52% { --nv-dx: -3px; }
    70% { --nv-dx: 2px; }
    100% { --nv-dx: 0px; }
  }
  @keyframes shake-b {
    0% { --nv-dx: 0px; }
    16% { --nv-dx: -7px; }
    34% { --nv-dx: 6px; }
    52% { --nv-dx: -3px; }
    70% { --nv-dx: 2px; }
    100% { --nv-dx: 0px; }
  }
  /* a wrong key also lets the lathes fall back to rest */
  .unlock[data-phase='error'] .seal :global(:is(.lathe-a, .lathe-b)) {
    transition: --lathe 420ms var(--ease-out);
  }
  @media (prefers-reduced-motion: reduce) {
    .unlock[data-phase='error'] .document {
      animation: none;
    }
  }

  /* ---- narrow documents: the watermark window folds into the margin ------- */
  @container (max-width: 880px) {
    .face {
      grid-template-columns: 30cqw 2cqw minmax(0, 1fr) 6cqw;
    }
    .wm-window {
      padding: 0;
      opacity: 0.6;
    }
    .perforated {
      display: none;
    }
    .facts {
      gap: 0 2.4cqw;
    }
  }
</style>
