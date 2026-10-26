// The ONLY boundary between the UI and the backend.
//
// Inside Tauri every function maps 1:1 to a command of the command contract.
// Outside Tauri (plain browser, `pnpm dev`, vitest) the same commands go to the
// in-memory MockBackend (mock.ts), so every screen works without Rust.
//
// Wire conventions the Rust side must follow:
//   - argument keys are snake_case exactly as below -> use
//     `#[tauri::command(rename_all = "snake_case")]` on every command;
//   - errors are `{ code, message }` (ApiError);
//   - events: "job-progress", "job-done", "vault-locked" (payloads in types.ts).
//
// Additions to the base command contract the UI needs (all implemented in src-tauri):
//   - vault_lock      { vault, reason? }            reason is echoed in vault-locked (UI idle timer sends "idle")
//   - settings_get { vault? } / settings_set { settings, vault? } -> Settings  { auto_lock_minutes, accent, theme }
//     Settings are PER VOLUME (stored inside it). `vault` given: that volume (error "locked" if not open).
//     Omitted: the most recently opened volume still open. None open: session defaults (not persisted;
//     new volumes start from them). settingsGet()/settingsSet() below fill `vault` with the UI's active
//     volume (bindActiveVault), so with several open volumes they act on the one on screen.
//   - default_volume_dir                           -> string     (folder of the executable / stick)
//   - job_cancel      { job }                      -> ()         (emits job-done with error.code "cancelled")
//   - mount_available returns ALL backends with { available, reason } so the UI can explain.
//   - slot_add_password { vault, password, label? } / slot_change_password { vault, slot, old_password, new_password }
//     / slot_remove { vault, slot } / slot_rotate_key { vault, slot, password } all return the updated SlotInfo[].
//   - vault_emblem_get { vault } / vault_emblem_set { vault, emblem, show_when_locked, locked_image_base64? } -> EmblemState
//     show_when_locked IS the appearance visibility (true = "visible", false = "encrypted").
//   - vault_appearance_get { vault } / vault_appearance_set { vault, accent?, emblem?, visibility?, locked_image_base64? }
//     -> AppearanceState. visible: accent + a small emblem copy go to the volume header (plaintext, readable
//     without the password; MAC-checked at unlock, VaultInfo.appearance_tampered). encrypted: header cleared,
//     locked screens show neutral defaults. An image emblem needs its <= 24 KB, 128 px copy for the header
//     (emblem.ts lockedImageOf; vaults.setEmblem sends it), else the locked screens show the initial.
//   - recent_list: a scan (app folder + 1 level, removable drive roots) + installed-mode known list; nothing
//     is stored on the stick. RecentEntry.accent / emblem / show_when_locked come from the header.
//   - Settings.accent: accent preset id (accents.ts); Settings.theme: system | light | dark.
//   - update_check { auto? } -> UpdateStatus (auto: the start-up check, skipped when disabled / done today)
//     update_download -> () emits "update-progress" { downloaded, total }; verifies the signature
//     update_install -> InstallOutcome · update_restart -> () (starts the new build, quits this one)
//     update_prefs_get -> UpdatePrefs · update_prefs_set { check_on_start } -> UpdatePrefs
//   - import_size { paths } -> number (bytes under the host paths; the space check before an import, lib/space.ts)
//   - vault_resize_info { vault } -> ResizeInfo · vault_resize { vault, size } -> ResizeInfo (after), emits
//     "vault-resize" { vault, phase, done, total }; errors not_fixed | out_of_range | too_small | no_space

import { m } from '../paraglide/messages.js';
import { MockBackend, STICK } from './mock';
import { switches } from './switches';
import { isIos, isMobile } from './platform';
import type {
  PhotoImport,
  ApiError,
  AppearanceState,
  Backend,
  Emblem,
  EmblemState,
  EventMap,
  EventName,
  FsNode,
  InstallOutcome,
  JobId,
  Kdf,
  LockReason,
  NodeId,
  RecentEntry,
  ResizeInfo,
  ResizeProgress,
  SearchFilters,
  SearchResult,
  SearchSort,
  Settings,
  SlotInfo,
  Tag,
  TagColor,
  TagId,
  ThemeId,
  UpdatePrefs,
  UpdateStatus,
  VaultId,
  VaultInfo,
  VaultStats,
  Visibility,
} from './types';

