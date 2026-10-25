// Wire types shared with the Tauri backend (app/src-tauri) and nv-fs.
// Field names are snake_case on purpose: they are exactly what serde emits.

export type VaultId = string;
export type NodeId = number; // nv_fs::NodeId (u64; safe as JS number below 2^53)
export type JobId = string;

export const ROOT: NodeId = 1;

/** Error shape of every command: `{ code, message }`. */
export interface ApiError {
  code: string; // e.g. "wrong_password", "not_found", "exists", "locked", "io", ...
  message: string;
}

export type Kdf = 'fast' | 'strong';

export interface VaultInfo {
  id: VaultId;
  path: string; // host path of the .nvault file
  name: string; // file stem, e.g. "Documenti"
  mountpoint: string | null; // set while mounted as an OS drive
  kdf?: { m_kib: number; t: number; p: number }; // parameters of the slot that unlocked it
  emblem: Emblem; // stored inside the volume
  /** the frame round the emblem (ornament/frames.ts FRAMES; unknown ids draw the default) */
  emblem_frame?: string;
  show_when_locked: boolean; // = visibility === 'visible' (kept for older callers)
  /** visible: accent + emblem copied to the header (locked screens show them); encrypted: only inside. */
  visibility?: Visibility;
  /** this volume's settings: apply them while it is the active one */
  settings?: Settings;
  /** the header's appearance had been edited while locked (it was rewritten from the volume) */
  appearance_tampered?: boolean;
  /** the document number: 9 characters from the header's volume id (store.rs volume_tag) */
  volume_tag?: string;
  /** format v2: the journal lives inside the .nvault (one file, nothing next to it) */
  single_file?: boolean;
  /** "Aggiorna formato" is available (vault_upgrade_format) */
  can_upgrade_format?: boolean;
  /** Opened OLDER than this computer last saw it (an older copy of the file); vault_accept_rollback keeps it */
  rolled_back?: { seen: number; now: number };
}

/** Whether a volume's look (accent + emblem) can be seen while it is locked. */
export type Visibility = 'visible' | 'encrypted';

/** vault_appearance_get / vault_appearance_set */
export interface AppearanceState {
  accent: string; // AccentId
  emblem: Emblem;
  emblem_frame?: string;
  visibility: Visibility;
  tampered: boolean;
}

/**
 * The big mark on the volume's document. Default: the initial letter (monogram).
 * `icon.id` is one of EMBLEM_ICONS (emblem.ts); `image` is a square PNG/WebP (<= 64 KB), base64.
 */
export type Emblem = { kind: 'initial' } | { kind: 'icon'; id: string } | { kind: 'image'; data_base64: string };

/** vault_emblem_get / vault_emblem_set (show_when_locked = visibility 'visible') */
export interface EmblemState {
  emblem: Emblem;
  show_when_locked: boolean;
  /** the frame round the emblem; absent = the guilloché rosette */
  emblem_frame?: string;
}

/**
 * One volume on Benvenuto. Not a stored list: a scan of the app's folder and removable drives
 * (+ the installed-mode known list), each read from the volume's header without a password.
 */
export interface RecentEntry {
  path: string;
  name: string;
  size_bytes: number; // size of the .nvault file
  last_opened: number; // unix ms: last opened by this app (session / known list), else the file's mtime
  exists: boolean; // false when the file is not reachable right now (other stick, moved)
  unlock_ms?: number; // hint for the unlock animation: measured here, else estimated from the KDF cost
  /** the header's emblem (visible volumes); null/absent = not known (show the initial) or encrypted */
  emblem?: Emblem | null;
  /** false = the volume keeps its look encrypted: neutral rosette, default accent, OS theme. Absent = true. */
  show_when_locked?: boolean;
  /** the header's accent preset id (visible volumes only) */
  accent?: string | null;
  /** where it was found */
  source?: 'here' | 'removable' | 'known' | 'session';
  /** the document number (9 characters from the header's volume id) */
  volume_tag?: string | null;
  /** occupied key slots (header) */
  key_slots?: number | null;
  /** Argon2id parameters of the first password slot (header) */
  kdf?: { m_kib: number; t: number; p: number } | null;
  /** the emblem's frame as this computer last saw the volume open (visible looks only) */
  emblem_frame?: string | null;
}

export type NodeKind = 'file' | 'dir';

