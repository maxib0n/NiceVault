<!--
  Emblem picker: initial (monogram) | curated icon | custom image, the frame round it (six,
  each previewed live with this emblem in it), and whether the volume's look (ink + emblem)
  shows while it is locked: "Aspetto visibile da chiuso" / "Aspetto cifrato" (show_when_locked
  IS the appearance visibility; saved with the emblem).
  Arrow keys move inside each radiogroup; Tab moves between groups.
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { ImagePlus } from '@lucide/svelte';
  import { EMBLEM_ICONS, imageFileToEmblem, INITIAL, initialOf } from '../lib/emblem';
  import { errorText } from '../lib/errors';
  import type { Emblem as EmblemT, EmblemState } from '../lib/types';
  import Dialog from './Dialog.svelte';
  import Emblem from './Emblem.svelte';
  import Rosette from '../ornament/Rosette.svelte';
  import { FRAMES, frameLabel, frameOf, type FrameId } from '../ornament/frames';
  import { radioKeys } from '../lib/radio';

  let {
    open,
    name,
    value,
    onsave,
    onclose,
  }: {
    open: boolean;
    /** volume name (initial + title) */
    name: string;
    value: EmblemState;
    /** resolves true when saved (the dialog then closes) */
    onsave: (s: EmblemState) => Promise<boolean>;
    onclose: () => void;
  } = $props();

  let emblem = $state<EmblemT>(INITIAL);
  let frame = $state<FrameId>('guilloche');
  let showWhenLocked = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let fileInput: HTMLInputElement | undefined = $state();
  let wasOpen = false;

  // reset the draft each time the dialog opens
  $effect(() => {
    if (open && !wasOpen) {
      emblem = $state.snapshot(value.emblem) as EmblemT;
      frame = frameOf(value.emblem_frame);
      showWhenLocked = value.show_when_locked;
      error = null;
      busy = false;
    }
    wasOpen = open;
  });

  const KINDS = [
    { id: 'initial', label: m.ep_initial },
    { id: 'icon', label: m.ep_icon },
    { id: 'image', label: m.ep_image },
  ] as const;

  const iconId = $derived(emblem.kind === 'icon' ? emblem.id : null);

  function pickKind(k: (typeof KINDS)[number]['id']) {
    if (k === 'initial') emblem = INITIAL;
    else if (k === 'icon') emblem = { kind: 'icon', id: iconId ?? EMBLEM_ICONS[0].id };
    else if (emblem.kind !== 'image') fileInput?.click();
  }

  async function onFile(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    (e.currentTarget as HTMLInputElement).value = '';
    if (!f) return;
    error = null;
    try {
      emblem = { kind: 'image', data_base64: await imageFileToEmblem(f) };
    } catch (err) {
      error = errorText(err);
    }
  }

  function gridKeys(e: KeyboardEvent) {
    const cols = 6;
    const i = EMBLEM_ICONS.findIndex((x) => x.id === iconId);
    const n = EMBLEM_ICONS.length;
    const d = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: cols, ArrowUp: -cols, Home: -i, End: n - 1 - i }[e.key];
    if (d === undefined) return;
    e.preventDefault();
    const j = Math.min(n - 1, Math.max(0, (i < 0 ? 0 : i) + d));
    emblem = { kind: 'icon', id: EMBLEM_ICONS[j].id };
    (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="radio"]')[j]?.focus();
  }

  const LOOKS = [
    { visible: true, label: m.look_visible, hint: m.look_visible_hint },
    { visible: false, label: m.look_encrypted, hint: m.look_encrypted_hint },
  ] as const;

  function lookKeys(e: KeyboardEvent) {
    const d = e.key === 'ArrowRight' || e.key === 'ArrowDown' || e.key === 'ArrowLeft' || e.key === 'ArrowUp';
    if (!d) return;
    e.preventDefault();
    showWhenLocked = !showWhenLocked;
    (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="radio"]')[showWhenLocked ? 0 : 1]?.focus();
  }

  function kindKeys(e: KeyboardEvent) {
    const i = KINDS.findIndex((k) => k.id === emblem.kind);
    const d = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const j = (i + d + KINDS.length) % KINDS.length;
    pickKind(KINDS[j].id);
    (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="radio"]')[j]?.focus();
  }

  async function save(e?: Event) {
    e?.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    const ok = await onsave({ emblem: $state.snapshot(emblem) as EmblemT, show_when_locked: showWhenLocked, emblem_frame: frame });
    busy = false;
    if (ok) onclose();
  }
