<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { onMount, tick } from 'svelte';

  let {
    name,
    oncommit,
    oncancel,
  }: { name: string; oncommit: (value: string) => Promise<boolean>; oncancel: () => void } = $props();

  let el: HTMLInputElement | undefined = $state();
  let done = false;

  onMount(() => {
    if (!el) return;
    el.value = name;
    el.focus();
    const dot = name.lastIndexOf('.');
    el.setSelectionRange(0, dot > 0 ? dot : name.length);
  });

  function returnFocus(list: HTMLElement | null | undefined) {
    void tick().then(() => {
      if (list?.isConnected && (document.activeElement === document.body || document.activeElement === null)) list.focus();
    });
  }

  async function commit(fromKeyboard = false) {
    if (done || !el) return;
    done = true;
    const list = el.closest<HTMLElement>('[role="listbox"]');
    const ok = await oncommit(el.value);
    if (ok) {
      if (fromKeyboard) returnFocus(list);
    } else if (el) {
      done = false;
      el.focus();
    }
  }

  function keydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      void commit(true);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      done = true;
      const list = el?.closest<HTMLElement>('[role="listbox"]');
      oncancel();
      returnFocus(list);
    }
  }
</script>

<input
  bind:this={el}
  class="rename-input input"
  aria-label={m.rename_label()}
  spellcheck="false"
  autocomplete="off"
  onkeydown={keydown}
  onblur={() => commit()}
  onclick={(e) => e.stopPropagation()}
  ondblclick={(e) => e.stopPropagation()}
/>

<style>
  .rename-input {
    width: 100%;
    height: 26px;
    padding: 0 6px;
    font-size: inherit;
    font-weight: 500;
    text-align: inherit;
    border-radius: var(--radius-sm);
    border-color: var(--focus);
    background: var(--field-focus);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 0 0 3px var(--accent-soft);
  }
  .rename-input:focus-visible {
    outline: none;
  }
</style>