/** nv_fs::Node */
export interface FsNode {
  id: NodeId;
  parent: NodeId;
  name: string;
  kind: NodeKind;
  size: number; // bytes, 0 for dirs
  mtime: number; // unix ns
  /** fs_list: the node's tags (absent = none); search hits carry them in SearchHit.tags */
  tags?: TagId[];
}

export interface JobProgress {
  job: JobId;
  done_bytes: number;
  total_bytes: number;
  current: string; // name of the file being processed
  /** delete jobs: items removed so far / in all (done_bytes = file data freed) */
  done_items?: number;
  total_items?: number;
}

/** `job-done.outcome` of a delete job (src-tauri delete.rs DeleteOutcome) */
export interface DeleteOutcome {
  deleted: number;
  total: number;
  freed_bytes: number;
  errors: { node: NodeId; name: string; code: string; message: string }[];
}

export interface JobDone {
  job: JobId;
  error?: ApiError | string | null;
  /** delete jobs: DeleteOutcome; device jobs: their own */
  outcome?: unknown;
}

/** screen = the session's screen was locked (lock_on_screen_lock) */
export type LockReason = 'user' | 'idle' | 'removed' | 'sleep' | 'screen';

export interface VaultLocked {
  vault: VaultId;
  reason: LockReason;
}

/** `fs-changed`: folders changed through the mounted drive (fs_events.rs; coalesced, <= 4/s). */
export interface FsChanged {
  vault: VaultId;
  dirs: NodeId[];
}

export type BackendId = 'fuse' | 'nfs' | 'winfsp';

export interface Backend {
  id: BackendId;
  label: string; // "FUSE", "NFS locale", "WinFsp"
  available: boolean;
  reason?: string | null; // English fallback when unavailable
  /** fuse_missing | linux_only | nfs_admin | nfs_missing | nfs_no_home | macos_only | winfsp_missing |
   *  winfsp_unsupported | winfsp_load_failed | windows_only (lib/backends.ts) */
  reason_code?: string | null;
  /** this OS's own backend: its reason is the one the "Unità di sistema" switch shows */
  native?: boolean;
  /** where to get what is missing (WinFsp's releases page), opened after a confirm */
  link?: string | null;
  /** oldest supported version of what is missing ("2.1") */
  min_version?: string | null;
}

/**
 * duress = "password di autodistruzione": entering it on Sblocca destroys the volume's keys and
 * answers exactly like a wrong password (slot_add_duress; removed with slot_remove). It is never
 * in_use, cannot be changed (invalid_slot) and cannot be the only key left (last_slot).
 */
export type SlotKind = 'password' | 'recipient' | 'fido2' | 'duress';

export interface SlotInfo {
  index: number; // 0..7
  kind: SlotKind;
  label?: string | null; // user label, e.g. "Principale", "Busta in cassaforte"
  kdf?: { m_kib: number; t: number; p: number } | null; // password slots
  fingerprint?: string | null; // recipient slots, hex
  in_use: boolean; // true for the slot that unlocked this session
}

export interface VaultStats {
  used_bytes: number; // sum of file sizes
  file_count: number;
  file_size_bytes: number; // size of the .nvault file on disk
  free_bytes?: number; // free space on the host drive
  /** when the volume's file was last written (unix ms; 0 = unknown) */
  modified_ms?: number;
}

export type ThemeId = 'system' | 'light' | 'dark';

/**
 * Per-volume settings (stored inside the volume). settings_get/set act on `vault` when given,
 * else on the most recently opened volume; with none open, on session defaults (seed of new volumes).
 */
export interface Settings {
  auto_lock_minutes: number; // idle minutes of THIS volume (its own timer); 0 = never ("mai")
  accent: string; // AccentId (accents.ts); unknown ids are ignored by settings_set
  theme: ThemeId; // unknown values are ignored by settings_set
  /** lock before the system sleeps (default true) */
  lock_on_sleep: boolean;
  /** lock when the session's screen is locked (default true) */
  lock_on_screen_lock: boolean;
  /** when this volume auto-locks (idle / sleep / screen): only it, or every open volume. Removal is always per volume. */
  lock_scope: LockScope;
  /** grid view tile size of this volume (default "m") */
  grid_size: GridSize;
  /** list view row density of this volume (default "normal") */
  list_density: ListDensity;
}

export type LockScope = 'volume' | 'all';
export type GridSize = 's' | 'm' | 'l' | 'xl';
export type ListDensity = 'compact' | 'normal' | 'comfortable';

