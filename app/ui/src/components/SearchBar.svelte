<!--
  The volume toolbar's search field, always shown; focusing it (or Ctrl/Cmd+F) opens search: typing
  searches the whole volume (or this folder: the scope switch on the filter line), one search
  per pause. Esc clears what is typed, then closes; ↓ / Enter go to the results.
  Hooks: [data-testid=search-field], [data-testid=search-close].
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Search, X } from '@lucide/svelte';
  import { tick } from 'svelte';
  import type { FileBrowser } from '../lib/stores/browser.svelte';

  let { browser }: { browser: FileBrowser } = $props();

  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
  const KEY = isMac ? '⌘F' : 'Ctrl+F';
  let input: HTMLInputElement | undefined = $state();

  // Ctrl/Cmd+F again (or the switch): focus the field and select what is typed
  $effect(() => {
    void browser.searchFocus;
    if (!browser.searchOpen) return;
    void tick().then(() => {
      input?.focus();
      input?.select();
    });
  });

  const listbox = () => input?.closest('.file-browser')?.querySelector<HTMLElement>('[role="listbox"]');

  function toResults() {
    const lb = listbox();
    if (!lb) return;
    if (browser.sel.focus === null && browser.order.length) browser.moveTo(0);
    lb.focus();
  }

  /** back to the folder, the keyboard on its listing (found before the field goes) */
  function close() {
    const lb = listbox();
    browser.closeSearch();
    void tick().then(() => lb?.focus());
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (browser.query) browser.setQuery('', true);
      else close();
    } else if (e.key === 'ArrowDown' || e.key === 'Enter') {
      e.preventDefault();
      if (e.key === 'Enter' && browser.query !== browser.appliedQuery) {
        browser.setQuery(browser.query, true);
        return;
      }
      toResults();
    }
  }
</script>

<div class="search-box" role="search" data-searching={browser.searching ? '' : undefined}>
  <Search size={15} aria-hidden="true" class="lens" />
  <input
    bind:this={input}
    class="input"
    type="search"
    value={browser.query}
    oninput={(e) => {
      if (!browser.searchOpen) browser.openSearch(false);
      browser.setQuery(e.currentTarget.value);
    }}
    onkeydown={keydown}
    onfocus={() => browser.searchOpen || browser.openSearch(false)}
    title={m.sr_open_title({ key: KEY })}
    placeholder={m.sr_placeholder()}
    aria-label={m.sr_field()}
    aria-keyshortcuts={isMac ? 'Meta+F' : 'Control+F'}
    spellcheck="false"
    autocomplete="off"
    data-testid="search-field"
  />
  <span class="busy" aria-hidden="true"></span>
  {#if browser.searchOpen}
    <button type="button" class="close" aria-label={m.sr_close()} title={m.sr_close()} data-testid="search-close" onclick={close}>
      <X size={14} />
    </button>
  {/if}
</div>

<style>
  .search-box {
    position: relative;
    display: flex;
    align-items: center;
    flex: 0 1 340px;
    min-width: 180px;
    margin-right: var(--space-2);
  }
  .search-box > :global(.lens) {
    position: absolute;
    left: 10px;
    color: var(--ink-3);
    pointer-events: none;
  }
  .search-box:focus-within > :global(.lens) {
    color: var(--accent);
  }
  .input {
    flex: 1;
    height: 30px;
    padding-left: 32px;
    padding-right: 52px;
    font-size: var(--text-md);
  }
  /* the platform's own clear key would double ours */
  .input::-webkit-search-decoration {
    -webkit-appearance: none;
    appearance: none;
  }
  /* (WebKit still draws its ⌫ with appearance: none: only display hides it) */
  .input::-webkit-search-cancel-button {
    display: none;
  }
  .close {
    position: absolute;
    right: 4px;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--ink-3);
  }
  .close:hover {
    background: color-mix(in oklab, var(--ink) 7%, transparent);
    color: var(--ink);
  }
  .close:active {
    background: color-mix(in oklab, var(--ink) 11%, transparent);
  }
  .close:focus-visible {
    outline-offset: -1px;
  }
  /* searching: a hairline of the ink runs under the text, nothing moves */
  .busy {
    position: absolute;
    left: 32px;
    right: 32px;
    bottom: 5px;
    height: 1px;
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
  }
  .busy::after {
    content: '';
    position: absolute;
    inset: 0;
    width: 40%;
    background: linear-gradient(90deg, transparent, var(--accent-line), transparent);
    /* swept on a registered property (styles/motion.css), not its transform */
    --nv-dx: -100%;
    translate: var(--nv-dx) 0;
  }
  .search-box[data-searching] .busy {
    opacity: 1;
    transition: opacity 160ms var(--ease-out) 180ms;
  }
  .search-box[data-searching] .busy::after {
    animation: sweep 900ms linear infinite;
  }
  @keyframes sweep {
    to {
      --nv-dx: 250%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .search-box[data-searching] .busy::after {
      animation: none;
      --nv-dx: 0px;
      width: 100%;
    }
  }
</style>
