// Password manager (pm_* commands): types and typed calls. Entries live inside the open volume.
//
// Secrets never come back from pmList / pmSearch / pmGet: the password, the TOTP secret, hidden
// custom field values and the password history are read only with pmReveal (show) or pmCopy
// (clipboard, cleared by the backend after `clear_after` seconds if it still holds the value).
// Mock parity: mock/passwords.ts (seeded entries in every mock volume, TOTP included).
//
// Screen: screens/Passwords.svelte (store: stores/pm.svelte.ts).

import { call, isTauri, mock } from './api';
import type { JobId, VaultId } from './types';

export type EntryId = string;

/** List / search row (no secrets). Times are unix ms. */
export interface PmEntrySummary {
  id: EntryId;
  title: string;
  username: string;
  urls: string[];
  /** "A/B" path, "" = no folder */
  folder: string;
  tags: string[];
  favorite: boolean;
  has_password: boolean;
  has_totp: boolean;
  created: number;
  updated: number;
  /** "<host>/<version>" when the first website's icon is stored in the volume (pmIconUrl), else null */
  icon: string | null;
}

export interface PmFieldView {
  name: string;
  hidden: boolean;
  /** null when hidden: reveal it with field `field:<index>` */
  value: string | null;
}

export type TotpAlgorithm = 'SHA1' | 'SHA256' | 'SHA512';

export interface PmTotpInfo {
  issuer: string | null;
  account: string | null;
  algorithm: TotpAlgorithm;
  digits: number;
  period: number;
}

/** pmGet: the summary + notes, custom fields (hidden values blanked), TOTP parameters. */
export interface PmEntry extends PmEntrySummary {
  notes: string;
  fields: PmFieldView[];
  totp: PmTotpInfo | null;
  history_count: number;
}

export interface PmFieldInput {
  name: string;
  /** omitted / null = keep the current value of the field with this name (hidden fields) */
  value?: string | null;
  hidden?: boolean;
}

/**
 * Create / update payload. Update: omitted keys stay unchanged. Create: omitted = empty;
 * `title` is required (non-blank). `totp`: otpauth:// URI (QR code text) or base32 secret; "" removes.
 * Changing `password` moves the old one into the history (last 10 kept).
 */
export interface PmEntryInput {
  title?: string;
  username?: string;
  password?: string;
  urls?: string[];
  notes?: string;
  totp?: string;
  folder?: string;
  tags?: string[];
  favorite?: boolean;
  fields?: PmFieldInput[];
}

/** What pmReveal / pmCopy read. `field:<i>` = custom field i; `history:<i>` = old password i (0 = newest). */
export type PmSecretField = 'password' | 'username' | 'notes' | 'totp' | 'totp_code' | `field:${number}` | `history:${number}`;

export interface PmHistoryItem {
  index: number;
  /** unix ms when it stopped being the password */
  changed: number;
}

export interface PmTotpCode {
  code: string;
  /** seconds until it changes (1..period) */
  remaining: number;
  period: number;
  digits: number;
}

/** pmTotpCheck: parameters + current code of a TOTP input that is not stored yet. */
export type PmTotpCheck = PmTotpInfo & PmTotpCode;

export interface PmCopied {
  /** seconds until the clipboard is wiped (0 = never) */
  clear_after: number;
}

export type PmGenOptions =
  | {
      kind: 'random';
      length?: number; // 4..128, default 20
      lower?: boolean; // default true
      upper?: boolean; // default true
      digits?: boolean; // default true
      symbols?: boolean; // default true
      exclude_ambiguous?: boolean; // drops 0 O 1 l I |
    }
  | {
      kind: 'passphrase';
      words?: number; // 3..20, default 5
      language?: 'en' | 'it'; // EFF large wordlist / Italian list, 7776 words each
      separator?: string; // <= 3 chars, default "-"
      capitalize?: boolean;
      number?: boolean; // one digit appended to one word
    };

export interface PmGenerated {
  value: string;
  entropy_bits: number;
}

export type PmImportKind = 'bitwarden' | 'keepass' | 'csv';

export interface PmImportSource {
  /** host path of the export file (Tauri) */
  path?: string;
  /** file contents instead of a path: text for bitwarden / csv, base64 for keepass */
  data?: string;
  /** KeePass master password */
  master_password?: string;
  /** KeePass key file: host path */
  keyfile?: string;
}