/** autolock_status: the backend's idle countdown of one open volume (does not count as activity). */
export interface AutoLockStatus {
  vault: VaultId;
  auto_lock_minutes: number;
  /** null = never (minutes 0, or an import/export is running) */
  remaining_ms: number | null;
}

// --- updater (update_* commands) ---------------------------------------------

export interface ReleaseNotes {
  it: string;
  en: string;
}

/** update_check: skipped = automatic check not due (disabled, or done in the last 24 h). */
export type UpdateStatus =
  | { status: 'skipped' }
  | { status: 'up_to_date'; current: string }
  | { status: 'available'; current: string; version: string; notes: ReleaseNotes; pub_date: string | null };

/** update_install: replaced in place (offer Riavvia) or, location not writable, saved in Downloads. */
export type InstallOutcome = { mode: 'replaced'; path: string } | { mode: 'downloaded'; path: string };

export interface UpdatePrefs {
  check_on_start: boolean;
  /** unix ms of the last successful check */
  last_check: number | null;
  /** true = running from a stick: preferences last until the app quits */
  portable: boolean;
}

export interface UpdateProgress {
  downloaded: number;
  /** null when the server sends no length */
  total: number | null;
}

export interface EventMap {
  'job-progress': JobProgress;
  'job-done': JobDone;
  'vault-locked': VaultLocked;
  'fs-changed': FsChanged;
  'update-progress': UpdateProgress;
  /** password manager: site icons stored for `vault` (refresh the list) */
  'pm-icons': { vault: VaultId; done: number; total: number };
  /** a second launch (or the first) brought a volume file: take it with launchPathTake */
  'open-path': null;
}

export type EventName = keyof EventMap;

// --- drives (drives.rs): the places "Nuovo volume" offers, live ----------------

/** One mounted drive of this computer (drives_list, drives-changed). */
export interface Drive {
  /** stable while mounted (Linux device path, Windows "E:", macOS BSD name) */
  id: string;
  /** volume label; "" = none (name it from mount_path, or "this computer" when system) */
  label: string;
  mount_path: string;
  /** where a new volume goes when this drive is picked (the app's folder, Documents, or the root) */
  folder: string;
  fs_type: string | null;
  total_bytes: number;
  free_bytes: number;
  /** removable or external (stick, SD card, external disk) */
  removable: boolean;
  /** holds the OS / the user's home */
  system: boolean;
  /** the app runs from this drive */
  app_here: boolean;
  /** a partition labelled VaultData / NiceVault */
  is_nicevault_device: boolean;
}

/** `drives-changed`: a drive was plugged in, pulled out, mounted or renamed (not free space alone). */
export interface DrivesChanged {
  drives: Drive[];
}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type -- declaration merging
export interface EventMap {
  'drives-changed': DrivesChanged;
}

// --- properties (props.rs): Proprietà -------------------------------------------

/** fs_props: what is known about one node at once (heavier totals / hashes are jobs). */
export interface NodeProps {
  id: NodeId;
  name: string;
  kind: NodeKind;
  /** "/Foto/Mare.jpg"; "/" = the volume root */
  path: string;
  size: number;
  /** unix ns */
  mtime: number;
  /** unix ns; null = unknown (made before this was kept) */
  created: number | null;
  /** unix ns; null = made inside the volume, or unknown */
  imported: number | null;
  /** host path it was imported from */
  origin: string | null;
  /** sniffed from the first bytes */
  mime: string | null;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  /** hex, when computed for the current content */
  sha256: string | null;
  /** folders: items directly inside */
  children: number | null;
}

/** props-progress: a running totals job (files/dirs/bytes) or hash job (done_bytes/total_bytes). */
export interface PropsProgress {
  job: JobId;
  files?: number;
  dirs?: number;
  bytes?: number;
  done_bytes?: number;
  total_bytes?: number;
}

/** props-done: the job's result (or error, e.g. code "cancelled"). */
export interface PropsDone extends PropsProgress {
  error?: ApiError | null;
  sha256?: string;
}

export interface EventMap {
  'props-progress': PropsProgress;
  'props-done': PropsDone;
}

// --- search, filters, tags (search.rs) -----------------------------------------

/** Filter groups by extension (folders: "folders"); the map is in lib/search.ts (mirrors nv-core). */
export type KindGroup = 'photos' | 'videos' | 'audio' | 'documents' | 'archives' | 'code' | 'other' | 'folders';

export type TagId = number;

