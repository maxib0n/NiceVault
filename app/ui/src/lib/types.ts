export type VaultId = string;
export type NodeId = number;
export type JobId = string;

export const ROOT: NodeId = 1;

export interface ApiError {
  code: string;
  message: string;
}

export type Kdf = 'fast' | 'strong';

export interface VaultInfo {
  id: VaultId;
  path: string;
  name: string;
  mountpoint: string | null;
  kdf?: { m_kib: number; t: number; p: number };
  emblem: Emblem;
  emblem_frame?: string;
  show_when_locked: boolean;
  visibility?: Visibility;
  settings?: Settings;
  appearance_tampered?: boolean;
  volume_tag?: string;
  single_file?: boolean;
  can_upgrade_format?: boolean;
  rolled_back?: { seen: number; now: number };
}

export type Visibility = 'visible' | 'encrypted';

export interface AppearanceState {
  accent: string;
  emblem: Emblem;
  emblem_frame?: string;
  visibility: Visibility;
  tampered: boolean;
}

export type Emblem = { kind: 'initial' } | { kind: 'icon'; id: string } | { kind: 'image'; data_base64: string };

export interface EmblemState {
  emblem: Emblem;
  show_when_locked: boolean;
  emblem_frame?: string;
}

export interface RecentEntry {
  path: string;
  name: string;
  size_bytes: number;
  last_opened: number;
  exists: boolean;
  unlock_ms?: number;
  emblem?: Emblem | null;
  show_when_locked?: boolean;
  accent?: string | null;
  source?: 'here' | 'removable' | 'known' | 'session';
  volume_tag?: string | null;
  key_slots?: number | null;
  kdf?: { m_kib: number; t: number; p: number } | null;
  emblem_frame?: string | null;
}

export type NodeKind = 'file' | 'dir';

export interface FsNode {
  id: NodeId;
  parent: NodeId;
  name: string;
  kind: NodeKind;
  size: number;
  mtime: number;
  tags?: TagId[];
}

export interface JobProgress {
  job: JobId;
  done_bytes: number;
  total_bytes: number;
  current: string;
  done_items?: number;
  total_items?: number;
}

export interface DeleteOutcome {
  deleted: number;
  total: number;
  freed_bytes: number;
  errors: { node: NodeId; name: string; code: string; message: string }[];
}

export interface JobDone {
  job: JobId;
  error?: ApiError | string | null;
  outcome?: unknown;
}

export type LockReason = 'user' | 'idle' | 'removed' | 'sleep' | 'screen';

export interface VaultLocked {
  vault: VaultId;
  reason: LockReason;
}

export interface FsChanged {
  vault: VaultId;
  dirs: NodeId[];
}

export type BackendId = 'fuse' | 'nfs' | 'winfsp';

export interface Backend {
  id: BackendId;
  label: string;
  available: boolean;
  reason?: string | null;
  reason_code?: string | null;
  native?: boolean;
  link?: string | null;
  min_version?: string | null;
}

export type SlotKind = 'password' | 'recipient' | 'fido2' | 'duress';

export interface SlotInfo {
  index: number;
  kind: SlotKind;
  label?: string | null;
  kdf?: { m_kib: number; t: number; p: number } | null;
  fingerprint?: string | null;
  in_use: boolean;
}

export interface VaultStats {
  used_bytes: number;
  file_count: number;
  file_size_bytes: number;
  free_bytes?: number;
  modified_ms?: number;
}

export type ThemeId = 'system' | 'light' | 'dark';

export interface Settings {
  auto_lock_minutes: number;
  accent: string;
  theme: ThemeId;
  lock_on_sleep: boolean;
  lock_on_screen_lock: boolean;
  lock_scope: LockScope;
  grid_size: GridSize;
  list_density: ListDensity;
}

export type LockScope = 'volume' | 'all';
export type GridSize = 's' | 'm' | 'l' | 'xl';
export type ListDensity = 'compact' | 'normal' | 'comfortable';

export interface AutoLockStatus {
  vault: VaultId;
  auto_lock_minutes: number;
  remaining_ms: number | null;
}

