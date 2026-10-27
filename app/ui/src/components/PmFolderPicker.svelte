<script lang="ts">
  import { Folder, FolderPlus, FolderX } from '@lucide/svelte';
  import { tick } from 'svelte';
  import { m } from '../paraglide/messages.js';

  let {
    folders,
    value = $bindable(''),
    inline = false,
    current = null,
    label,
    id = 'pm-folder',
    autofocus = false,
    onchoose,
  }: {
    folders: readonly string[];
    value?: string;
    inline?: boolean;
    current?: string | null;
    label: string;
    id?: string;
    autofocus?: boolean;
    onchoose?: (folder: string) => void;
  } = $props();

  type Opt = { folder: string; kind: 'none' | 'folder' | 'new' };

  const clean = (f: string) => f.split(/[/\\]/).map((x) => x.trim()).filter(Boolean).join('/');
  // svelte-ignore state_referenced_locally
  let text = $state(inline ? '' : value);
  // svelte-ignore state_referenced_locally
  let open = $state(inline);
  let active = $state(0);
  let typed = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLUListElement | undefined = $state();

  $effect(() => {
    if (!inline && !open && clean(text) !== clean(value)) text = value;
  });

  const options = $derived.by((): Opt[] => {
    const q = inline || typed ? text.trim().toLowerCase() : '';
    const typedPath = clean(text);
    const list: Opt[] = [];
    if (!q || m.pm_folder_none().toLowerCase().includes(q)) list.push({ folder: '', kind: 'none' });
    for (const f of folders) if (!q || f.toLowerCase().includes(q)) list.push({ folder: f, kind: 'folder' });
    if (q && typedPath && !folders.some((f) => f.toLowerCase() === typedPath.toLowerCase())) list.push({ folder: typedPath, kind: 'new' });
    return list;
  });

  $effect(() => {
    const want = clean(text).toLowerCase();
    const exact = want ? options.findIndex((o) => o.kind !== 'new' && o.folder.toLowerCase() === want) : -1;
    active = exact >= 0 ? exact : Math.min(active, Math.max(0, options.length - 1));
  });

  $effect(() => {
    if (autofocus) void tick().then(() => inputEl?.focus());
  });

  const optId = (i: number) => `${id}-opt-${i}`;

  function choose(o: Opt) {
    if (inline) {
      onchoose?.(o.folder);
      return;
    }
    value = o.folder;
    text = o.folder;
    open = false;
  }

  function keys(e: KeyboardEvent) {
    const n = options.length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!open) {
        open = true;
        return;
      }
      active = n ? (active + (e.key === 'ArrowDown' ? 1 : -1) + n) % n : 0;
      void tick().then(() => listEl?.querySelector(`#${optId(active)}`)?.scrollIntoView({ block: 'nearest' }));
    } else if ((e.key === 'Home' || e.key === 'End') && open && inline) {
      e.preventDefault();
      active = e.key === 'Home' ? 0 : n - 1;
    } else if (e.key === 'Enter') {
      if (!open || !options[active]) return;
      e.preventDefault();
      choose(options[active]);
    } else if (e.key === 'Escape' && open && !inline) {
      e.preventDefault();
      e.stopPropagation();
      open = false;
    }
  }

  function input(e: Event & { currentTarget: HTMLInputElement }) {
    text = e.currentTarget.value;
    open = true;
    typed = true;
    if (!inline) value = clean(text);
  }

  const name = (f: string) => f.split('/').at(-1) ?? f;
  const parent = (f: string) => f.split('/').slice(0, -1).join(' › ');
</script>

<div class="picker" class:inline data-testid="pm-folder-picker">
  <input
    bind:this={inputEl}
    {id}
    class="input"
    role="combobox"
    aria-label={label}
    aria-expanded={open}
    aria-controls="{id}-list"
    aria-autocomplete="list"
    aria-activedescendant={open && options[active] ? optId(active) : undefined}
    value={text}
    placeholder={m.pm_folder_search()}
    autocomplete="off"
    spellcheck="false"
    oninput={input}
    onkeydown={keys}
    onfocus={() => {
      open = true;
      typed = false;
    }}
    onblur={() => !inline && (open = false)}
  />
  {#if open}
    <ul bind:this={listEl} id="{id}-list" class="list" role="listbox" aria-label={label}>
      {#each options as o, i (o.kind + o.folder)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id={optId(i)}
          class="opt"
          role="option"
          aria-selected={i === active}
          data-folder={o.kind === 'new' ? undefined : o.folder}
          data-new={o.kind === 'new' ? o.folder : undefined}
          data-current={current !== null && o.kind !== 'new' && o.folder === current ? 'true' : undefined}
          style:--depth={o.kind === 'folder' && !((inline || typed) && text.trim()) ? o.folder.split('/').length - 1 : 0}
          onmousedown={(e) => e.preventDefault()}
          onmousemove={() => (active = i)}
          onclick={() => choose(o)}
        >
          {#if o.kind === 'none'}
            <FolderX size={14} aria-hidden="true" /><span class="o-name">{m.pm_folder_none()}</span>
          {:else if o.kind === 'new'}
            <FolderPlus size={14} aria-hidden="true" /><span class="o-name">{m.pm_folder_new({ name: o.folder.split('/').join(' › ') })}</span>
          {:else}
            <Folder size={14} aria-hidden="true" /><span class="o-name">{name(o.folder)}</span>
            {#if (inline || typed) && text.trim() && parent(o.folder)}<span class="o-parent">{parent(o.folder)}</span>{/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .picker {
    position: relative;
  }
  .picker .input {
    width: 100%;
  }
  .list {
    list-style: none;
    margin: 6px 0 0;
    padding: 4px;
    max-height: 240px;
    overflow-y: auto;
    border-radius: var(--radius-md);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .picker:not(.inline) .list {
    position: absolute;
    z-index: 30;
    left: 0;
    right: 0;
    margin-top: 4px;
    background: var(--paper-hi);
    box-shadow:
      inset 0 1px 0 var(--key-lip),
      var(--shadow-pop);
  }
  .inline .list {
    max-height: min(320px, 46vh);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 32px;
    padding: 0 10px 0 calc(10px + var(--depth, 0) * 16px);
    border-radius: var(--radius-sm);
    color: var(--ink-2);
    cursor: default;
  }
  .opt :global(svg) {
    flex: none;
    color: var(--ink-3);
  }
  .opt[aria-selected='true'] {
    background: var(--select-wash);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--select-ring) 22%, transparent);
  }
  .opt[aria-selected='true'] :global(svg) {
    color: var(--accent);
  }
  .opt[data-current='true'] .o-name {
    font-weight: 600;
  }
  .opt[data-current='true'] .o-name::after {
    content: '';
    display: inline-block;
    width: 5px;
    height: 5px;
    margin: 0 0 2px 8px;
    border-radius: 50%;
    background: var(--accent);
  }
  .opt[data-new] {
    color: var(--accent);
  }
  .opt[data-new] :global(svg) {
    color: var(--accent);
  }
  .o-name {
    min-width: 0;
    overflow-wrap: anywhere;
    line-height: 1.3;
    padding: 6px 0;
  }
  .o-parent {
    margin-left: auto;
    padding-left: var(--space-3);
    font-size: var(--text-xs);
    color: var(--ink-3);
    text-align: right;
    overflow-wrap: anywhere;
  }
</style>