export const isTauri: boolean = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// ---------------------------------------------------------------------------
// transport

let mockInstance: MockBackend | null = null;

/** The active mock (browser/tests only). */
export function mock(): MockBackend {
  if (!mockInstance) {
    mockInstance = new MockBackend({
      empty: switches.empty,
      mountUnavailable: switches.mount === 'unavailable',
      mounted: switches.mount === 'mounted',
      mountOs: switches.mount?.startsWith('win_') || switches.mount?.startsWith('mac_') ? switches.mount : null,
      importFails: switches.error === 'import',
      deleteFails: switches.error === 'delete',
      slowDelete: switches.slow_delete,
      singleSlot: switches.slots1,
      emblem: switches.emblem,
      appearance: switches.appearance,
      frame: switches.frame,
      updateOnStart: switches.update !== null,
      updateMode: switches.update === 'downloads' ? 'downloaded' : 'replaced',
      pmEntries: switches.pm ?? 0,
      device: switches.device,
      deviceFail: switches.device_fail,
      deviceOffline: switches.device_offline,
      deviceSlow: switches.device_slow,
      deviceVolumes: switches.device_volumes,
      stick: switches.stick,
      stickOffline: switches.stick_offline,
      stickFail: switches.stick_fail,
      fixed: switches.fixed,
      resizeFail: switches.resize_fail,
      fmtFull: switches.fmt_full,
    });
    if (typeof window !== 'undefined') {
      // devtools handle: __nv.mock.simulateRemoval(), __nv.mock.setMountAvailable(false), ...
      (window as unknown as { __nv: unknown }).__nv = { mock: mockInstance };
    }
  }
  return mockInstance;
}

/** Tests: swap in a configured mock (e.g. speed 0). */
export function useMock(m: MockBackend): void {
  mockInstance = m;
}

export function isApiError(e: unknown): e is ApiError {
  return typeof e === 'object' && e !== null && 'code' in e && 'message' in e;
}

function normalizeError(e: unknown): ApiError {
  if (isApiError(e)) return { code: String(e.code), message: String(e.message) };
  if (typeof e === 'string') return { code: 'error', message: e };
  if (e instanceof Error) return { code: 'error', message: e.message };
  return { code: 'error', message: m.error_unknown() };
}

/** Exported for the feature modules (passwords.ts); screens use the typed wrappers. */
export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    if (isTauri) {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke<T>(cmd, args);
    }
    return (await mock().invoke(cmd, args)) as T;
  } catch (e) {
    throw normalizeError(e);
  }
}

export type Unlisten = () => void;

/** Subscribe to a backend event. Returns an unlisten function. */
export async function on<E extends EventName>(event: E, handler: (payload: EventMap[E]) => void): Promise<Unlisten> {
  if (isTauri) {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<EventMap[E]>(event, (e) => handler(e.payload));
  }
  return mock().listen(event, handler);
}

export const onJobProgress = (h: (p: EventMap['job-progress']) => void) => on('job-progress', h);
export const onJobDone = (h: (p: EventMap['job-done']) => void) => on('job-done', h);
export const onVaultLocked = (h: (p: EventMap['vault-locked']) => void) => on('vault-locked', h);
export const onUpdateProgress = (h: (p: EventMap['update-progress']) => void) => on('update-progress', h);
export const onPmIcons = (h: (p: EventMap['pm-icons']) => void) => on('pm-icons', h);

// ---------------------------------------------------------------------------
// volumes

/** `fixedSize`: a preallocated volume of that many bytes (Crea dispositivo); omitted = grows. */
export const vaultCreate = (path: string, password: string, kdf: Kdf = 'strong', fixedSize?: number | null) =>
  call<VaultInfo>('vault_create', fixedSize ? { path, password, kdf, fixed_size: fixedSize } : { path, password, kdf });
export const vaultOpen = (path: string, password: string) => call<VaultInfo>('vault_open', { path, password });
export const vaultLock = (vault: VaultId, reason: LockReason = 'user') => call<void>('vault_lock', { vault, reason });
export const vaultListOpen = () => call<VaultInfo[]>('vault_list_open');
export const vaultStats = (vault: VaultId) => call<VaultStats>('vault_stats', { vault });
/** "Aggiorna formato": makes the volume single-file in place (crash-safe; older builds refuse it
 *  afterwards). Returns the updated VaultInfo. `unsupported`: a fixed-size volume. */
