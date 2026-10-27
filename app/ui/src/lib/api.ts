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

let mockInstance: MockBackend | null = null;

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
      (window as unknown as { __nv: unknown }).__nv = { mock: mockInstance };
    }
  }
  return mockInstance;
}

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

export const vaultCreate = (path: string, password: string, kdf: Kdf = 'strong', fixedSize?: number | null) =>
  call<VaultInfo>('vault_create', fixedSize ? { path, password, kdf, fixed_size: fixedSize } : { path, password, kdf });
export const vaultOpen = (path: string, password: string) => call<VaultInfo>('vault_open', { path, password });
export const vaultLock = (vault: VaultId, reason: LockReason = 'user') => call<void>('vault_lock', { vault, reason });
export const vaultListOpen = () => call<VaultInfo[]>('vault_list_open');
export const vaultStats = (vault: VaultId) => call<VaultStats>('vault_stats', { vault });
export const vaultUpgradeFormat = (vault: VaultId) => call<VaultInfo>('vault_upgrade_format', { vault });
export const vaultAcceptRollback = (vault: VaultId) => call<VaultInfo>('vault_accept_rollback', { vault });

export const vaultResizeInfo = (vault: VaultId) => call<ResizeInfo>('vault_resize_info', { vault });
export const vaultResize = (vault: VaultId, size: number) => call<ResizeInfo>('vault_resize', { vault, size });
export const onVaultResize = (h: (p: ResizeProgress) => void) => on('vault-resize', h);

export const vaultEmblemGet = (vault: VaultId) => call<EmblemState>('vault_emblem_get', { vault });
export const vaultEmblemSet = (vault: VaultId, emblem: Emblem, show_when_locked = true, locked_image_base64?: string | null, emblem_frame?: string | null) =>
  call<EmblemState>('vault_emblem_set', { vault, emblem, show_when_locked, locked_image_base64: locked_image_base64 ?? null, emblem_frame: emblem_frame ?? null });

export const vaultAppearanceGet = (vault: VaultId) => call<AppearanceState>('vault_appearance_get', { vault });
export interface AppearancePatch {
  accent?: string;
  emblem?: Emblem;
  emblem_frame?: string;
  visibility?: Visibility;
  locked_image_base64?: string | null;
}
export const vaultAppearanceSet = (vault: VaultId, patch: AppearancePatch) =>
  notifySettings(vault, call<AppearanceState>('vault_appearance_set', { vault, ...patch }), (a) => ({ accent: a.accent }));

export const recentList = () => call<RecentEntry[]>('recent_list');
export const recentForget = (path: string) => call<RecentEntry[]>('recent_forget', { path });
export const defaultVolumeDir = () => call<string>('default_volume_dir');

let activeVault: () => VaultId | null = () => null;

export function bindActiveVault(get: () => VaultId | null): void {
  activeVault = get;
}

type SettingsListener = (vault: VaultId | null, s: Partial<Settings>) => void;
const settingsListeners = new Set<SettingsListener>();

export function onSettingsChanged(l: SettingsListener): Unlisten {
  settingsListeners.add(l);
  return () => settingsListeners.delete(l);
}

async function notifySettings<T>(vault: VaultId | null, p: Promise<T>, pick: (r: T) => Partial<Settings>): Promise<T> {
  const r = await p;
  for (const l of settingsListeners) l(vault, pick(r));
  return r;
}

export const settingsGet = (vault: VaultId | null = activeVault()) => call<Settings>('settings_get', { vault });
export function settingsSet(settings: Partial<Settings>, vault: VaultId | null = activeVault()): Promise<Settings> {
  return notifySettings(vault, call<Settings>('settings_set', { settings, vault }), (s) => s);
}

export const fsList = (vault: VaultId, dir: NodeId, filters?: SearchFilters | null) =>
  call<FsNode[]>('fs_list', { vault, dir, filters: filters ?? null });
export const fsMkdir = (vault: VaultId, parent: NodeId, name: string) => call<FsNode>('fs_mkdir', { vault, parent, name });
export const fsRename = (vault: VaultId, node: NodeId, new_parent: NodeId, new_name: string) =>
  call<void>('fs_rename', { vault, node, new_parent, new_name });
export const fsDelete = (vault: VaultId, nodes: NodeId[]) => call<JobId>('fs_delete', { vault, nodes });
export const fsExport = (vault: VaultId, nodes: NodeId[], dest_dir: string) =>
  call<JobId>('fs_export', { vault, nodes, dest_dir });
export const fsOpenExternal = (vault: VaultId, node: NodeId): Promise<{ host_disk_notice: boolean } | null> =>
  isMobile && isTauri
    ? mobileShare(vault, [node]).then(() => null)
    : call<{ host_disk_notice: boolean } | null>('fs_open_external', { vault, node });

