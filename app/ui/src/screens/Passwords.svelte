<!--
  Password: the open volume's password manager. Head: title, search-as-you-type, Nuova voce,
  Generatore, Importa. Body: filters (all / favourites / 2FA / folders) and the list on the
  left, the chosen entry (or the multi-selection) on the right. A strip at the foot counts down
  the clipboard wipe.
  The list renders only the rows in view (fixed row height, ROW px), so thousands of entries
  scroll and filter at frame rate; site icons are <img> on nvault:// (or a monogram).
  Folders: "Sposta in cartella…" (context menu, Ctrl/Cmd+Shift+M, or drag entries onto a folder
  of the tree / onto "Tutte" to take them out), rename / delete from a folder's context menu
  (F2 / Canc on a focused folder).
  Keys: Ctrl/Cmd+F search · ↑/↓ (Shift extends) Home End in the list · Ctrl/Cmd+A all ·
  Ctrl/Cmd+click, Shift+click pick more · Esc back to one · Enter edit · Canc delete ·
  Menu / Shift+F10 context menu · Ctrl/Cmd+C password · Ctrl/Cmd+B username · Ctrl/Cmd+T 2FA code
  (copies of the selected entry; Ctrl+C copies selected text when there is some).
  Switches: ?screen=passwords&entry=<id|title>&q=<search>&pm=<N extra entries>&dialog=pm-edit|pm-new|pm-import|pm-import-2|pm-generator|pm-history|pm-delete|pm-move