export const vaultUpgradeFormat = (vault: VaultId) => call<VaultInfo>('vault_upgrade_format', { vault });
/** "Tieni questa versione": the volume opened older than last seen here is the one to keep (VaultInfo.rolled_back). */
export const vaultAcceptRollback = (vault: VaultId) => call<VaultInfo>('vault_accept_rollback', { vault });

/** Estendi / Riduci: size facts (capacity null = a growable volume). */
export const vaultResizeInfo = (vault: VaultId) => call<ResizeInfo>('vault_resize_info', { vault });
/** New capacity for a fixed-size volume (bytes, rounded down to 4 KiB); resolves when done. */
export const vaultResize = (vault: VaultId, size: number) => call<ResizeInfo>('vault_resize', { vault, size });
export const onVaultResize = (h: (p: ResizeProgress) => void) => on('vault-resize', h);

export const vaultEmblemGet = (vault: VaultId) => call<EmblemState>('vault_emblem_get', { vault });
/** `locked_image_base64`: the header copy of an image emblem (emblem.ts lockedImageOf). */
export const vaultEmblemSet = (vault: VaultId, emblem: Emblem, show_when_locked = true, locked_image_base64?: string | null, emblem_frame?: string | null) =>
  call<EmblemState>('vault_emblem_set', { vault, emblem, show_when_locked, locked_image_base64: locked_image_base64 ?? null, emblem_frame: emblem_frame ?? null });

export const vaultAppearanceGet = (vault: VaultId) => call<AppearanceState>('vault_appearance_get', { vault });
export interface AppearancePatch {
  accent?: string;
  emblem?: Emblem;
  /** the frame round the emblem (ornament/frames.ts) */
  emblem_frame?: string;
  visibility?: Visibility;
  locked_image_base64?: string | null;
}
export const vaultAppearanceSet = (vault: VaultId, patch: AppearancePatch) =>
  notifySettings(vault, call<AppearanceState>('vault_appearance_set', { vault, ...patch }), (a) => ({ accent: a.accent }));

export const recentList = () => call<RecentEntry[]>('recent_list');
export const recentForget = (path: string) => call<RecentEntry[]>('recent_forget', { path });
export const defaultVolumeDir = () => call<string>('default_volume_dir');

// --- per-volume settings ------------------------------------------------------

let activeVault: () => VaultId | null = () => null;

/** The stores tell api.ts which volume is on screen: settingsGet/Set default to it. */
export function bindActiveVault(get: () => VaultId | null): void {
  activeVault = get;
}

type SettingsListener = (vault: VaultId | null, s: Partial<Settings>) => void;
const settingsListeners = new Set<SettingsListener>();

/** Called after every successful settings_set / vault_appearance_set (vault null = session defaults). */
export function onSettingsChanged(l: SettingsListener): Unlisten {
  settingsListeners.add(l);
  return () => settingsListeners.delete(l);
}

async function notifySettings<T>(vault: VaultId | null, p: Promise<T>, pick: (r: T) => Partial<Settings>): Promise<T> {
  const r = await p;
  for (const l of settingsListeners) l(vault, pick(r));
  return r;
}

/** Settings of `vault` (default: the active volume; none open: session defaults). */
export const settingsGet = (vault: VaultId | null = activeVault()) => call<Settings>('settings_get', { vault });
export function settingsSet(settings: Partial<Settings>, vault: VaultId | null = activeVault()): Promise<Settings> {
  return notifySettings(vault, call<Settings>('settings_set', { settings, vault }), (s) => s);
}

// ---------------------------------------------------------------------------
// files

/** A folder's items (each with its `tags`); `filters` narrows them (search.rs; `within` ignored). */
export const fsList = (vault: VaultId, dir: NodeId, filters?: SearchFilters | null) =>
  call<FsNode[]>('fs_list', { vault, dir, filters: filters ?? null });
export const fsMkdir = (vault: VaultId, parent: NodeId, name: string) => call<FsNode>('fs_mkdir', { vault, parent, name });
export const fsRename = (vault: VaultId, node: NodeId, new_parent: NodeId, new_name: string) =>
  call<void>('fs_rename', { vault, node, new_parent, new_name });
