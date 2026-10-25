<!-- The "Volume aperto" main area: toolbar, file view, context menu, delete confirm, quick look, drop zone. -->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import {
    CheckCheck,
    CircleCheck,
    Download,
    ImageDown,
    Eye,
    ExternalLink,
    FolderInput,
    FolderOpen,
    FolderPlus,
    Info,
    FolderSearch,
    Pencil,
    Tag as TagIcon,
    Trash,
    ArrowDownToLine,
    Upload,
  } from '@lucide/svelte';
  import { flushSync, onMount } from 'svelte';
  import { isMobile } from '../lib/platform';
  import { formatNumber } from '../lib/format';
  import MobileRename from './MobileRename.svelte';
  import MobileSelectBar from './MobileSelectBar.svelte';
  import MobileMove from './MobileMove.svelte';
  import GalleryAsk from './GalleryAsk.svelte';
  import * as api from '../lib/api';
  import { app } from '../lib/stores/app.svelte';
  import { DRAG_MIME } from '../lib/dnd';
  import { canPreview } from '../lib/filetype';
  import type { FileBrowser } from '../lib/stores/browser.svelte';
  import Dialog from './Dialog.svelte';
  import FileView from './FileView.svelte';
  import Menu, { type MenuItem } from './Menu.svelte';
  import PropertiesDialog from './PropertiesDialog.svelte';
  import { prefs } from '../lib/stores/prefs.svelte';
  import { viewSize } from '../lib/stores/viewsize.svelte';
  import QuickLook from './QuickLook.svelte';
  import Toolbar from './Toolbar.svelte';
  import TagManager from './TagManager.svelte';
  import TagPicker from './TagPicker.svelte';

  let { browser }: { browser: FileBrowser } = $props();
  let view: HTMLDivElement | undefined = $state();
  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
  const MOD = isMac ? '⌘' : 'Ctrl+';
  const PROPS_KEY = isMac ? '⌥↵' : 'Alt+↵';

  onMount(() => {
    let off: (() => void) | undefined;
    void api
      .onOsDragDrop((e) => {
        if (e.type === 'leave') {
          browser.dragOver = false;
          return;
        }
        // A modal (delete confirmation, quick look…) owns the window: ignore OS
        // drops like the inert page ignores HTML5 ones, instead of importing behind it.
        if (document.querySelector('dialog[open]')) {
          browser.dragOver = false;
          return;
        }
        if (e.type === 'enter' || e.type === 'over') browser.dragOver = true;
        else if (e.type === 'drop') {
          browser.dragOver = false;
          // dropped on a folder tile -> into that folder, like the HTML5 path
          void browser.importSources(e.paths, folderAt(e.position) ?? browser.cwd);
        }
      })
      .then((u) => (off = u));
    view?.focus();
    return () => off?.();
  });

  // --- folder history: mouse back/forward buttons, Alt+←/→ (Cmd+[ / ] on macOS) ----------
  /** -1 back, 1 forward. Quick look open: back closes it. Another dialog open: nothing. */
  function history(dir: -1 | 1): boolean {
    if (browser.quickLook !== null) {
      if (dir < 0) browser.quickLook = null;
      return true;
    }
    if (document.querySelector('dialog[open]') || browser.renaming !== null) return false;
    void (dir < 0 ? browser.goBack() : browser.goForward());
    return true;
  }
  function mouseHistory(e: MouseEvent) {
    // button 3 = back, 4 = forward (the side buttons); WebKitGTK reports them on mouseup
    if ((e.button !== 3 && e.button !== 4) || app.covered) return;
    e.preventDefault();
    if (e.type === 'mouseup') history(e.button === 3 ? -1 : 1);
  }
  function keyHistory(e: KeyboardEvent) {
    if (app.covered) return;
    const t = e.target as HTMLElement | null;
    // Ctrl/Cmd+F: the volume's search field (open, focused, what is typed selected)
    if ((isMac ? e.metaKey : e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'f' && !t?.closest('dialog[open]')) {
      e.preventDefault();
      browser.tagger = null;
      browser.tagManager = null;
      browser.openSearch();
      return;
    }
    if (t?.closest('input, textarea, [contenteditable="true"]')) return;
    // view size: Ctrl/Cmd + = (or +) bigger, Ctrl/Cmd + - smaller, Ctrl/Cmd + 0 back to the default
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (mod && !e.altKey && !t?.closest('dialog[open]')) {
      const z = e.key === '=' || e.key === '+' ? 1 : e.key === '-' || e.key === '_' ? -1 : e.key === '0' ? 0 : null;
      if (z !== null) {
        e.preventDefault();
        viewSize.zoom(browser.vault.id, prefs.view, z);
        return;
      }
    }
    const alt = e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey;
    const dir = alt && e.key === 'ArrowLeft' ? -1 : alt && e.key === 'ArrowRight' ? 1 : isMac && e.metaKey && e.key === '[' ? -1 : isMac && e.metaKey && e.key === ']' ? 1 : 0;
    if (dir && history(dir)) e.preventDefault();
  }

  /** Folder of this listing under a viewport point (a folder tile), if any. */
  function folderAt(p?: api.DragPoint): number | null {
    if (!p) return null;
    const tile = document.elementFromPoint(p.x, p.y)?.closest<HTMLElement>('[role="option"][data-kind="dir"]');
    const id = Number(tile?.id.match(/^item-(\d+)$/)?.[1]);
    return Number.isFinite(id) && browser.byId.get(id)?.kind === 'dir' ? id : null;
  }

  // --- HTML5 drop of OS files (browser / mock mode) ---------------------------
  const hasFiles = (e: DragEvent) => !!e.dataTransfer && [...e.dataTransfer.types].includes('Files') && ![...e.dataTransfer.types].includes(DRAG_MIME);

  function dragover(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    browser.dragOver = true;
  }
  function dragleave(e: DragEvent) {
    const to = e.relatedTarget as Node | null;
    if (!to || !(e.currentTarget as HTMLElement).contains(to)) browser.dragOver = false;
  }
  function drop(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    browser.dragOver = false;
    const files = [...(e.dataTransfer?.files ?? [])];
    void browser.importSources(files);
  }

  // --- context menu ---------------------------------------------------------
  const menuItems = $derived.by<MenuItem[]>(() => {
    const menu = browser.menu;
    if (!menu) return [];
    if (menu.target === null) {
      return [
        { id: 'new-folder', label: m.fb_new_folder(), icon: FolderPlus, shortcut: `${MOD}⇧N`, disabled: browser.resultsMode, action: () => browser.newFolder() },
        { id: 'import', label: m.fb_import_files(), icon: Upload, action: () => browser.importViaDialog() },
        { id: 'select-all', label: m.fb_select_all(), icon: CheckCheck, shortcut: `${MOD}A`, separatorBefore: true, disabled: browser.entries.length === 0, action: () => browser.selectAll() },
        { id: 'props', label: m.fb_props(), icon: Info, shortcut: PROPS_KEY, separatorBefore: true, action: () => browser.showProps([browser.cwd]) },
      ];
    }
    const sel = browser.selectedNodes;
    const one = sel.length === 1 ? sel[0] : null;
    const items: MenuItem[] = [];
    if (one?.kind === 'dir') {
      items.push({ id: 'open', label: m.fb_open(), icon: FolderOpen, shortcut: '↵', action: () => browser.activate(one.id) });
    } else if (one) {
      if (canPreview(one)) items.push({ id: 'preview', label: m.fb_preview(), icon: Eye, shortcut: m.key_space(), action: () => (browser.quickLook = one.id) });
      items.push({ id: 'external', label: m.fb_open_external(), icon: ExternalLink, action: () => browser.openExternal(one.id) });
    }
    if (one && browser.resultsMode && browser.hitMeta.has(one.id)) {
      items.push({ id: 'reveal', label: m.sr_show_in_folder(), icon: FolderSearch, action: () => browser.openHitPlace(one.id) });
    }
    // a phone: picking several starts here (a tap then picks instead of opening)
    if (isMobile && !browser.selecting) items.push({ id: 'select', label: m.menu_select(), icon: CircleCheck, action: () => browser.startSelecting(menu.target) });
    // (flushSync: the phone's rename sheet takes the focus in the tap's own task, or iOS keeps the keyboard down)
    if (one) items.push({ id: 'rename', label: m.fb_rename(), icon: Pencil, shortcut: 'F2', action: () => (isMobile ? flushSync(() => browser.startRename(one.id)) : browser.startRename(one.id)) });
    // a phone has no drag & drop between folders: a page to pick where (MobileMove)
    if (isMobile && !browser.resultsMode) items.push({ id: 'move', label: m.mv_action(), icon: FolderInput, action: () => (moving = [...browser.selectedIds]) });
    items.push({ id: 'tags', label: m.tag_action(), icon: TagIcon, action: () => browser.openTagger({ x: menu.x, y: menu.y }) });
    items.push({
      id: 'export',
      label: sel.length > 1 ? m.fb_export_n({ n: String(sel.length) }) : m.fb_export(),
      icon: Download,
      separatorBefore: true,
      action: () => browser.exportNodes(),
    });
    // a phone: pictures / videos straight into the system gallery
    if (isMobile && sel.some((n) => browser.isMedia(n.id))) items.push({ id: 'gallery', label: m.gallery_action(), icon: ImageDown, action: () => browser.saveToGallery() });
    items.push({ id: 'props', label: m.fb_props(), icon: Info, shortcut: PROPS_KEY, action: () => browser.showProps() });
    items.push({
      id: 'delete',
      label: sel.length > 1 ? m.fb_delete_n({ n: String(sel.length) }) : m.action_delete(),
      icon: Trash,
      shortcut: isMac ? '⌘⌫' : m.key_delete(),
      danger: true,
      separatorBefore: true,
      action: () => browser.requestDelete(),
    });
    return items;
  });

  // a phone's sheet says what it acts on
  const menuTitle = $derived.by(() => {
    if (!browser.menu) return undefined;
    if (browser.menu.target === null) return browser.trail[browser.trail.length - 1]?.name;
    const sel = browser.selectedNodes;
    return sel.length === 1 ? sel[0].name : m.sel_count({ count: sel.length, n: formatNumber(sel.length) });
  });
  let moving = $state<number[] | null>(null);
  const renamingNode = $derived(isMobile && browser.renaming !== null ? browser.byId.get(browser.renaming) : undefined);
  // the drop target starts under the folder's head, whatever its height (filter line, wrapped
  // actions): a fixed offset ran through the title's figures
  let sectionEl: HTMLElement | undefined = $state();
  const dropTop = $derived.by(() => {
    if (!browser.dragOver) return undefined;
    const head = sectionEl?.querySelector<HTMLElement>('.folder-head');
    return head ? `${head.offsetTop + head.offsetHeight}px` : undefined;
  });

  const deleteNames = $derived((browser.confirmDelete ?? []).map((id) => browser.byId.get(id)?.name ?? '').filter(Boolean));
</script>

<svelte:window onmouseup={mouseHistory} onmousedown={mouseHistory} onauxclick={mouseHistory} onkeydown={keyHistory} />

<section
  bind:this={sectionEl}
  class="file-browser sheet"
  class:drag-over={browser.dragOver}
  aria-label={m.fb_label()}
  ondragover={dragover}
  ondragleave={dragleave}
  ondrop={drop}
>
  <Toolbar {browser} />
  <FileView {browser} bind:view />

  {#if browser.dragOver}
    <div class="drop-overlay" aria-hidden="true" data-testid="drop-overlay" style:--drop-top={dropTop}>
      <span class="reg tl"></span><span class="reg tr"></span><span class="reg bl"></span><span class="reg br"></span>
      <div class="plaque nv-enter">
        <span class="bed"><ArrowDownToLine size={22} strokeWidth={1.6} /></span>
        <div class="plaque-text">
          <p class="plaque-title display">{m.fb_drop()} <em>{browser.trail[browser.trail.length - 1]?.name}</em></p>
          <p class="plaque-line caps">{m.fb_drop_line()}</p>
        </div>
      </div>
    </div>
  {/if}
</section>

{#if browser.menu}
  <Menu
    items={menuItems}
    x={browser.menu.x}
    y={browser.menu.y}
    label={browser.menu.target === null ? m.fb_menu_folder() : m.fb_menu_item()}
    title={menuTitle}
    onclose={(restore) => {
      browser.closeMenu();
      if (restore) view?.focus();
    }}
  />
{/if}

<Dialog
  open={browser.confirmDelete !== null}
  title={deleteNames.length === 1 ? m.fb_delete_one({ name: deleteNames[0] }) : m.fb_delete_many({ n: String(deleteNames.length) })}
  variant="danger"
  describedby="delete-desc"
  onclose={() => {
    browser.confirmDelete = null;
    view?.focus();
  }}
>
  <p id="delete-desc">{m.fb_delete_desc()}</p>
  {#if deleteNames.length > 1}
    <ul class="delete-list">
      {#each deleteNames.slice(0, 6) as n (n)}<li>{n}</li>{/each}
      {#if deleteNames.length > 6}<li class="hint">{m.fb_delete_more({ n: String(deleteNames.length - 6) })}</li>{/if}
    </ul>
  {/if}
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (browser.confirmDelete = null)}>{m.action_cancel()}</button>
    <!-- svelte-ignore a11y_autofocus -->
    <button type="button" class="btn danger" autofocus onclick={() => browser.confirmDeleteNow().then(() => view?.focus())}>{m.action_delete()}</button>
  {/snippet}
</Dialog>

{#if renamingNode}
  <MobileRename name={renamingNode.name} oncommit={(v) => browser.commitRename(renamingNode.id, v)} oncancel={() => browser.cancelRename()} />
{/if}
{#if isMobile && browser.selecting}
  <MobileSelectBar {browser} />
{/if}
{#if moving}
  {@const ids = moving}
  <MobileMove
    vault={browser.vault.id}
    volumeName={browser.vault.name}
    {ids}
    label={browser.labelFor(ids.map((id) => browser.byId.get(id)).filter((n) => !!n))}
    start={browser.trail}
    from={browser.cwd}
    onmove={async (dest) => {
      await browser.moveInto(ids, dest);
      moving = null;
      if (browser.selecting) browser.endSelecting();
    }}
    oncancel={() => (moving = null)}
  />
{/if}
{#if isMobile}
  <GalleryAsk />
{/if}

<QuickLook {browser} />

<PropertiesDialog {browser} onclosed={() => view?.focus()} />

{#if browser.tagger}
  <TagPicker {browser} onclosed={(restore) => restore && view?.focus()} />
{/if}
{#if browser.tagManager}
  <TagManager {browser} onclosed={(restore) => restore && view?.focus()} />
{/if}

<style>
  .file-browser {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  /* files arriving: the page is lit round its edge in the volume's ink, four registration marks
     (as on a print) at the corners, and a plaque at the foot says where they will land */
  .drop-overlay {
    position: absolute;
    inset: var(--drop-top, var(--page-head-h)) 14px 14px 18px;
    z-index: 5;
    display: grid;
    /* the plaque waits at the foot of the page, so every folder stays a target */
    place-items: end center;
    padding-bottom: 18px;
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--accent) 5%, transparent);
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--accent) 38%, transparent),
      inset 0 0 28px color-mix(in srgb, var(--accent) 16%, transparent);
    pointer-events: none;
  }
  .reg {
    position: absolute;
    width: 18px;
    height: 18px;
    border: 0 solid color-mix(in srgb, var(--accent) 70%, var(--foil));
    opacity: 0.85;
  }
  .reg.tl { top: 8px; left: 8px; border-width: 1.5px 0 0 1.5px; border-top-left-radius: 5px; }
  .reg.tr { top: 8px; right: 8px; border-width: 1.5px 1.5px 0 0; border-top-right-radius: 5px; }
  .reg.bl { bottom: 8px; left: 8px; border-width: 0 0 1.5px 1.5px; border-bottom-left-radius: 5px; }
  .reg.br { bottom: 8px; right: 8px; border-width: 0 1.5px 1.5px 0; border-bottom-right-radius: 5px; }
  .plaque {
    position: relative;
    display: flex;
    align-items: center;
    gap: 14px;
    max-width: min(460px, 92%);
    padding: 10px 22px 10px 12px;
    border-radius: 999px;
    background: linear-gradient(180deg, var(--paper-hi), var(--paper));
    box-shadow:
      0 0 0 1px var(--paper-rule),
      inset 0 1px 0 rgb(255 255 255 / 0.35),
      var(--shadow-pop);
    animation: nv-enter var(--dur-ui) var(--ease-out) backwards;
    --enter-from: translateY(8px) scale(0.985);
    --enter-to: translateY(0) scale(1);
  }
  /* the arrow sits in a bed pressed into the plaque, in the ink's foil toned toward the paper */
  .bed {
    display: grid;
    place-items: center;
    flex: none;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    color: var(--accent);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 3px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .plaque-text {
    min-width: 0;
    text-align: left;
  }
  .plaque-title {
    font-size: 19px;
    line-height: 1.15;
    color: var(--title-ink);
    overflow-wrap: anywhere;
  }
  .plaque-title em {
    font-style: italic;
    color: var(--accent);
  }
  .plaque-line {
    margin-top: 3px;
  }

  /* what is about to go: a short ruled list */
  .delete-list {
    list-style: none;
    margin: 0;
    padding: 2px 0;
    border-radius: var(--radius-sm);
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
    font-size: var(--text-sm);
    color: var(--ink);
  }
  .delete-list li {
    padding: 6px 12px;
    overflow-wrap: anywhere;
  }
  .delete-list li + li {
    border-top: 1px solid var(--paper-rule);
  }
</style>