/** The 7 inks + neutral. A colour outside this list (a newer build's) shows as neutral. */
export type TagColor = 'velluto' | 'prugna' | 'prussia' | 'bottiglia' | 'seppia' | 'ardesia' | 'ottanio' | 'neutral';

export interface Tag {
  id: TagId;
  /** 1..64 characters; unique ignoring case, accents and repeated spaces */
  name: string;
  color: TagColor | string;
  /** nodes carrying it */
  count: number;
}

/** Narrows fs_search results and fs_list listings; every given field must hold. */
export interface SearchFilters {
  /** any of these (empty / absent = any) */
  kinds?: KindGroup[];
  /** unix ns, inclusive */
  modified_after?: number | null;
  /** unix ns, exclusive */
  modified_before?: number | null;
  /** bytes, inclusive; a size bound leaves folders out */
  min_size?: number | null;
  max_size?: number | null;
  /** nodes carrying these tags: all of them, or any with tags_any */
  tags?: TagId[];
  tags_any?: boolean;
  /** fs_search only: inside this folder, any depth (default: the whole volume) */
  within?: NodeId | null;
}

/** relevance = best match first (whole name, name start, word start, anywhere); folders first in the others */
export type SearchSort = 'relevance' | 'name' | 'size' | 'mtime' | 'type';

export interface SearchHit {
  node: FsNode;
  /** the folder holding it: "/" or "/Foto/Mare" */
  path: string;
  /** [start, end) UTF-16 offsets into node.name that matched (sorted, merged): string.slice() them */
  highlights: [number, number][];
  tags: TagId[];
}

export interface SearchResult {
  hits: SearchHit[];
  /** all matches (this page is a slice of them) */
  total: number;
  /** pass back as `cursor` for the next page; null = last page */
  next_cursor: string | null;
  /** names the stored index still has to fill (informational; results are complete) */
  pending: number;
}

/** search-index: the background fill of a volume's stored index (done === total at the end). */
export interface SearchIndexProgress {
  vault: VaultId;
  done: number;
  total: number;
}

/** tags-changed: re-read tags_list; re-list `dirs` on screen (chips of `nodes` changed). */
export interface TagsChanged {
  vault: VaultId;
  nodes: NodeId[];
  dirs: NodeId[];
}

export interface EventMap {
  'search-index': SearchIndexProgress;
  'tags-changed': TagsChanged;
}

// --- Estendi / Riduci (resize.rs): a fixed-size volume's capacity ---------------

/** vault_resize_info / vault_resize. Sizes in bytes. */
export interface ResizeInfo {
  /** the fixed capacity; null = a growable volume (its file follows its content: nothing to resize) */
  capacity: number | null;
  /** length of the volume file */
  file_bytes: number;
  /** store bytes holding data */
  used_bytes: number;
  /** smallest size a shrink accepts (data + margin; = capacity when it cannot shrink) */
  min_capacity: number;
  /** smallest size without compaction; below it the shrink needs the new size free on the drive */
  min_in_place: number;
  /** largest size the drive has room for */
  max_capacity: number;
  /** free space on the volume's drive */
  drive_free: number;
}

export type ResizePhase = 'compact' | 'extend' | 'commit' | 'truncate';

export interface ResizeProgress {
  vault: VaultId;
  phase: ResizePhase;
  done: number;
  total: number;
}

export interface EventMap {
  'vault-resize': ResizeProgress;
}

// --- camera roll import (src-tauri/src/mobile.rs mobile_import_photos, pick_queue.rs) -------

/** Why a picked photo / video could not be imported. */
export type PickReason = 'offline' | 'icloud' | 'stalled' | 'unsupported' | 'no_space' | 'cancelled' | 'expired' | 'other';

export interface PickFailure {
  /** position in the pick (for "Riprova questi") */
  index: number;
  name: string;
  reason: PickReason;
  message: string;
}

/** mobile-pick-progress: ~4/s while items are fetched (iCloud downloads) and imported, then once with finished. */
export interface PickProgress {
  job: JobId;
  total: number;
  /** imported + failed */
  done: number;
  imported: number;
  failed: number;
  /** items being downloaded now */
  fetching: number;
  /** overall 0..1, downloads in flight included */
  fraction: number;
  finished: boolean;
  cancelled: boolean;
  /** with finished: what could not be imported */
  failures: PickFailure[] | null;
}

export interface PhotoImport {
  job: JobId;
  total: number;
}

export interface EventMap {
  'mobile-pick-progress': PickProgress;
}