/** A job (job-progress with done_items / total_items, fs-changed per committed batch, job-done.outcome). */
export const fsDelete = (vault: VaultId, nodes: NodeId[]) => call<JobId>('fs_delete', { vault, nodes });
export const fsExport = (vault: VaultId, nodes: NodeId[], dest_dir: string) =>
  call<JobId>('fs_export', { vault, nodes, dest_dir });
/** host_disk_notice: say once that the copy sits on this computer's disk (portable, not a RAM folder).
 *  mobile: "Apri con" hands a decrypted copy to another app through the share sheet. */
export const fsOpenExternal = (vault: VaultId, node: NodeId): Promise<{ host_disk_notice: boolean } | null> =>
  isMobile && isTauri
    ? mobileShare(vault, [node]).then(() => null)
    : call<{ host_disk_notice: boolean } | null>('fs_open_external', { vault, node });

// ---- Android / iOS only (src-tauri/src/mobile.rs) ----
/** A picked volume URI copied into the app's Volumes folder; returns the path to open. */
export const mobileVolumeIn = (uri: string) => call<string>('mobile_volume_in', { uri });
/** The share sheet for files of a volume (decrypted copies in the app cache). */
export const mobileShare = (vault: VaultId, nodes: NodeId[]) => call<void>('mobile_share', { vault, nodes, title: m.mobile_share_title() });
/** Where exports go on a phone (no folder picker). */
export const mobileExportDir = () => call<string>('mobile_export_dir');
/** The camera roll (Android Photo Picker, iOS PHPicker; photos and videos, several at once)
 *  into `parent`: every item is fetched (iCloud / Google Photos downloads awaited), imported as
 *  it arrives (mobile-pick-progress), staged copies wiped one by one. `retry`: indices of a
 *  previous pick's failures ("Riprova questi"), no picker. null when nothing was picked. */
export const mobileImportPhotos = (vault: VaultId, parent: NodeId, retry?: number[]) =>
  call<PhotoImport | null>('mobile_import_photos', { vault, parent, retry: retry ?? null });
export const onPickProgress = (h: (p: EventMap['mobile-pick-progress']) => void) => on('mobile-pick-progress', h);
/** "Salva nella Galleria": pictures / videos into the phone's gallery (iOS Photos add-only,
 *  Android MediaStore Pictures|Movies/NiceVault). `others`: not gallery media here, for Files.
 *  Errors: `gallery_denied` (iOS access refused), `unsupported` (Android < 10). */
export const mobileSaveGallery = (vault: VaultId, nodes: NodeId[]) =>
  call<{ saved: NodeId[]; others: NodeId[] }>('mobile_save_gallery', { vault, nodes });
/** Android: another app uses a volume through the Files provider right now (the background lock waits). */
export const mobileDocsBusy = () => call<boolean>('mobile_docs_busy');
/** iOS: the document picker, files copied into the app under their real names (Android: SAF via the dialog plugin). */
export const mobilePickFiles = (multiple: boolean) => call<string[]>('mobile_pick_files', { multiple });
// in-app viewers (app/src-tauri/src/viewers.rs)
/** a loopback stream URL for audio/video (WebKitGTK only), else null: play previewUrl() */
export const mediaUrl = (vault: VaultId, node: NodeId) => call<string | null>('media_url', { vault, node });
export const mediaRelease = (url: string) => call<void>('media_release', { url });
/** open with the system app through the mounted drive (no decrypted copy); not_mounted otherwise */
export const fsOpenMounted = (vault: VaultId, node: NodeId) => call<void>('fs_open_mounted', { vault, node });
/** an http(s)/mailto link from a document, in the system browser / mail app */
export const openLink = (url: string) => call<void>('open_link', { url });
export const jobCancel = (job: JobId) => call<void>('job_cancel', { job });

/** A host path (Tauri, dialogs, OS drag & drop) or a browser File (mock mode only). */
export type ImportSource = string | File;

/** Bytes the host paths hold (files and folders; 0 for what can't be read): the free-space check before an import. */
export const importSize = (paths: string[]) => call<number>('import_size', { paths });

export function fsImport(vault: VaultId, parent: NodeId, sources: ImportSource[]): Promise<JobId> {
  const host_paths = isTauri ? sources.filter((s): s is string => typeof s === 'string') : sources;
  return call<JobId>('fs_import', { vault, parent, host_paths });
}

// ---------------------------------------------------------------------------
// mount