/** job-done of a pm_import carries the number of entries written. */
export interface PmImportDone {
  job: JobId;
  error?: { code: string; message: string } | null;
  imported: number;
}

/** Error codes of the pm_* commands (besides locked / not_found / io). */
export type PmErrorCode =
  | 'invalid_entry'
  | 'invalid_totp'
  | 'invalid_field'
  | 'invalid_options'
  | 'invalid_import'
  | 'unsupported'
  | 'wrong_password'
  | 'clipboard';

export const PM_DEFAULT_CLEAR_AFTER = 30;

export const pmList = (vault: VaultId) => call<PmEntrySummary[]>('pm_list', { vault });
export const pmSearch = (vault: VaultId, query: string) => call<PmEntrySummary[]>('pm_search', { vault, query });
export const pmGet = (vault: VaultId, id: EntryId) => call<PmEntry>('pm_get', { vault, id });
export const pmCreate = (vault: VaultId, entry: PmEntryInput) => call<PmEntry>('pm_create', { vault, entry });
export const pmUpdate = (vault: VaultId, id: EntryId, entry: PmEntryInput) => call<PmEntry>('pm_update', { vault, id, entry });
export const pmDelete = (vault: VaultId, id: EntryId) => call<void>('pm_delete', { vault, id });
export const pmHistory = (vault: VaultId, id: EntryId) => call<PmHistoryItem[]>('pm_history', { vault, id });
export const pmReveal = (vault: VaultId, id: EntryId, field: PmSecretField) => call<string>('pm_reveal', { vault, id, field });
export const pmCopy = (vault: VaultId, id: EntryId, field: PmSecretField, clear_after: number = PM_DEFAULT_CLEAR_AFTER) =>
  call<PmCopied>('pm_copy', { vault, id, field, clear_after });
export const pmTotp = (vault: VaultId, id: EntryId) => call<PmTotpCode>('pm_totp', { vault, id });
export const pmTotpCheck = (totp: string) => call<PmTotpCheck>('pm_totp_check', { totp });
export const pmGenerate = (opts: PmGenOptions = { kind: 'random' }) => call<PmGenerated>('pm_generate', { opts });
/** Moves entries into `folder` ("A/B", "" = none); returns how many moved. `updated` is not touched. */
export const pmMove = (vault: VaultId, ids: EntryId[], folder: string) => call<number>('pm_move', { vault, ids, folder });
/** Renames a folder and everything below it (merges into an existing one); "not_found" when empty. */
export const pmFolderRename = (vault: VaultId, from: string, to: string) => call<number>('pm_folder_rename', { vault, from, to });
/** Deletes a folder: its entries and subfolders move up into its parent. */
export const pmFolderDelete = (vault: VaultId, folder: string) => call<number>('pm_folder_delete', { vault, folder });
/** "Ripara voci importate": maps raw export fields (AUTOFILLURLS, CREATETIME...) of older imports; returns entries changed. */
export const pmRepair = (vault: VaultId) => call<number>('pm_repair', { vault });
/**
 * Fetches the site icons that are due (never tried, or older than a week) in the background,
 * directly from each site; no-op when the site_icons setting is off. Returns how many hosts it
 * tries; `pm-icons {vault, done, total}` events follow (then reload the list).
 */
export const pmIconsRefresh = (vault: VaultId) => call<number>('pm_icons_refresh', { vault });

const isWindows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
/** WebView2 (Windows) and the Android WebView reach custom schemes as http://<scheme>.localhost */
const httpSchemes = isWindows || (typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent));

/** <img> URL of an entry's stored site icon (`icon` from the summary). */
export function pmIconUrl(vault: VaultId, icon: string): string {
  if (!isTauri) return mock().pm.iconUrl(icon);
  const v = encodeURIComponent(vault);
  const path = icon.split('/').map(encodeURIComponent).join('/');
  return httpSchemes ? `http://nvault.localhost/${v}/pm-icon/${path}` : `nvault://${v}/pm-icon/${path}`;
}

/** Starts an import job: listen to job-progress (done_bytes / total_bytes = entries) and job-done (PmImportDone). */
export const pmImport = (vault: VaultId, kind: PmImportKind, src: PmImportSource) =>
  call<JobId>('pm_import', {
    vault,
    kind,
    path: src.path ?? null,
    data: src.data ?? null,
    master_password: src.master_password ?? null,
    keyfile: src.keyfile ?? null,
  });