</script>

<Dialog {open} title={m.ep_title()} variant="wide" onclose={() => !busy && onclose()}>
  <form class="picker" id="emblem-form" onsubmit={save} data-testid="emblem-picker">
    <div class="preview">
      <span class="preview-seal"><Rosette size={128} kind="seal" {frame}><Emblem {emblem} {name} size={44} /></Rosette></span>
      <div class="preview-side">
        <div class="kinds" role="radiogroup" aria-label={m.ep_kind()} tabindex="-1" onkeydown={kindKeys}>
          {#each KINDS as k (k.id)}
            <button
              type="button"
              role="radio"
              class="kind"
              aria-checked={emblem.kind === k.id}
              tabindex={emblem.kind === k.id ? 0 : -1}
              data-testid="emblem-kind-{k.id}"
              onclick={() => pickKind(k.id)}
            >
              {k.label()}
            </button>
          {/each}
        </div>
        <p class="hint">
          {emblem.kind === 'initial' ? m.ep_hint_initial({ letter: initialOf(name) }) : emblem.kind === 'icon' ? m.ep_hint_icon() : m.ep_hint_image()}
        </p>
      </div>
    </div>

    {#if emblem.kind === 'icon'}
      <div class="grid" role="radiogroup" aria-label={m.ep_icons()} tabindex="-1" onkeydown={gridKeys}>
        {#each EMBLEM_ICONS as ic (ic.id)}
          <button
            type="button"
            role="radio"
            class="icon-btn"
            aria-checked={iconId === ic.id}
            aria-label={ic.label()}
            title={ic.label()}
            tabindex={iconId === ic.id ? 0 : -1}
            data-testid="emblem-icon-{ic.id}"
            onclick={() => (emblem = { kind: 'icon', id: ic.id })}
          >
            <Emblem emblem={{ kind: 'icon', id: ic.id }} {name} size={34} decorative />
          </button>
        {/each}
      </div>
    {:else if emblem.kind === 'image'}
      <div class="image-row">
        <button type="button" class="btn sm" onclick={() => fileInput?.click()}>
          <ImagePlus size={14} aria-hidden="true" /> {m.ep_other_image()}
        </button>
        <p class="hint">{m.ep_image_hint()}</p>
      </div>
    {/if}
    <input
      bind:this={fileInput}
      type="file"
      accept="image/png,image/jpeg,image/webp"
      class="file"
      tabindex="-1"
      aria-hidden="true"
      data-testid="emblem-file"
      onchange={onFile}
    />

    <div class="frames-block">
      <p class="look-title caps" id="picker-frame-title">{m.ep_frame()}</p>
      <div
        class="frames"
        role="radiogroup"
        aria-labelledby="picker-frame-title"
        aria-describedby="picker-frame-hint"
        tabindex="-1"
        data-testid="emblem-frames"
        onkeydown={(e) => radioKeys(e, FRAMES, frame, (v) => (frame = v))}
      >
        {#each FRAMES as f (f)}
          <button
            type="button"
            role="radio"
            class="frame-btn"
            aria-checked={frame === f}
            tabindex={frame === f ? 0 : -1}
            data-frame={f}
            onclick={() => (frame = f)}
          >
            <span class="frame-well"><Rosette size={58} kind="swatch" frame={f}><Emblem {emblem} {name} size={19} decorative /></Rosette></span>
            <span class="frame-name">{frameLabel(f)}</span>
          </button>
        {/each}
      </div>
      <p class="hint" id="picker-frame-hint">{m.ep_frame_hint()}</p>
    </div>

    <div class="look">
      <p class="look-title caps" id="picker-look-title">{m.look_locked()}</p>
      <div
        class="looks"
        role="radiogroup"
        aria-labelledby="picker-look-title"
        aria-describedby="picker-look-hint"
        tabindex="-1"
        data-testid="emblem-show-locked"
        onkeydown={lookKeys}
      >
        {#each LOOKS as l (l.visible)}
          <button type="button" role="radio" class="kind" aria-checked={showWhenLocked === l.visible} tabindex={showWhenLocked === l.visible ? 0 : -1} onclick={() => (showWhenLocked = l.visible)}>
            {l.label()}
          </button>
        {/each}
      </div>
      <p class="hint" id="picker-look-hint">{LOOKS[showWhenLocked ? 0 : 1].hint()}</p>
    </div>

    {#if error}<p class="error-text" role="alert">{error}</p>{/if}
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={onclose} disabled={busy}>{m.action_cancel()}</button>
    <button type="submit" form="emblem-form" class="btn primary" disabled={busy} data-testid="emblem-save">{busy ? m.action_saving() : m.action_save()}</button>
  {/snippet}
</Dialog>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  /* the emblem as it will be printed on the document */
  .preview {
    display: flex;
    align-items: center;
    gap: var(--space-5);
    padding: var(--space-3);
    border-radius: var(--radius-md);
    background: var(--paper);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .preview-seal {
    display: grid;
    flex: none;
  }
  .preview-side {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .kinds {
    display: inline-flex;
    flex-wrap: wrap;
    align-self: flex-start;
    gap: 2px;
    padding: 3px;
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .kind {
    min-height: 28px;
    padding: 0 13px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    font-size: var(--text-md);
    font-weight: 500;
    color: var(--ink-3);
  }
  .kind:hover {
    color: var(--ink);
  }
  .kind:active {
    background: color-mix(in oklab, var(--ink) 7%, transparent);
  }
  .kind[aria-checked='true'] {
    color: var(--ink);
    font-weight: 600;
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      0 0 0 1px var(--key-edge),
      var(--lift-key);
  }
  .kind:focus-visible {
    outline-offset: 1px;
  }

  /* the icons, each engraved on a small paper disc */
  .grid {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 6px;
  }
  .icon-btn {
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    padding: 4px;
    border: 0;
    border-radius: 50%;
    background: var(--paper);
    box-shadow:
      0 0 0 1px var(--paper-rule),
      0 1px 2px rgb(var(--cast) / 0.1);
  }
  .icon-btn:hover {
    box-shadow:
      0 0 0 1px var(--accent-line),
      0 2px 5px rgb(var(--cast) / 0.16);
  }
  .icon-btn:active {
    transform: translateY(1px);
  }
  .icon-btn[aria-checked='true'] {
    background: var(--accent-soft);
    box-shadow:
      0 0 0 2px var(--paper-hi),
      0 0 0 3.5px var(--accent);
  }
  .icon-btn:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 3px;
  }
  .image-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-start;
  }
  .file {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
  }

  /* the frames: each one drawn round this emblem, in a small paper well */
  .frames-block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .frames {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(84px, 1fr));
    gap: 4px;
    align-items: start;
  }
  .frame-btn {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 8px 4px 7px;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: var(--ink-3);
  }
  .frame-well {
    display: grid;
    place-items: center;
    width: 66px;
    height: 66px;
    border-radius: 50%;
    background: var(--paper);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  /* two lines are kept for every name, so the wells and the first lines all align */
  .frame-name {
    min-height: 2.5em;
    font-size: var(--text-xs);
    line-height: 1.25;
    text-align: center;
    text-wrap: balance;
  }
  .frame-btn:hover {
    color: var(--ink);
    background: color-mix(in oklab, var(--ink) 4%, transparent);
  }
  .frame-btn:active .frame-well {
    transform: translateY(1px);
  }
  .frame-btn[aria-checked='true'] {
    color: var(--accent);
  }
  .frame-btn[aria-checked='true'] .frame-well {
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 0 0 2px var(--paper-hi),
      0 0 0 3.5px var(--accent);
  }
  .frame-btn[aria-checked='true'] .frame-name {
    font-weight: 600;
  }
  .frame-btn:focus-visible {
    outline-offset: 1px;
  }

  /* while locked: the look shows, or the volume appears neutral */
  .look {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .looks {
    display: inline-flex;
    align-self: flex-start;
    flex-wrap: wrap;
    gap: 2px;
    padding: 3px;
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
</style>