export const mountAvailable = () => call<Backend[]>('mount_available');
export const mount = (vault: VaultId) => call<{ mountpoint: string }>('mount', { vault });
export const unmount = (vault: VaultId) => call<void>('unmount', { vault });

// ---------------------------------------------------------------------------
// keyslots

export const slotsList = (vault: VaultId) => call<SlotInfo[]>('slots_list', { vault });
export const slotAddPassword = (vault: VaultId, password: string, label?: string) =>
  call<SlotInfo[]>('slot_add_password', { vault, password, label: label ?? null });
export const slotChangePassword = (vault: VaultId, slot: number, old_password: string, new_password: string) =>
  call<SlotInfo[]>('slot_change_password', { vault, slot, old_password, new_password });
export const slotRemove = (vault: VaultId, slot: number) => call<SlotInfo[]>('slot_remove', { vault, slot });
/**
 * Re-encrypts the whole volume under a new master key (nv-core rotate_master): every old copy of
 * the header stops opening what is written from now on. Keeps only password slot `slot` (whose
 * `password` must open it); every other key is dropped. Needs free space as large as the volume
 * (the new copy is written beside it, then swapped in). One call, no progress events.
 */
export const slotRotateKey = (vault: VaultId, slot: number, password: string) => call<SlotInfo[]>('slot_rotate_key', { vault, slot, password });

// ---------------------------------------------------------------------------
// previews

const isWindows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
/** WebView2 (Windows) and the Android WebView reach custom schemes as http://<scheme>.localhost */
const httpSchemes = isWindows || (typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent));

/**
 * URL that streams the decrypted bytes of a node (Range supported) for
 * <img>/<video>/<audio>/fetch. Tauri custom schemes are `nvault://<host>/...`
 * on macOS/Linux/iOS and `http://nvault.<host>/...` on Windows (WebView2) and Android.
 */
export function previewUrl(vault: VaultId, node: NodeId, thumb?: number, fit?: number): string {
  if (!isTauri) return mock().assetUrl(vault, node) ?? '';
  const v = encodeURIComponent(vault);
  // `?thumb=<px>`: a picture scaled so its longer side is <= px device pixels (thumbs.rs);
  // `?fit=<px>`: the same, finely resampled, for a picture shown whole (quick look)
  const q = thumb ? `?thumb=${Math.round(thumb)}` : fit ? `?fit=${Math.round(fit)}` : '';
  return httpSchemes ? `http://nvault.localhost/${v}/${node}${q}` : `nvault://${v}/${node}${q}`;
}

// ---------------------------------------------------------------------------
// dialogs (native in Tauri, canned answers in mock mode)

const nvaultFilter = () => [{ name: m.dialog_volume_filter(), extensions: ['nvault'] }];

export async function pickVolumeFile(): Promise<string | null> {
  if (!isTauri) return `${STICK}/Archivio fiscale.nvault`;
  if (isIos) {
    const [p] = await mobilePickFiles(false);
    return p ? mobileVolumeIn(p) : null;
  }
  const { open } = await import('@tauri-apps/plugin-dialog');
  // mobile: the system picker can't filter by an unknown extension, and returns a URI: the
  // volume is copied into the app first (mobile_volume_in)
  const r = await open({ multiple: false, directory: false, filters: isMobile ? undefined : nvaultFilter(), title: m.dialog_open_volume() });
  if (typeof r !== 'string') return null;
  return isMobile ? mobileVolumeIn(r) : r;
}

export async function pickFolder(title: string, defaultPath?: string): Promise<string | null> {
  if (!isTauri) return defaultPath ?? '/home/utente/Scrivania';
  // mobile: no folder picker; exports go to Download/NiceVault (Android) / Documents (iOS)
  if (isMobile) return mobileExportDir();
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ directory: true, multiple: false, title, defaultPath });
  return typeof r === 'string' ? r : null;
}

export async function pickImportFiles(): Promise<string[]> {
  if (!isTauri) {
    // ?import_pick=a.bin,b.bin: what the mock file dialog returns (e2e: huge* does not fit, slow* is slow)
    const pick = new URLSearchParams(location.search).get('import_pick');
    return pick ? pick.split(',').map((n) => `/home/utente/Scaricati/${n}`) : ['/home/utente/Scaricati/Bolletta luce.pdf', '/home/utente/Immagini/Spiaggia.jpg'];
  }
  if (isIos) return mobilePickFiles(true);
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: true, directory: false, title: m.dialog_import_into() });
  return Array.isArray(r) ? r : typeof r === 'string' ? [r] : [];
}