export const mobileVolumeIn = (uri: string) => call<string>('mobile_volume_in', { uri });
export const mobileShare = (vault: VaultId, nodes: NodeId[]) => call<void>('mobile_share', { vault, nodes, title: m.mobile_share_title() });
export const mobileExportDir = () => call<string>('mobile_export_dir');
export const mobileImportPhotos = (vault: VaultId, parent: NodeId, retry?: number[]) =>
  call<PhotoImport | null>('mobile_import_photos', { vault, parent, retry: retry ?? null });
export const onPickProgress = (h: (p: EventMap['mobile-pick-progress']) => void) => on('mobile-pick-progress', h);
export const mobileSaveGallery = (vault: VaultId, nodes: NodeId[]) =>
  call<{ saved: NodeId[]; others: NodeId[] }>('mobile_save_gallery', { vault, nodes });
export const mobileDocsBusy = () => call<boolean>('mobile_docs_busy');
export const mobilePickFiles = (multiple: boolean) => call<string[]>('mobile_pick_files', { multiple });
export const mediaUrl = (vault: VaultId, node: NodeId) => call<string | null>('media_url', { vault, node });
export const mediaRelease = (url: string) => call<void>('media_release', { url });
export const fsOpenMounted = (vault: VaultId, node: NodeId) => call<void>('fs_open_mounted', { vault, node });
export const openLink = (url: string) => call<void>('open_link', { url });
export const jobCancel = (job: JobId) => call<void>('job_cancel', { job });

export type ImportSource = string | File;

export const importSize = (paths: string[]) => call<number>('import_size', { paths });

export function fsImport(vault: VaultId, parent: NodeId, sources: ImportSource[]): Promise<JobId> {
  const host_paths = isTauri ? sources.filter((s): s is string => typeof s === 'string') : sources;
  return call<JobId>('fs_import', { vault, parent, host_paths });
}

export const mountAvailable = () => call<Backend[]>('mount_available');
export const mount = (vault: VaultId) => call<{ mountpoint: string }>('mount', { vault });
export const unmount = (vault: VaultId) => call<void>('unmount', { vault });

export const slotsList = (vault: VaultId) => call<SlotInfo[]>('slots_list', { vault });
export const slotAddPassword = (vault: VaultId, password: string, label?: string) =>
  call<SlotInfo[]>('slot_add_password', { vault, password, label: label ?? null });
export const slotChangePassword = (vault: VaultId, slot: number, old_password: string, new_password: string) =>
  call<SlotInfo[]>('slot_change_password', { vault, slot, old_password, new_password });
export const slotRemove = (vault: VaultId, slot: number) => call<SlotInfo[]>('slot_remove', { vault, slot });
export const slotRotateKey = (vault: VaultId, slot: number, password: string) => call<SlotInfo[]>('slot_rotate_key', { vault, slot, password });

const isWindows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
const httpSchemes = isWindows || (typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent));

export function previewUrl(vault: VaultId, node: NodeId, thumb?: number, fit?: number): string {
  if (!isTauri) return mock().assetUrl(vault, node) ?? '';
  const v = encodeURIComponent(vault);
  const q = thumb ? `?thumb=${Math.round(thumb)}` : fit ? `?fit=${Math.round(fit)}` : '';
  return httpSchemes ? `http://nvault.localhost/${v}/${node}${q}` : `nvault://${v}/${node}${q}`;
}

const nvaultFilter = () => [{ name: m.dialog_volume_filter(), extensions: ['nvault'] }];

export async function pickVolumeFile(): Promise<string | null> {
  if (!isTauri) return `${STICK}/Archivio fiscale.nvault`;
  if (isIos) {
    const [p] = await mobilePickFiles(false);
    return p ? mobileVolumeIn(p) : null;
  }
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: false, directory: false, filters: isMobile ? undefined : nvaultFilter(), title: m.dialog_open_volume() });
  if (typeof r !== 'string') return null;
  return isMobile ? mobileVolumeIn(r) : r;
}

export async function pickFolder(title: string, defaultPath?: string): Promise<string | null> {
  if (!isTauri) return defaultPath ?? '/home/utente/Scrivania';
  if (isMobile) return mobileExportDir();
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ directory: true, multiple: false, title, defaultPath });
  return typeof r === 'string' ? r : null;
}

export async function pickImportFiles(): Promise<string[]> {
  if (!isTauri) {
    const pick = new URLSearchParams(location.search).get('import_pick');
    return pick ? pick.split(',').map((n) => `/home/utente/Scaricati/${n}`) : ['/home/utente/Scaricati/Bolletta luce.pdf', '/home/utente/Immagini/Spiaggia.jpg'];
  }
  if (isIos) return mobilePickFiles(true);
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: true, directory: false, title: m.dialog_import_into() });
  return Array.isArray(r) ? r : typeof r === 'string' ? [r] : [];
}