-->
<script lang="ts">
  import { ClipboardCheck, Copy, Folder, FolderInput, KeyRound, MousePointerClick, Pencil, Plus, Search, SearchX, Star, Trash, Upload, Wand2, X } from '@lucide/svelte';
  import { onDestroy, onMount, tick } from 'svelte';
  import * as api from '../lib/api';
  import { app } from '../lib/stores/app.svelte';
  import type { EntryId, PmEntrySummary } from '../lib/api';
  import { formatNumber } from '../lib/format';
  import { switches } from '../lib/switches';
  import { pmIcons } from '../lib/pmIcons.svelte';
  import { PmStore, type PmFilter } from '../lib/stores/pm.svelte';
  import { vaults } from '../lib/stores/vaults.svelte';
  import { m } from '../paraglide/messages.js';
  import Band from '../ornament/Band.svelte';
  import PwLedger from '../components/PwLedger.svelte';
  import Dialog from '../components/Dialog.svelte';
  import Menu, { type MenuItem } from '../components/Menu.svelte';
  import PmDetail from '../components/PmDetail.svelte';
  import MobileBack from '../components/MobileBack.svelte';
  import { isMobile } from '../lib/platform';
  import PmEditor from '../components/PmEditor.svelte';
  import PmFolderPicker from '../components/PmFolderPicker.svelte';
  import PmGenerator from '../components/PmGenerator.svelte';
  import PmImport from '../components/PmImport.svelte';

  /** row pitch of the virtual list (px): .entry height + gap */
  const ROW = 52;
  /** rows rendered above / below the viewport */
  const OVERSCAN = 10;
  /** the rendered window moves in steps of this many rows: most scroll frames touch no DOM */
  const STEP = 8;
  /** dataTransfer type of dragged entries (JSON EntryId[]) */
  const PM_DRAG = 'application/x-nv-pm-entries';

  const store = new PmStore(vaults.activeId!);
  let searchEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let rowsEl: HTMLElement | undefined = $state();
  let editing = $state<'new' | 'edit' | null>(null);
  let genOpen = $state(false);
  let importOpen = $state(false);
  let importStep = $state<1 | 2>(1);
  let deleting = $state(false);
  let historyOpen = $state(false);
  /** entries in the "Sposta in cartella" dialog */
  let moving = $state<EntryId[] | null>(null);
  let renaming = $state<string | null>(null);
  let renameTo = $state('');
  let renameError = $state<string | null>(null);
  let folderDeleting = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; items: MenuItem[]; label: string; title?: string; back: HTMLElement | null } | null>(null);
  /** folder (or "" = Tutte) under a drag */
  let dropOn = $state<string | null>(null);
  /** first row in view and the viewport height (virtual list) */
  let first = $state(0);
  let viewH = $state(600);
  /** icons that failed to load: the monogram stays */
  let broken = $state.raw<Set<string>>(new Set());
  /** the last entry opened with the pointer (its card decodes once, PmDetail `opened`) */
  let opened = $state<{ id: EntryId; n: number } | null>(null);
  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

  onMount(async () => {
    await store.load();
    void store.startIcons();
    if (api.isTauri) return;
    // URL switches (mock): land on an entry, a search or a dialog for screenshots
    if (switches.q) {
      store.setQuery(switches.q);
      store.applied = switches.q;
    }
    const want = switches.entry?.toLowerCase();
    const hit = want ? store.all.find((e) => e.id === switches.entry || e.title.toLowerCase() === want) : isMobile ? undefined : store.all[0]; // a phone lands on the list
    if (hit) {
      await store.select(hit.id);
      await tick();
      reveal(hit.id);
    }
    const d = switches.dialog;
    if (d === 'pm-edit' && store.detail) editing = 'edit';
    else if (d === 'pm-new') editing = 'new';
    else if (d === 'pm-generator') genOpen = true;
    else if (d === 'pm-import' || d === 'pm-import-2') {
      importStep = d === 'pm-import-2' ? 2 : 1;
      importOpen = true;
    } else if (d === 'pm-history' && store.detail) historyOpen = true;
    else if (d === 'pm-delete' && store.detail) deleting = true;
    else if (d === 'pm-move' && store.selectedId) moving = [store.selectedId];
  });
  onDestroy(() => {
    store.dispose();
    pmIcons.drop(store.vault);
  });

  const FIXED: { f: PmFilter; label: () => string; icon: typeof Star }[] = [
    { f: { kind: 'all' }, label: m.pm_all, icon: KeyRound },
    { f: { kind: 'favorites' }, label: m.pm_favorites, icon: Star },
    { f: { kind: 'totp' }, label: m.pm_with_totp, icon: ClipboardCheck },
  ];

  // ---------------- virtual list ----------------

  const total = $derived(store.visible.length);
  const span = $derived(Math.ceil(viewH / ROW) + 1);
  const from = $derived(Math.max(0, Math.min(first, total - span) - OVERSCAN));
  const to = $derived(Math.min(total, first + STEP + span + OVERSCAN));
  const rows = $derived(store.visible.slice(from, to));

  function onScroll() {
    if (!listEl) return;
    const raw = Math.max(0, Math.floor((listEl.scrollTop - (rowsEl?.offsetTop ?? 0)) / ROW));
    const f = raw - (raw % STEP);
    if (f !== first) first = f;
  }

  // a new search or filter starts at the top
  $effect(() => {
    void store.applied;
    void store.filter;
    if (listEl) listEl.scrollTop = 0;
    first = 0;
  });

  /** Scrolls the list so the row of `id` is in view. */
  function reveal(id: EntryId | null) {
    if (!id || !listEl) return;
    const i = store.positions.get(id);
    if (i === undefined) return;
    const top = (rowsEl?.offsetTop ?? 0) + i * ROW;
    if (top < listEl.scrollTop) listEl.scrollTop = top;
    else if (top + ROW > listEl.scrollTop + listEl.clientHeight) listEl.scrollTop = top + ROW - listEl.clientHeight;
    onScroll();
  }

  // ---------------- keys ----------------

  const anyDialog = () => !!document.querySelector('dialog[open]');
  const inField = (t: EventTarget | null) => t instanceof HTMLElement && !!t.closest('input, textarea, [contenteditable="true"]');
  /** what a bulk action works on: the picked entries, else the one in the detail */
  const targets = () => (store.picked.length ? store.picked : store.selectedId ? [store.selectedId] : []);

  function keys(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (!mod || e.altKey || anyDialog() || menu || app.covered) return;
    const k = e.key.toLowerCase();
    if (e.shiftKey) {
      if (k === 'm' && targets().length) {
        e.preventDefault();
        moving = targets();
      }
      return;
    }
    if (k === 'f') {
      e.preventDefault();
      searchEl?.focus();
      searchEl?.select();
      return;
    }
    const sel = store.selected;
    if (!sel) return;
    if (k === 'c') {
      // the person's own text selection (or a field's) is copied as usual
      if (inField(e.target) || (window.getSelection()?.toString() ?? '') !== '') return;
      if (!sel.has_password) return;
      e.preventDefault();
      void store.copy('password', m.pm_password(), sel.id);
    } else if (k === 'b' && !inField(e.target)) {
      e.preventDefault();
      void store.copy('username', m.pm_username(), sel.id);
    } else if (k === 't' && !inField(e.target) && sel.has_totp) {
      e.preventDefault();
      void store.copy('totp_code', m.pm_totp(), sel.id);
    }
  }

  function listKeys(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      store.step(e.key === 'ArrowDown' ? 1 : -1, { shift: e.shiftKey });
      void tick().then(() => reveal(store.selectedId));
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      store.step(e.key === 'Home' ? -1e9 : 1e9, { shift: e.shiftKey });
      void tick().then(() => reveal(store.selectedId));
    } else if (e.key === 'Enter' && store.detail && store.picked.length <= 1) {
      e.preventDefault();
      editing = 'edit';
    } else if (e.key === 'Delete' && store.detail && store.picked.length <= 1) {
      e.preventDefault();
      deleting = true;
    } else if (mod && !e.shiftKey && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      store.selectAllVisible();
    } else if (e.key === 'Escape' && store.picked.length > 1) {
      e.preventDefault();
      void store.select(store.selectedId);
    } else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      e.preventDefault();
      const row = listEl?.querySelector<HTMLElement>(`[id="pm-${store.selectedId}"]`);
      const r = (row ?? listEl)?.getBoundingClientRect();
      if (r && store.selected) openEntryMenu(store.selected, r.left + 24, r.bottom - 6, listEl ?? null);
    }
  }

  function searchKeys(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      listEl?.focus();
      if (!store.selectedId) store.step(1);
    } else if (e.key === 'Escape' && store.query) {
      e.preventDefault();
      store.setQuery('');
    }
  }

  function folderKeys(e: KeyboardEvent, folder: string) {
    if (e.key === 'F2') {
      e.preventDefault();
      startRename(folder);
    } else if (e.key === 'Delete') {
      e.preventDefault();
      folderDeleting = folder;
    } else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      openFolderMenu(folder, r.left + 24, r.bottom - 4, e.currentTarget as HTMLElement);
    }
  }

  // ---------------- menus ----------------

  function openEntryMenu(e: PmEntrySummary, x: number, y: number, back: HTMLElement | null) {
    const many = store.picked.length > 1 && store.sel.selected.has(e.id);
    const items: MenuItem[] = many
      ? [
          { id: 'move', label: m.pm_move_title({ count: store.picked.length, n: String(store.picked.length) }) + '…', icon: FolderInput, shortcut: isMac ? '⌘⇧M' : 'Ctrl+Shift+M', action: () => (moving = [...store.picked]) },
          { id: 'clear', label: m.pm_clear_selection(), icon: X, action: () => void store.select(e.id) },
        ]
      : [
          { id: 'copy-pw', label: m.pm_copy_password(), icon: Copy, shortcut: isMac ? '⌘C' : 'Ctrl+C', disabled: !e.has_password, action: () => void store.copy('password', m.pm_password(), e.id) },
          { id: 'copy-user', label: m.pm_copy_username(), icon: Copy, shortcut: isMac ? '⌘B' : 'Ctrl+B', disabled: !e.username, action: () => void store.copy('username', m.pm_username(), e.id) },
          { id: 'move', label: m.pm_move_to(), icon: FolderInput, shortcut: isMac ? '⌘⇧M' : 'Ctrl+Shift+M', separatorBefore: true, action: () => (moving = [e.id]) },
          { id: 'fav', label: e.favorite ? m.pm_unmark_favorite() : m.pm_mark_favorite(), icon: Star, action: () => void store.toggleFavorite(e.id) },
          { id: 'edit', label: m.action_edit(), icon: Pencil, shortcut: '↵', action: () => void store.touch(e.id).then(() => (editing = 'edit')) },
          { id: 'delete', label: m.action_delete(), icon: Trash, danger: true, separatorBefore: true, action: () => void store.touch(e.id).then(() => (deleting = true)) },
        ];
    menu = { x, y, items, label: m.pm_entry_menu({ name: e.title }), title: many ? m.pm_selected_n({ count: store.picked.length, n: formatNumber(store.picked.length) }) : e.title, back };
  }

  function openFolderMenu(folder: string, x: number, y: number, back: HTMLElement | null) {
    menu = {
      x,
      y,
      label: m.pm_folder_menu({ name: folder.split('/').at(-1) ?? folder }),
      title: folder.split('/').at(-1) ?? folder,
      back,
      items: [
        { id: 'rename', label: m.pm_folder_rename(), icon: Pencil, shortcut: 'F2', action: () => startRename(folder) },
        { id: 'delete', label: m.pm_folder_delete(), icon: Trash, danger: true, action: () => (folderDeleting = folder) },
      ],
    };
  }

  function closeMenu(restore: boolean) {
    const back = menu?.back;
    menu = null;
    if (restore) back?.focus();
  }

  function startRename(folder: string) {
    renaming = folder;
    renameTo = folder;
    renameError = null;
  }

  async function doRename(e?: Event) {
    e?.preventDefault();
    if (!renaming || !renameTo.trim()) return;
    const err = await store.renameFolder(renaming, renameTo);
    if (err) renameError = err;
    else renaming = null;
  }

  async function doMove(folder: string) {
    const ids = moving;
    moving = null;
    if (ids) await store.move(ids, folder);
    listEl?.focus();
  }

  /** the folder every moving entry is in now (marked in the picker), else null */
  const movingFrom = $derived.by(() => {
    if (!moving) return null;
    const fs = new Set(moving.map((id) => store.all.find((e) => e.id === id)?.folder ?? ''));
    return fs.size === 1 ? [...fs][0] : null;
  });

  // ---------------- drag & drop ----------------

  function dragStart(ev: DragEvent, e: PmEntrySummary) {
    void store.touch(e.id);
    const ids = store.sel.selected.has(e.id) && store.picked.length > 1 ? store.picked : [e.id];
    ev.dataTransfer?.setData(PM_DRAG, JSON.stringify(ids));
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = 'move';
  }

  const dragged = (ev: DragEvent) => !!ev.dataTransfer && [...ev.dataTransfer.types].includes(PM_DRAG);

  function dragOver(ev: DragEvent, folder: string) {
    if (!dragged(ev)) return;
    ev.preventDefault();
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = 'move';
    dropOn = folder;
  }

  function drop(ev: DragEvent, folder: string) {
    if (!dragged(ev)) return;
    ev.preventDefault();
    dropOn = null;
    let ids: EntryId[] = [];
    try {
      ids = JSON.parse(ev.dataTransfer?.getData(PM_DRAG) || '[]');
    } catch {
      /* not ours */
    }
    if (Array.isArray(ids) && ids.length) void store.move(ids.map(String), folder);
  }

  const initial = (t: string) => (t.trim().match(/\p{L}|\p{N}/u)?.[0] ?? '·').toLocaleUpperCase();
  const shortcuts = $derived(isMac ? m.pm_shortcuts_mac() : m.pm_shortcuts());
  const iconHost = (icon: string) => icon.split('/')[0] ?? '';