/** Pointer position in CSS pixels of the webview (when the platform reports one). */
export interface DragPoint {
  x: number;
  y: number;
}

export type OsDragEvent =
  | { type: 'enter' | 'over'; paths?: string[]; position?: DragPoint }
  | { type: 'drop'; paths: string[]; position?: DragPoint }
  | { type: 'leave' };

/**
 * OS drag & drop of host files onto the window. Tauri only; in a plain browser
 * the HTML5 drop path is used, and the mock can replay Tauri-style events
 * (`__nv.mock.simulateOsDrag(...)`) so this code path is testable too.
 */
export async function onOsDragDrop(handler: (e: OsDragEvent) => void): Promise<Unlisten> {
  if (!isTauri) return mock().onOsDrag(handler);
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  // Tauri labels the position "physical", but wry reports GTK (Linux) and AppKit
  // (macOS) coordinates in logical points; only WebView2 (Windows) is in device px.
  const scale = isWindows ? window.devicePixelRatio || 1 : 1;
  const point = (p?: { x: number; y: number }): DragPoint | undefined => (p ? { x: p.x / scale, y: p.y / scale } : undefined);
  return getCurrentWebview().onDragDropEvent((e) => {
    const p = e.payload as { type: string; paths?: string[]; position?: { x: number; y: number } };
    if (p.type === 'drop') handler({ type: 'drop', paths: p.paths ?? [], position: point(p.position) });
    else if (p.type === 'leave') handler({ type: 'leave' });
    else handler({ type: p.type === 'enter' ? 'enter' : 'over', paths: p.paths, position: point(p.position) });
  });
}

// ---------------------------------------------------------------------------
// password manager: pm_* commands (types + calls in passwords.ts, mock in pmMock.ts)

export * from './passwords';

// ---------------------------------------------------------------------------
// auto-lock + duress
//
//   - vault_touch { vault } -> ()       user activity on that volume (idle store, throttled). Every
//     command that touches a volume also counts; so do running jobs and the mounted drive's reads/writes.
//   - autolock_status -> AutoLockStatus[]   each open volume's idle countdown (not activity itself).
//     The backend owns the timers (one per volume, Settings.auto_lock_minutes, 0 = never) and the
//     sleep / screen-lock triggers (Settings.lock_on_sleep / lock_on_screen_lock, lock_scope);
//     it emits vault-locked with reason idle | sleep | screen | removed | user.
//   - slot_add_duress { vault, password, label? } -> SlotInfo[]   kind "duress"; errors
//     "duress_conflict" (same as another password), "slots_full", "invalid_password". Remove it
//     with slot_remove (vault unlocked). Entering it on Sblocca destroys the volume: vault_open
//     answers "wrong_password", forever, for every password.

export const vaultTouch = (vault: VaultId) => call<void>('vault_touch', { vault });
export const autolockStatus = () => call<import('./types').AutoLockStatus[]>('autolock_status');
export const slotAddDuress = (vault: VaultId, password: string, label?: string) =>
  call<SlotInfo[]>('slot_add_duress', { vault, password, label: label ?? null });

// ---------------------------------------------------------------------------
// global settings (Impostazioni) — not a volume's: the UI language and the defaults for NEW
// volumes. Installed mode keeps them in settings.json in the OS config dir; portable mode in
// memory for this session only (nothing on the stick). See app/src-tauri/src/global.rs.
//   - app_settings_get                          -> AppSettings { language, defaults, persisted, app_theme, app_accent, ui_scale, new_like_app, site_icons, start_at_login, start_minimized, close_to_tray }
//   - app_settings_set { patch: AppSettingsPatch } -> AppSettings  (ui_scale: the backend sets the webview's zoom)
//   - app_info                                  -> AppInfo { version, portable, config_dir }
//   start_at_login / start_minimized: installed mode only (portable: ignored, always false).
//   start_at_login writes the OS's start-at-login entry (error "autostart_failed": nothing changed);
//   start_minimized: started at login it stays hidden behind a tray icon.
//   close_to_tray: closing the window hides it in the tray (default on when installed; quits
//   anyway when the desktop shows no tray icons). Portable: always false.

