<script lang="ts">
  import { FolderOpen, HardDrive, Monitor, ShieldCheck, Usb } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { m } from '../paraglide/messages.js';
  import * as api from '../lib/api';
  import { formatBytes } from '../lib/format';
  import { drives } from '../lib/stores/drives.svelte';
  import type { Drive } from '../lib/types';

  interface Props {
    dir: string;
    labelledby?: string;
  }
  let { dir = $bindable(), labelledby }: Props = $props();

  let selected = $state<string | null>(null);
  let custom = $state<string | null>(null);
  let gone = $state<string | null>(null);

  function driveName(d: Drive): string {
    if (d.system) return m.drive_system();
    if (d.label) return d.label;
    return d.mount_path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || d.mount_path;
  }

  const usedPct = (d: Drive) => (d.total_bytes > 0 ? Math.min(100, Math.max(0, Math.round(((d.total_bytes - d.free_bytes) / d.total_bytes) * 100))) : 0);

  function match(list: Drive[], d: string): string | null {
    if (!d) return null;
    const exact = list.find((x) => x.folder === d);
    if (exact) return exact.id;
    return 'other';
  }

  onMount(() => {
    void drives.refresh();
  });

  $effect(() => {
    if (selected !== null || !drives.loaded || !dir) return;
    selected = match(drives.list, dir);
    if (selected === 'other') custom = dir;
  });

  $effect(() => {
    const list = drives.list;
    if (!selected || selected === 'other') return;
    if (list.some((d) => d.id === selected)) return;
    const was = untrackName(selected);
    const next = list[0];
    gone = was;
    if (next) {
      selected = next.id;
      dir = next.folder;
    } else {
      selected = custom ? 'other' : null;
      dir = custom ?? '';
    }
  });

  const seen = new Map<string, string>();
  $effect(() => {
    for (const d of drives.list) seen.set(d.id, driveName(d));
  });
  function untrackName(id: string): string {
    return seen.get(id) ?? id;
  }

  function choose(d: Drive) {
    gone = null;
    selected = d.id;
    dir = d.folder;
  }

  async function chooseOther() {
    const start = custom ?? undefined;
    const picked = await api.pickFolder(m.dialog_where_save(), start);
    if (!picked) return;
    gone = null;
    custom = picked;
    selected = 'other';
    dir = picked;
  }

  function keys(e: KeyboardEvent) {
    if (!['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    const radios = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="radio"]')];
    const i = radios.indexOf(document.activeElement as HTMLElement);
    const n = radios.length;
    const j = e.key === 'Home' ? 0 : e.key === 'End' ? n - 1 : e.key === 'ArrowRight' || e.key === 'ArrowDown' ? (i + 1) % n : (i - 1 + n) % n;
    e.preventDefault();
    radios[j]?.focus();
    const id = radios[j]?.dataset.driveId;
    const d = drives.list.find((x) => x.id === id);
    if (d) choose(d);
  }

  const kindIcon = (d: Drive) => (d.is_nicevault_device ? ShieldCheck : d.removable ? Usb : d.system ? Monitor : HardDrive);
  const tabStop = $derived(selected ?? drives.list[0]?.id ?? 'other');
</script>

<div class="drive-picker">
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="drives" role="radiogroup" aria-labelledby={labelledby} aria-busy={!drives.loaded} onkeydown={keys} data-testid="drive-picker">
    {#each drives.list as d (d.id)}
      {@const Icon = kindIcon(d)}
      {@const on = selected === d.id}
      <button
        type="button"
        role="radio"
        class="drive"
        aria-checked={on}
        tabindex={tabStop === d.id ? 0 : -1}
        data-testid="drive"
        data-drive-id={d.id}
        data-removable={d.removable}
        title={d.label && d.system ? `${d.label} · ${d.mount_path}` : d.mount_path}
        onclick={() => choose(d)}
      >
        <span class="glyph" aria-hidden="true"><Icon size={18} strokeWidth={1.6} /></span>
        <span class="name" data-testid="drive-name">{driveName(d)}</span>
        <span class="tags">
          {#if d.app_here}<span class="badge">{m.drive_app_here()}</span>{/if}
          {#if d.is_nicevault_device}<span class="badge">{m.drive_device()}</span>{/if}
          <span class="badge" data-tone="quiet">{d.removable ? m.drive_removable() : m.drive_internal()}</span>
        </span>
        <span class="bar" aria-hidden="true"><span class="fill" style:width="{usedPct(d)}%"></span></span>
        <span class="meta">
          <span class="num">{m.drive_free({ free: formatBytes(d.free_bytes), total: formatBytes(d.total_bytes) })}</span>
          {#if d.fs_type}<span class="fs">{d.fs_type}</span>{/if}
        </span>
      </button>
    {/each}
    <button
      type="button"
      role="radio"
      class="drive other"
      aria-checked={selected === 'other'}
      tabindex={tabStop === 'other' ? 0 : -1}
      data-testid="drive-other"
      onclick={chooseOther}
    >
      <span class="glyph" aria-hidden="true"><FolderOpen size={18} strokeWidth={1.6} /></span>
      <span class="name">{m.drive_other()}</span>
      <span class="meta other-path" class:none={!custom}>{custom ?? m.drive_other_hint()}</span>
    </button>
  </div>
  {#if gone}<p class="error-text" role="status" data-testid="drive-gone">{m.drive_gone({ name: gone })}</p>{/if}
</div>

<style>
  .drive-picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .drives {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-2);
  }
  .drive {
    position: relative;
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-areas:
      'glyph name'
      'glyph tags'
      'bar bar'
      'meta meta';
    column-gap: var(--space-2);
    row-gap: 4px;
    align-items: center;
    min-height: 92px;
    padding: 10px 12px;
    border: 0;
    border-radius: var(--radius-md);
    text-align: left;
    color: var(--ink);
    background: linear-gradient(180deg, var(--key-hi), var(--key-lo));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1px var(--key-edge),
      var(--lift-key);
  }
  .drive:hover {
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1px var(--key-edge-lo),
      var(--lift-key-hover);
  }
  .drive:active {
    transform: translateY(1px);
  }
  .drive[aria-checked='true'] {
    background: linear-gradient(180deg, color-mix(in oklab, var(--accent-soft) 70%, var(--key-hi)), var(--accent-soft));
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      inset 0 0 0 1.5px var(--accent),
      var(--lift-key);
  }
  .drive:focus-visible {
    outline-offset: 2px;
  }
  .glyph {
    grid-area: glyph;
    align-self: start;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    color: var(--ink-3);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .drive[aria-checked='true'] .glyph {
    color: var(--accent);
  }
  .name {
    grid-area: name;
    min-width: 0;
    font-size: var(--text-base);
    font-weight: 600;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }
  .tags {
    grid-area: tags;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .bar {
    grid-area: bar;
    display: block;
    height: 4px;
    margin-top: 4px;
    border-radius: 2px;
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 1px var(--field-shade),
      0 1px 0 var(--paper-hi);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: var(--ink-3);
  }
  .drive[aria-checked='true'] .fill {
    background: linear-gradient(180deg, var(--accent), var(--key-accent-hi));
  }
  .meta {
    grid-area: meta;
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .fs {
    font-family: var(--font-mono);
    text-transform: lowercase;
  }
  .other {
    grid-template-areas:
      'glyph name'
      'meta meta';
    align-content: center;
  }
  .other-path {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
    color: var(--ink-2);
  }
  .other-path.none {
    font-family: var(--font-ui);
    color: var(--ink-3);
  }
</style>
