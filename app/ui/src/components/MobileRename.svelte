<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { onMount } from 'svelte';
  import { sheetSwipe } from '../lib/sheetSwipe';

  let {
    name,
    title = m.fb_rename(),
    oncommit,
    oncancel,
  }: { name: string; title?: string; oncommit: (value: string) => Promise<boolean>; oncancel: () => void } = $props();

  let dlg: HTMLDialogElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let busy = $state(false);
  let value = $state('');
  const same = $derived(value.trim() === '' || value.trim() === name);

  onMount(() => {
    value = name;
    dlg?.showModal();
    if (!input) return;
    input.value = name;
    input.focus();
    const dot = name.lastIndexOf('.');
    input.setSelectionRange(0, dot > 0 ? dot : name.length);
  });

  async function commit() {
    if (busy) return;
    if (same) return oncancel();
    busy = true;
    const ok = await oncommit(value);
    busy = false;
    if (!ok) input?.focus();
  }
</script>

<dialog
  bind:this={dlg}
  class="m-rename nv-enter"
  aria-labelledby="m-rename-title"
  data-testid="mobile-rename"
  oncancel={(e) => {
    e.preventDefault();
    oncancel();
  }}
  onclick={(e) => e.target === dlg && oncancel()}
  use:sheetSwipe={{ onclose: oncancel }}
>
  <form
    onsubmit={(e) => {
      e.preventDefault();
      void commit();
    }}
  >
    <span class="grabber" aria-hidden="true"></span>
    <h2 id="m-rename-title" class="title display">{title}</h2>
    <label class="field">
      <span class="caps">{m.rename_label()}</span>
      <input
        bind:this={input}
        bind:value
        class="input"
        type="text"
        enterkeyhint="done"
        autocapitalize="off"
        autocorrect="off"
        spellcheck="false"
        autocomplete="off"
      />
    </label>
    <div class="keys">
      <button type="button" class="btn" onclick={oncancel}>{m.action_cancel()}</button>
      <button type="submit" class="btn primary" disabled={busy || same}>{m.fb_rename()}</button>
    </div>
  </form>
</dialog>

<style>
  .m-rename {
    position: fixed;
    inset: auto 0 var(--nv-kb, 0px);
    width: 100%;
    max-width: 560px;
    margin: 0 auto;
    padding: 0;
    border: 0;
    border-radius: var(--sheet-r, 18px) var(--sheet-r, 18px) 0 0;
    background: var(--paper-hi);
    color: var(--ink);
    box-shadow:
      inset 0 1px 0 var(--slip-lip, var(--key-lip)),
      0 -1px 0 rgb(var(--cast) / 0.18),
      0 -18px 40px -12px rgb(var(--cast) / 0.5);
    overflow: visible;
    --enter-from: translate3d(0, 56px, 0);
    --enter-to: translate3d(0, 0, 0);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
  }
  .m-rename::backdrop {
    background: var(--scrim);
  }
  form {
    display: grid;
    gap: 14px;
    padding: 8px max(18px, var(--safe-right, 0px)) calc(16px + var(--safe-bottom, 0px)) max(18px, var(--safe-left, 0px));
  }
  :global(html[data-keyboard]) form {
    padding-bottom: 12px;
  }
  .grabber {
    justify-self: center;
    width: 38px;
    height: 5px;
    border-radius: 3px;
    background: var(--grabber, var(--paper-rule));
    box-shadow: 0 1px 0 var(--key-lip);
  }
  .title {
    margin: 2px 0 0;
    font-size: var(--text-xl);
    font-weight: 500;
    line-height: 1.15;
  }
  .field {
    display: grid;
    gap: 6px;
  }
  .field .caps {
    color: var(--ink-3);
  }
  .field .input {
    width: 100%;
    min-height: 48px;
    font-size: 17px;
  }
  .keys {
    display: grid;
    grid-template-columns: 1fr 1.4fr;
    gap: 10px;
  }
  .keys .btn {
    min-height: 48px;
    justify-content: center;
  }
  @media (prefers-reduced-motion: reduce) {
    .m-rename {
      animation: none;
    }
  }
</style>