export interface AppSettings {
  /** "system" | "it" | "en" */
  language: string;
  /** what a volume created from now on starts with */
  defaults: Settings;
  /** true = kept in the OS config dir (installed); false = this session only (portable) */
  persisted: boolean;
  /** password manager: fetch each entry's site icon directly from its website (default on) */
  site_icons: boolean;
  /** subtle UI sounds, synthesized in the webview (default on) */
  sounds: boolean;
  /** ←/→ in a full-screen video: seek inside it (default) or step to the previous / next file */
  video_arrows: VideoArrows;
  /** the volume's sidebar folded to a slim rail (desktop; default off) */
  sidebar_collapsed: boolean;
  /** the app's own look, worn wherever no volume's look applies (lib/worn.ts) */
  app_theme: ThemeId;
  app_accent: string;
  /** interface size, percent: one of UI_SCALES */
  ui_scale: number;
  /** new volumes start with the app's theme and ink ("Come l'app") */
  new_like_app: boolean;
  /** "Avvia all'accesso" (installed only) */
  start_at_login: boolean;
  /** "Avvia ridotto": hidden at login, tray icon (installed only) */
  start_minimized: boolean;
  /** "Resta nel vassoio quando chiudi la finestra" (installed only, default on) */
  close_to_tray: boolean;
  /** phones: "Salva foto e video nella Galleria" for Esporta; null = not asked yet (asked once) */
  gallery_save: boolean | null;
}

export type VideoArrows = 'seek' | 'step';

export interface AppSettingsPatch {
  language?: string;
  defaults?: Partial<Settings>;
  app_theme?: ThemeId;
  app_accent?: string;
  ui_scale?: number;
  new_like_app?: boolean;
  site_icons?: boolean;
  sounds?: boolean;
  video_arrows?: VideoArrows;
  sidebar_collapsed?: boolean;
  start_at_login?: boolean;
  start_minimized?: boolean;
  close_to_tray?: boolean;
  gallery_save?: boolean;
}

/** Interface sizes, percent (mirrors store::UI_SCALES). */
export const UI_SCALES = [90, 100, 110, 125, 150] as const;

export interface AppInfo {
  version: string;
  portable: boolean;
  config_dir: string | null;
}

export const appSettingsGet = () => call<AppSettings>('app_settings_get');
export const appSettingsSet = (patch: AppSettingsPatch) => call<AppSettings>('app_settings_set', { patch });
export const appInfo = () => call<AppInfo>('app_info');

/** The `.nvault` file the app was launched with / a second launch brought (once), else null. */
export const launchPathTake = () => call<string | null>('launch_path_take');
export const onOpenPath = (h: () => void) => on('open-path', () => h());

/** A file to read (password-manager import, key file). Mock: a canned path. */
export async function pickFile(title: string, extensions?: string[]): Promise<string | null> {
  if (!isTauri) return `/home/utente/Scaricati/export.${extensions?.[0] ?? 'txt'}`;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: false, directory: false, title, filters: extensions ? [{ name: title, extensions }] : undefined });
  return typeof r === 'string' ? r : null;
}

// ---------------------------------------------------------------------------
// live refresh (fs_events.rs)

/** Folders changed outside the app (the mounted drive): re-list the visible ones. */
export const onFsChanged = (h: (p: EventMap['fs-changed']) => void) => on('fs-changed', h);

// ---------------------------------------------------------------------------
// updater

export const updateCheck = (auto = false) => call<UpdateStatus>('update_check', { auto });
export const updateDownload = () => call<void>('update_download');
export const updateInstall = () => call<InstallOutcome>('update_install');
export const updateRestart = () => call<void>('update_restart');
export const updatePrefsGet = () => call<UpdatePrefs>('update_prefs_get');
export const updatePrefsSet = (check_on_start: boolean) => call<UpdatePrefs>('update_prefs_set', { check_on_start });

// ---------------------------------------------------------------------------
// drives (drives.rs): the mounted drives, live
//   - drives_list -> Drive[]   fresh enumeration (free space up to date); the app's drive first
//   - event drives-changed { drives }   a drive appeared / went / was renamed (Benvenuto re-scans)
// Mock: __nv.mock.simulateDrive('add' | 'remove', drive?)

export const drivesList = () => call<import('./types').Drive[]>('drives_list');
export const onDrivesChanged = (h: (p: EventMap['drives-changed']) => void) => on('drives-changed', h);