export interface ReleaseNotes {
  it: string;
  en: string;
}

export type UpdateStatus =
  | { status: 'skipped' }
  | { status: 'up_to_date'; current: string }
  | { status: 'available'; current: string; version: string; notes: ReleaseNotes; pub_date: string | null };

export type InstallOutcome = { mode: 'replaced'; path: string } | { mode: 'downloaded'; path: string };

export interface UpdatePrefs {
  check_on_start: boolean;
  last_check: number | null;
  portable: boolean;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

export interface EventMap {
  'job-progress': JobProgress;
  'job-done': JobDone;
  'vault-locked': VaultLocked;
  'fs-changed': FsChanged;
  'update-progress': UpdateProgress;
  'pm-icons': { vault: VaultId; done: number; total: number };
  'open-path': null;
}

export type EventName = keyof EventMap;

export interface Drive {
  id: string;
  label: string;
  mount_path: string;
  folder: string;
  fs_type: string | null;
  total_bytes: number;
  free_bytes: number;
  removable: boolean;
  system: boolean;
  app_here: boolean;
  is_nicevault_device: boolean;
}

export interface DrivesChanged {
  drives: Drive[];
}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type -- declaration merging
export interface EventMap {
  'drives-changed': DrivesChanged;
}

export interface NodeProps {
  id: NodeId;
  name: string;
  kind: NodeKind;
  path: string;
  size: number;
  mtime: number;
  created: number | null;
  imported: number | null;
  origin: string | null;
  mime: string | null;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  sha256: string | null;
  children: number | null;
}

export interface PropsProgress {
  job: JobId;
  files?: number;
  dirs?: number;
  bytes?: number;
  done_bytes?: number;
  total_bytes?: number;
}

export interface PropsDone extends PropsProgress {
  error?: ApiError | null;
  sha256?: string;
}

export interface EventMap {
  'props-progress': PropsProgress;
  'props-done': PropsDone;
}

export type KindGroup = 'photos' | 'videos' | 'audio' | 'documents' | 'archives' | 'code' | 'other' | 'folders';

export type TagId = number;

export type TagColor = 'velluto' | 'prugna' | 'prussia' | 'bottiglia' | 'seppia' | 'ardesia' | 'ottanio' | 'neutral';

export interface Tag {
  id: TagId;
  name: string;
  color: TagColor | string;
  count: number;
}

export interface SearchFilters {
  kinds?: KindGroup[];
  modified_after?: number | null;
  modified_before?: number | null;
  min_size?: number | null;
  max_size?: number | null;
  tags?: TagId[];
  tags_any?: boolean;
  within?: NodeId | null;
}

export type SearchSort = 'relevance' | 'name' | 'size' | 'mtime' | 'type';

export interface SearchHit {
  node: FsNode;
  path: string;
  highlights: [number, number][];
  tags: TagId[];
}

export interface SearchResult {
  hits: SearchHit[];
  total: number;
  next_cursor: string | null;
  pending: number;
}

export interface SearchIndexProgress {
  vault: VaultId;
  done: number;
  total: number;
}

export interface TagsChanged {
  vault: VaultId;
  nodes: NodeId[];
  dirs: NodeId[];
}

export interface EventMap {
  'search-index': SearchIndexProgress;
  'tags-changed': TagsChanged;
}

export interface ResizeInfo {
  capacity: number | null;
  file_bytes: number;
  used_bytes: number;
  min_capacity: number;
  min_in_place: number;
  max_capacity: number;
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

export type PickReason = 'offline' | 'icloud' | 'stalled' | 'unsupported' | 'no_space' | 'cancelled' | 'expired' | 'other';

export interface PickFailure {
  index: number;
  name: string;
  reason: PickReason;
  message: string;
}

export interface PickProgress {
  job: JobId;
  total: number;
  done: number;
  imported: number;
  failed: number;
  fetching: number;
  fraction: number;
  finished: boolean;
  cancelled: boolean;
  failures: PickFailure[] | null;
}

export interface PhotoImport {
  job: JobId;
  total: number;
}

export interface EventMap {
  'mobile-pick-progress': PickProgress;
}