export interface DragPoint {
  x: number;
  y: number;
}

export type OsDragEvent =
  | { type: 'enter' | 'over'; paths?: string[]; position?: DragPoint }
  | { type: 'drop'; paths: string[]; position?: DragPoint }
  | { type: 'leave' };

export async function onOsDragDrop(handler: (e: OsDragEvent) => void): Promise<Unlisten> {
  if (!isTauri) return mock().onOsDrag(handler);
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  const scale = isWindows ? window.devicePixelRatio || 1 : 1;
  const point = (p?: { x: number; y: number }): DragPoint | undefined => (p ? { x: p.x / scale, y: p.y / scale } : undefined);
  return getCurrentWebview().onDragDropEvent((e) => {
    const p = e.payload as { type: string; paths?: string[]; position?: { x: number; y: number } };
    if (p.type === 'drop') handler({ type: 'drop', paths: p.paths ?? [], position: point(p.position) });
    else if (p.type === 'leave') handler({ type: 'leave' });
    else handler({ type: p.type === 'enter' ? 'enter' : 'over', paths: p.paths, position: point(p.position) });
  });
}

export * from './passwords';

export const vaultTouch = (vault: VaultId) => call<void>('vault_touch', { vault });
export const autolockStatus = () => call<import('./types').AutoLockStatus[]>('autolock_status');
export const slotAddDuress = (vault: VaultId, password: string, label?: string) =>
  call<SlotInfo[]>('slot_add_duress', { vault, password, label: label ?? null });

export interface AppSettings {
  language: string;
  defaults: Settings;
  persisted: boolean;
  site_icons: boolean;
  sounds: boolean;
  video_arrows: VideoArrows;
  sidebar_collapsed: boolean;
  app_theme: ThemeId;
  app_accent: string;
  ui_scale: number;
  new_like_app: boolean;
  start_at_login: boolean;
  start_minimized: boolean;
  close_to_tray: boolean;
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

export const UI_SCALES = [90, 100, 110, 125, 150] as const;

export interface AppInfo {
  version: string;
  portable: boolean;
  config_dir: string | null;
}

export const appSettingsGet = () => call<AppSettings>('app_settings_get');
export const appSettingsSet = (patch: AppSettingsPatch) => call<AppSettings>('app_settings_set', { patch });
export const appInfo = () => call<AppInfo>('app_info');

export const launchPathTake = () => call<string | null>('launch_path_take');
export const onOpenPath = (h: () => void) => on('open-path', () => h());

export async function pickFile(title: string, extensions?: string[]): Promise<string | null> {
  if (!isTauri) return `/home/utente/Scaricati/export.${extensions?.[0] ?? 'txt'}`;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: false, directory: false, title, filters: extensions ? [{ name: title, extensions }] : undefined });
  return typeof r === 'string' ? r : null;
}

export const onFsChanged = (h: (p: EventMap['fs-changed']) => void) => on('fs-changed', h);

export const updateCheck = (auto = false) => call<UpdateStatus>('update_check', { auto });
export const updateDownload = () => call<void>('update_download');
export const updateInstall = () => call<InstallOutcome>('update_install');
export const updateRestart = () => call<void>('update_restart');
export const updatePrefsGet = () => call<UpdatePrefs>('update_prefs_get');
export const updatePrefsSet = (check_on_start: boolean) => call<UpdatePrefs>('update_prefs_set', { check_on_start });

export const drivesList = () => call<import('./types').Drive[]>('drives_list');
export const onDrivesChanged = (h: (p: EventMap['drives-changed']) => void) => on('drives-changed', h);

export const fsProps = (vault: VaultId, node: NodeId) => call<import('./types').NodeProps>('fs_props', { vault, node });
export const fsPropsTree = (vault: VaultId, nodes: NodeId[]) => call<JobId>('fs_props_tree', { vault, nodes });
export const fsHash = (vault: VaultId, node: NodeId) => call<JobId>('fs_hash', { vault, node });
export const onPropsProgress = (h: (p: EventMap['props-progress']) => void) => on('props-progress', h);
export const onPropsDone = (h: (p: EventMap['props-done']) => void) => on('props-done', h);

export interface SearchOptions {
  filters?: SearchFilters | null;
  sort?: SearchSort;
  desc?: boolean;
  limit?: number;
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
export const tagsOf = (vault: VaultId, nodes: NodeId[]) => call<Record<string, TagId[]>>('tags_of', { vault, nodes });
export const onTagsChanged = (h: (p: EventMap['tags-changed']) => void) => on('tags-changed', h);
export const onSearchIndex = (h: (p: EventMap['search-index']) => void) => on('search-index', h);