// ---------------------------------------------------------------------------
// properties (props.rs): Proprietà
//   - fs_props { vault, node } -> NodeProps          type sniffed, image size / media duration from headers,
//                                                    created / imported / origin (null = unknown: show "—")
//   - fs_props_tree { vault, nodes } -> JobId        recursive files / dirs / bytes: props-progress, props-done
//   - fs_hash { vault, node } -> JobId               SHA-256: props-progress { done_bytes, total_bytes },
//                                                    props-done { sha256 } (cached in the volume)
// Cancel with job_cancel (the dialog does when it closes).

export const fsProps = (vault: VaultId, node: NodeId) => call<import('./types').NodeProps>('fs_props', { vault, node });
export const fsPropsTree = (vault: VaultId, nodes: NodeId[]) => call<JobId>('fs_props_tree', { vault, nodes });
export const fsHash = (vault: VaultId, node: NodeId) => call<JobId>('fs_hash', { vault, node });
export const onPropsProgress = (h: (p: EventMap['props-progress']) => void) => on('props-progress', h);
export const onPropsDone = (h: (p: EventMap['props-done']) => void) => on('props-done', h);

// ---------------------------------------------------------------------------
// search, filters, tags (search.rs)
//   - fs_search { vault, query, filters?, sort?, desc?, limit? = 100 (<= 1000), cursor? } -> SearchResult
//       query: words (each must occur in the name, anywhere; case and accents ignored), "a phrase";
//       "" = filters only. Errors: invalid_query (sort / cursor / text over 1 KiB), locked.
//       The first search of a session builds the in-memory index (~0.1-0.2 s at 100k nodes): call it
//       once when the search field opens. Old volumes fill their stored index in the background:
//       event search-index { vault, done, total } (results are complete meanwhile).
//   - fs_list { vault, dir, filters? } -> FsNode[] with `tags` (see fsList above)
//   - tags_list { vault } -> Tag[] (by name, with counts)
//   - tags_create { vault, name, color } -> Tag        invalid_name / invalid_color / exists / too_many_tags
//   - tags_update { vault, tag, name?, color? } -> Tag  same errors + not_found
//   - tags_delete { vault, tag } -> ()                  (its assignments go with it)
//   - tags_assign / tags_unassign { vault, tags, nodes } -> Tag[] (new counts; missing nodes skipped)
//   - tags_of { vault, nodes } -> { [node]: TagId[] }   (only nodes that have any)
//   - event tags-changed { vault, nodes, dirs } after every change

export interface SearchOptions {
  filters?: SearchFilters | null;
  sort?: SearchSort;
  desc?: boolean;
  limit?: number;
  /** next_cursor of the previous page */
  cursor?: string | null;
}

export const fsSearch = (vault: VaultId, query: string, o: SearchOptions = {}) =>
  call<SearchResult>('fs_search', {
    vault,
    query,
    filters: o.filters ?? null,
    sort: o.sort ?? null,
    desc: o.desc ?? false,
    limit: o.limit ?? null,
    cursor: o.cursor ?? null,
  });

export const tagsList = (vault: VaultId) => call<Tag[]>('tags_list', { vault });
export const tagsCreate = (vault: VaultId, name: string, color: TagColor) => call<Tag>('tags_create', { vault, name, color });
export const tagsUpdate = (vault: VaultId, tag: TagId, patch: { name?: string; color?: TagColor }) =>
  call<Tag>('tags_update', { vault, tag, name: patch.name ?? null, color: patch.color ?? null });
export const tagsDelete = (vault: VaultId, tag: TagId) => call<void>('tags_delete', { vault, tag });
export const tagsAssign = (vault: VaultId, tags: TagId[], nodes: NodeId[]) => call<Tag[]>('tags_assign', { vault, tags, nodes });
export const tagsUnassign = (vault: VaultId, tags: TagId[], nodes: NodeId[]) => call<Tag[]>('tags_unassign', { vault, tags, nodes });
/** Keys are node ids as strings (JSON objects); nodes without tags are absent. */
export const tagsOf = (vault: VaultId, nodes: NodeId[]) => call<Record<string, TagId[]>>('tags_of', { vault, nodes });
export const onTagsChanged = (h: (p: EventMap['tags-changed']) => void) => on('tags-changed', h);
export const onSearchIndex = (h: (p: EventMap['search-index']) => void) => on('search-index', h);
