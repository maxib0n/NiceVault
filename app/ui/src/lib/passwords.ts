import { call, isTauri, mock } from './api';
import type { JobId, VaultId } from './types';

export type EntryId = string;

export interface PmEntrySummary {
  id: EntryId;
  title: string;
  username: string;
  urls: string[];
  folder: string;
  tags: string[];
  favorite: boolean;
  has_password: boolean;
  has_totp: boolean;
  created: number;
  updated: number;
  icon: string | null;
}

export interface PmFieldView {
  name: string;
  hidden: boolean;
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

export interface PmEntry extends PmEntrySummary {
  notes: string;
  fields: PmFieldView[];
  totp: PmTotpInfo | null;
  history_count: number;
}

export interface PmFieldInput {
  name: string;
  value?: string | null;
  hidden?: boolean;
}

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

export type PmSecretField = 'password' | 'username' | 'notes' | 'totp' | 'totp_code' | `field:${number}` | `history:${number}`;

export interface PmHistoryItem {
  index: number;
  changed: number;
}

export interface PmTotpCode {
  code: string;
  remaining: number;
  period: number;
  digits: number;
}

export type PmTotpCheck = PmTotpInfo & PmTotpCode;

export interface PmCopied {
  clear_after: number;
}

export type PmGenOptions =
  | {
      kind: 'random';
      length?: number;
      lower?: boolean;
      upper?: boolean;
      digits?: boolean;
      symbols?: boolean;
      exclude_ambiguous?: boolean;
    }
  | {
      kind: 'passphrase';
      words?: number;
      language?: 'en' | 'it';
      separator?: string;
      capitalize?: boolean;
      number?: boolean;
    };

export interface PmGenerated {
  value: string;
  entropy_bits: number;
}

export type PmImportKind = 'bitwarden' | 'keepass' | 'csv';

export interface PmImportSource {
  path?: string;
  data?: string;
  master_password?: string;
  keyfile?: string;
}

export interface PmImportDone {
  job: JobId;
  error?: { code: string; message: string } | null;
  imported: number;
}

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
export const pmMove = (vault: VaultId, ids: EntryId[], folder: string) => call<number>('pm_move', { vault, ids, folder });
export const pmFolderRename = (vault: VaultId, from: string, to: string) => call<number>('pm_folder_rename', { vault, from, to });
export const pmFolderDelete = (vault: VaultId, folder: string) => call<number>('pm_folder_delete', { vault, folder });
export const pmRepair = (vault: VaultId) => call<number>('pm_repair', { vault });
export const pmIconsRefresh = (vault: VaultId) => call<number>('pm_icons_refresh', { vault });

const isWindows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
const httpSchemes = isWindows || (typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent));

export function pmIconUrl(vault: VaultId, icon: string): string {
  if (!isTauri) return mock().pm.iconUrl(icon);
  const v = encodeURIComponent(vault);
  const path = icon.split('/').map(encodeURIComponent).join('/');
  return httpSchemes ? `http://nvault.localhost/${v}/pm-icon/${path}` : `nvault://${v}/pm-icon/${path}`;
}

export const pmImport = (vault: VaultId, kind: PmImportKind, src: PmImportSource) =>
  call<JobId>('pm_import', {
    vault,
    kind,
    path: src.path ?? null,
    data: src.data ?? null,
    master_password: src.master_password ?? null,
    keyfile: src.keyfile ?? null,
  });