</script>

<svelte:window onkeydown={keys} />

<section class="passwords sheet" aria-labelledby="pm-title" data-testid="passwords">
  <header class="pm-head">
    <div class="line">
      <h1 id="pm-title" class="title display">{m.pm_title()}</h1>
      {#if store.loaded}<span class="count">{m.pm_count({ count: store.all.length, n: formatNumber(store.all.length) })}</span>{/if}
      <span class="spacer"></span>
      <button type="button" class="btn" onclick={() => (genOpen = true)} data-testid="pm-open-generator"><Wand2 size={16} aria-hidden="true" /> <span class="label-text">{m.pm_generator()}</span></button>
      <button type="button" class="btn" onclick={() => ((importStep = 1), (importOpen = true))} data-testid="pm-open-import"><Upload size={16} aria-hidden="true" /> <span class="label-text">{m.pm_import()}</span></button>
      <button type="button" class="btn primary" onclick={() => (editing = 'new')} data-testid="pm-new"><Plus size={16} aria-hidden="true" /> {m.pm_new()}</button>
    </div>
    <div class="search">
      <Search size={15} aria-hidden="true" />
      <input
        bind:this={searchEl}
        class="input"
        type="search"
        value={store.query}
        oninput={(e) => store.setQuery(e.currentTarget.value)}
        onkeydown={searchKeys}
        placeholder={m.pm_search_ph()}
        aria-label={m.pm_search()}
        aria-keyshortcuts={isMac ? 'Meta+F' : 'Control+F'}
        aria-controls="pm-list"
        spellcheck="false"
        autocomplete="off"
        data-testid="pm-search"
      />
      <kbd class="kbd">{isMac ? '⌘F' : 'Ctrl+F'}</kbd>
    </div>
    <Band />
  </header>

  <div class="body">
    <aside class="side">
      <nav class="filters" aria-label={m.pm_filters()}>
        {#each FIXED as x (x.f.kind)}
          {@const Icon = x.icon}
          <button
            type="button"
            class="filter"
            aria-current={store.isFilter(x.f) ? 'true' : undefined}
            data-filter={x.f.kind}
            data-drop={x.f.kind === 'all' && dropOn === '' ? 'true' : undefined}
            onclick={() => (store.filter = x.f)}
            ondragover={x.f.kind === 'all' ? (ev) => dragOver(ev, '') : undefined}
            ondragleave={() => (dropOn = null)}
            ondrop={x.f.kind === 'all' ? (ev) => drop(ev, '') : undefined}
          >
            <Icon size={14} aria-hidden="true" /><span class="f-name">{x.label()}</span><span class="f-n num">{store.countIn(x.f)}</span>
          </button>
        {/each}
        {#each store.folders as f (f)}
          {@const depth = f.split('/').length - 1}
          <button
            type="button"
            class="filter"
            style:--depth={depth}
            aria-current={store.isFilter({ kind: 'folder', folder: f }) ? 'true' : undefined}
            aria-keyshortcuts="F2 Delete"
            data-filter="folder"
            data-folder={f}
            data-drop={dropOn === f ? 'true' : undefined}
            onclick={() => (store.filter = { kind: 'folder', folder: f })}
            onkeydown={(e) => folderKeys(e, f)}
            oncontextmenu={(e) => {
              e.preventDefault();
              openFolderMenu(f, e.clientX, e.clientY, e.currentTarget);
            }}
            ondragover={(ev) => dragOver(ev, f)}
            ondragleave={() => (dropOn = null)}
            ondrop={(ev) => drop(ev, f)}
          >
            <Folder size={14} aria-hidden="true" /><span class="f-name">{f.split('/').at(-1)}</span><span class="f-n num">{store.countIn({ kind: 'folder', folder: f })}</span>
          </button>
        {/each}
      </nav>

      <div
        bind:this={listEl}
        bind:clientHeight={viewH}
        id="pm-list"
        class="entries"
        role="listbox"
        tabindex="0"
        aria-label={m.pm_entries()}
        aria-multiselectable="true"
        aria-activedescendant={store.selectedId && store.positions.has(store.selectedId) ? `pm-${store.selectedId}` : undefined}
        onkeydown={listKeys}
        onscroll={onScroll}
        data-testid="pm-list"
        data-scroller
        data-q={store.applied}
        data-count={total}
      >
        {#if store.loaded && store.all.length === 0}
          <!-- an empty register: ruled lines waiting for their first entry -->
          <div class="ledger" data-state="empty" aria-hidden="true">
            {#each { length: 6 } as _, i (i)}<span class="ledger-line" style:--i={i}></span>{/each}
          </div>
        {:else if store.loaded && total === 0}
          <div class="none" data-state="no-match">
            <span class="none-glyph" aria-hidden="true"><SearchX size={18} strokeWidth={1.6} /></span>
            <p class="none-text">{m.pm_no_match()}</p>
            {#if store.query}<button type="button" class="btn sm ghost" onclick={() => store.setQuery('')}>{m.pm_clear_search()}</button>{/if}
          </div>
        {:else}
          <div bind:this={rowsEl} class="rows" style:height="{total * ROW}px">
            {#each rows as e, k (e.id)}
              {@const i = from + k}
              {@const src = e.icon ? pmIcons.src(store.vault, e.icon) : null}
              <!-- keyboard is on the listbox (aria-activedescendant) -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div
                id="pm-{e.id}"
                class="entry"
                role="option"
                tabindex="-1"
                style:transform="translateY({i * ROW}px)"
                aria-selected={store.sel.selected.has(e.id)}
                aria-posinset={i + 1}
                aria-setsize={total}
                data-current={store.selectedId === e.id ? 'true' : undefined}
                data-testid="pm-entry"
                data-title={e.title}
                draggable="true"
                onclick={(ev) => {
                  const one = !ev.shiftKey && !(isMac ? ev.metaKey : ev.ctrlKey);
                  if (one && e.id !== store.selectedId) opened = { id: e.id, n: (opened?.n ?? 0) + 1 };
                  void store.select(e.id, { shift: ev.shiftKey, toggle: isMac ? ev.metaKey : ev.ctrlKey });
                  listEl?.focus();
                }}
                ondblclick={() => (editing = 'edit')}
                oncontextmenu={(ev) => {
                  ev.preventDefault();
                  // a phone keeps the list behind the sheet (the entry opens only for Modifica / Elimina)
                  if (!isMobile) void store.touch(e.id);
                  openEntryMenu(e, ev.clientX, ev.clientY, listEl ?? null);
                }}
                ondragstart={(ev) => dragStart(ev, e)}
                ondragend={() => (dropOn = null)}
              >
                <!-- one frame for every entry: the site's own icon on an enamel plate, or its initial
                     engraved on a plate of the volume's ink -->
                {#if e.icon && src && !broken.has(e.icon)}
                  <span class="plate" data-plate="site">
                    <img
                      class="site-icon"
                      {src}
                      alt={m.pm_icon_of({ host: iconHost(e.icon) })}
                      width="20"
                      height="20"
                      decoding="async"
                      draggable="false"
                      data-testid="pm-icon"
                      onerror={() => (broken = new Set([...broken, e.icon!]))}
                    />
                  </span>
                {:else}
                  <span class="plate mono-badge" data-plate="initial" aria-hidden="true">{initial(e.title)}</span>
                {/if}
                <span class="e-main">
                  <span class="e-title">{e.title}</span>
                  <span class="e-sub">{e.username || e.urls[0] || e.folder || ' '}</span>
                </span>
                <span class="e-badges">
                  {#if e.has_totp}<span class="badge" title={m.pm_totp()}>{m.pm_badge_totp()}</span>{/if}
                  {#if e.favorite}<Star size={13} class="star" fill="currentColor" aria-label={m.pm_favorite()} />{/if}
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </aside>

    <div class="main">
      {#if store.picked.length > 1}
        <div class="many" data-testid="pm-multi" data-count={store.picked.length}>
          <p class="display many-n">{m.pm_selected_n({ count: store.picked.length, n: formatNumber(store.picked.length) })}</p>
          <p class="hint">{m.pm_selection_hint()}</p>
          <div class="many-actions">
            <button type="button" class="btn primary" onclick={() => (moving = [...store.picked])} data-testid="pm-move-many"><FolderInput size={15} aria-hidden="true" /> {m.pm_move_to()}</button>
            <button type="button" class="btn" onclick={() => void store.select(store.selectedId)}>{m.pm_clear_selection()}</button>
          </div>
        </div>
      {:else if store.detail && store.selectedId === store.detail.id}
        <MobileBack onback={() => void store.select(null)} />
        <PmDetail
          {store}
          entry={store.detail}
          opened={opened && opened.id === store.detail.id ? opened.n : 0}
          onedit={() => (editing = 'edit')}
          ondelete={() => (deleting = true)}
          onmove={() => store.selectedId && (moving = [store.selectedId])}
          bind:historyOpen
        />
      {:else if store.loaded && store.all.length === 0}
        <!-- the first page of the register: a seal, one sentence, the two ways in -->
        <div class="empty" data-state="empty-detail">
          <span class="empty-seal"><PwLedger size={120} /></span>
          <p class="empty-title display">{m.pm_empty()}</p>
          <p class="hint empty-hint">{m.pm_empty_hint()}</p>
          <div class="empty-actions">
            <button type="button" class="btn primary" onclick={() => (editing = 'new')}><Plus size={16} aria-hidden="true" /> {m.pm_new()}</button>
            <button type="button" class="btn" onclick={() => ((importStep = 1), (importOpen = true))}><Upload size={16} aria-hidden="true" /> {m.pm_import()}</button>
          </div>
        </div>
      {:else if store.loaded}
        <div class="pick" data-state="none-selected">
          <span class="none-glyph" aria-hidden="true"><MousePointerClick size={18} strokeWidth={1.6} /></span>
          <p class="none-text">{m.pm_none_selected()}</p>
        </div>
      {/if}
    </div>
  </div>

  <footer class="status" aria-live="polite">
    {#if store.copied}
      <span class="copied" data-testid="pm-copied" data-left={store.copyLeft ?? ''}>
        <ClipboardCheck size={14} aria-hidden="true" />
        {store.copyLeft !== null ? m.pm_copied({ field: store.copied.label, n: String(store.copyLeft) }) : m.pm_copied_keep({ field: store.copied.label })}
      </span>
      {#if store.copied.until}
        <span class="drain" style:--left={Math.min(1, (store.copyLeft ?? 0) / Math.max(1, store.copied.seconds))}></span>
      {/if}
    {:else}
      <span class="keys">{shortcuts}</span>
    {/if}
  </footer>
</section>

{#if menu}
  <Menu items={menu.items} x={menu.x} y={menu.y} label={menu.label} title={menu.title} onclose={closeMenu} />
{/if}

<PmEditor open={editing !== null} {store} entry={editing === 'edit' ? store.detail : null} onclose={() => (editing = null)} />
<PmGenerator open={genOpen} onclose={() => (genOpen = false)} />
<PmImport open={importOpen} {store} initialStep={importStep} onclose={() => (importOpen = false)} />
<Dialog open={deleting} title={m.pm_delete_title({ name: store.detail?.title ?? '' })} variant="danger" onclose={() => (deleting = false)} describedby="pm-delete-desc">
  <p id="pm-delete-desc">{m.pm_delete_desc()}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (deleting = false)}>{m.action_cancel()}</button>
    <!-- svelte-ignore a11y_autofocus -->
    <button
      type="button"
      class="btn danger"
      autofocus
      data-testid="pm-delete-confirm"
      onclick={() => {
        deleting = false;
        if (store.selectedId) void store.remove(store.selectedId);
      }}>{m.action_delete()}</button
    >
  {/snippet}
</Dialog>
<Dialog open={moving !== null} title={m.pm_move_title({ count: moving?.length ?? 0, n: String(moving?.length ?? 0) })} onclose={() => (moving = null)}>
  <div class="move" data-testid="pm-move">
    {#if moving}
      <PmFolderPicker id="pm-move-folder" inline autofocus folders={store.folders} current={movingFrom} label={m.pm_folder_pick()} onchoose={doMove} />
    {/if}
  </div>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (moving = null)}>{m.action_cancel()}</button>
  {/snippet}
</Dialog>
<Dialog open={renaming !== null} title={m.pm_folder_rename_title({ name: renaming?.split('/').at(-1) ?? '' })} onclose={() => (renaming = null)}>
  <form class="rename" id="pm-rename-form" onsubmit={doRename} data-testid="pm-folder-rename">
    <label class="field-label" for="pm-rename-input">{m.pm_folder_name()}</label>
    <!-- svelte-ignore a11y_autofocus -->
    <input id="pm-rename-input" class="input" bind:value={renameTo} autocomplete="off" spellcheck="false" autofocus aria-describedby="pm-rename-hint" aria-invalid={!!renameError} />
    <p id="pm-rename-hint" class={renameError ? 'error-text' : 'hint'}>{renameError ?? m.pm_folder_rename_hint()}</p>
  </form>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (renaming = null)}>{m.action_cancel()}</button>
    <button type="submit" form="pm-rename-form" class="btn primary" disabled={!renameTo.trim()} data-testid="pm-folder-rename-save">{m.fb_rename()}</button>
  {/snippet}
</Dialog>
<Dialog open={folderDeleting !== null} title={m.pm_folder_delete_title({ name: folderDeleting?.split('/').at(-1) ?? '' })} variant="danger" onclose={() => (folderDeleting = null)} describedby="pm-fdel-desc">
  <p id="pm-fdel-desc">{m.pm_folder_delete_desc()}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (folderDeleting = null)}>{m.action_cancel()}</button>
    <!-- svelte-ignore a11y_autofocus -->
    <button
      type="button"
      class="btn danger"
      autofocus
      data-testid="pm-folder-delete-confirm"
      onclick={() => {
        const f = folderDeleting;
        folderDeleting = null;
        if (f) void store.deleteFolder(f);
      }}>{m.action_delete()}</button
    >
  {/snippet}
</Dialog>

<style>
  .passwords {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .pm-head {
    --bleed-start: calc(var(--space-5) + 6px);
    --bleed-end: var(--space-5);
    flex: none;
    padding: var(--space-5) var(--space-5) 0 calc(var(--space-5) + 6px);
  }
  .line {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
  }
  .title {
    font-size: clamp(30px, 3vw, 40px);
    font-weight: 500;
    line-height: 1.05;
    color: var(--title-ink);
  }
  .count {
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
  .spacer {
    flex: 1;
  }
  .line .btn {
    align-self: center;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    margin: var(--space-3) 0 var(--space-3);
    max-width: 560px;
  }
  .search > :global(svg) {
    position: absolute;
    left: 11px;
    color: var(--ink-3);
    pointer-events: none;
  }
  .search .input {
    flex: 1;
    padding-left: 34px;
    padding-right: 64px;
  }
  .kbd {
    position: absolute;
    right: 10px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-4);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(260px, 340px) minmax(0, 1fr);
  }
  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--paper-rule);
  }
  .filters {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-height: 38%;
    overflow-y: auto;
    padding: var(--space-3) var(--space-3) var(--space-2) var(--space-5);
    border-bottom: 1px solid var(--paper-rule);
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    padding: 0 8px 0 calc(8px + var(--depth, 0) * 14px);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--ink-2);
    font-size: var(--text-md);
    text-align: left;
  }
  .filter :global(svg) {
    color: var(--ink-3);
    flex: none;
  }
  .filter:hover {
    color: var(--ink);
    background: color-mix(in oklab, var(--ink) 5%, transparent);
  }
  .filter[aria-current='true'] {
    color: var(--ink);
    font-weight: 600;
    background: var(--accent-soft);
  }
  .filter[aria-current='true'] :global(svg) {
    color: var(--accent);
  }
  .f-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .f-n {
    font-size: var(--text-xs);
    color: var(--ink-3);
  }
  .entries {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2) var(--space-3) var(--space-4) var(--space-5);
    outline: none;
  }
  /* virtual list: only the rows in view exist, each placed by translateY(i * ROW) */
  .rows {
    position: relative;
    contain: strict;
    width: 100%;
  }
  .entry {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 50px;
    padding: 0 8px;
    border-radius: var(--radius-md);
    cursor: default;
    contain: layout style;
  }
  .entry:hover {
    background: color-mix(in oklab, var(--ink) 4%, transparent);
  }
  .entry[aria-selected='true'] {
    background: var(--select-wash);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--select-ring) 22%, transparent);
  }
  .entries:focus-visible .entry[data-current='true'] {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  /* the plate every entry's mark sits on: a small rounded square, lifted off the page */
  .plate {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 9px;
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.5),
      inset 0 0 0 1px rgb(var(--cast) / 0.12),
      0 1px 1px rgb(var(--cast) / 0.25),
      0 2px 4px -2px rgb(var(--cast) / 0.4);
  }
  /* a site's icon: enamel, the light plate icons are drawn for, in either theme */
  .plate[data-plate='site'] {
    background: linear-gradient(180deg, var(--plate-hi), var(--plate-lo));
  }
  .site-icon {
    display: block;
    width: 20px;
    height: 20px;
    object-fit: contain;
  }
  .filter[data-drop='true'] {
    color: var(--ink);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .many {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-4);
  }
  .many-n {
    font-size: var(--display-s);
    color: var(--title-ink);
  }
  .many-actions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .move {
    min-width: min(420px, 80vw);
  }
  .rename {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: min(380px, 80vw);
  }
  /* no icon: the initial, engraved on a plate of the volume's ink, a hairline inside its edge */
  .mono-badge {
    position: relative;
    font-family: var(--font-display);
    font-size: 17px;
    font-weight: 500;
    line-height: 1;
    padding-bottom: 1px;
    color: var(--accent);
    background: linear-gradient(180deg, color-mix(in oklab, var(--accent-soft) 70%, var(--key-hi)), var(--accent-soft));
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.12),
      inset 0 0 0 1px color-mix(in srgb, var(--accent-line) 45%, transparent),
      0 1px 1px rgb(var(--cast) / 0.25),
      0 2px 4px -2px rgb(var(--cast) / 0.4);
  }
  .mono-badge::after {
    content: '';
    position: absolute;
    inset: 3px;
    border-radius: 6px;
    box-shadow: inset 0 0 0 0.75px color-mix(in srgb, var(--accent-line) 55%, transparent);
    pointer-events: none;
  }
  .entry[aria-selected='true'] .mono-badge {
    color: var(--accent-ink);
    background: linear-gradient(180deg, var(--key-accent-hi), var(--key-accent-lo));
  }
  .entry[aria-selected='true'] .mono-badge::after {
    box-shadow: inset 0 0 0 0.75px rgb(255 255 255 / 0.35);
  }
  .e-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .e-title {
    font-weight: 600;
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .e-sub {
    font-size: var(--text-sm);
    color: var(--ink-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .e-badges {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
  }
  /* nothing yet: the list is a ruled register, its lines fading down the page */
  .ledger {
    display: flex;
    flex-direction: column;
    padding: var(--space-2) 8px 0;
  }
  .ledger-line {
    height: 52px;
    border-bottom: 1px solid var(--paper-rule);
    opacity: calc(1 - var(--i) * 0.16);
  }
  .ledger-line:first-child {
    border-top: 1px solid var(--paper-rule);
  }
  /* the register's first page, in the detail */
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    max-width: 420px;
    margin: clamp(24px, 9vh, 96px) auto 0;
    text-align: center;
  }
  .empty-seal {
    display: grid;
    margin-bottom: var(--space-2);
    color: var(--accent);
  }
  .empty-title {
    font-size: var(--display-m);
    color: var(--title-ink);
  }
  .empty-hint {
    font-size: var(--text-base);
    text-wrap: balance;
  }
  .empty-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
  /* no match, nothing picked: one quiet line by a pressed glyph */
  .none,
  .pick {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6) var(--space-3);
    text-align: center;
  }
  .pick {
    margin-top: clamp(24px, 12vh, 140px);
  }
  .none-glyph {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    color: var(--ink-3);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .none-text {
    font-size: var(--text-base);
    color: var(--ink-2);
  }
  .main {
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-4) var(--space-6) var(--space-6);
  }
  .status {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 34px;
    padding: 0 var(--space-5);
    border-top: 1px solid var(--paper-rule);
    font-size: var(--text-sm);
    color: var(--ink-3);
    overflow: hidden;
  }
  .copied {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ink);
    font-weight: 500;
  }
  .copied :global(svg) {
    color: var(--accent);
  }
  /* the clipboard drains as the wipe approaches (a transform: nothing is laid out again) */
  .drain {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 2px;
    background: var(--accent);
    transform: scaleX(var(--left));
    transform-origin: 0 50%;
    transition: transform 1s linear;
  }
  .keys {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-4);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  @media (max-width: 900px) {
    .body {
      grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
    }
    .label-text {
      display: none;
    }
    .main {
      padding: var(--space-3) var(--space-4) var(--space-5);
    }
  }
</style>
