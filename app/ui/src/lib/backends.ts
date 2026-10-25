// Mount backends (mount_available) in the UI language: the backend sends a reason code.
import type { Backend } from './types';
import { m } from '../paraglide/messages.js';

const REASON: Record<string, (b: Backend) => string> = {
  fuse_missing: () => m.mount_fuse_missing(),
  linux_only: () => m.mount_linux_only(),
  nfs_admin: () => m.mount_nfs_admin(),
  nfs_missing: () => m.mount_nfs_missing(),
  nfs_no_home: () => m.mount_nfs_no_home(),
  macos_only: () => m.mount_macos_only(),
  winfsp_missing: () => m.mount_winfsp_missing(),
  winfsp_unsupported: (b) => m.mount_winfsp_unsupported({ version: b.min_version ?? '2.1' }),
  winfsp_load_failed: () => m.mount_winfsp_load_failed(),
  windows_only: () => m.mount_windows_only(),
};

/** Why a backend is unavailable (null when it is available or nothing is known). */
export function backendReason(b: Backend): string | null {
  if (b.available) return null;
  return (b.reason_code ? REASON[b.reason_code]?.(b) : null) ?? b.reason ?? null;
}

/** The backend whose state the system-drive switch explains: this OS's own one (older
 *  backends without `native`: the first unavailable one). */
export function nativeBackend(list: Backend[]): Backend | null {
  return list.find((b) => b.native) ?? list.find((b) => !b.available) ?? null;
}

/** "FUSE", "NFS locale", "WinFsp" */
export function backendLabel(b: Backend): string {
  return b.id === 'nfs' ? m.mount_nfs_label() : b.label;
}
