import { checkDigit, mrzText, pad } from './lathe';

export function placeOf(path: string, name?: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  let place = '';
  const ext = /^\/(run\/media\/[^/]+|media\/[^/]+|Volumes)\/([^/]+)\//.exec(path);
  if (ext) place = ext[2];
  else if (parts.length >= 2) place = parts[parts.length - 2];
  if (name !== undefined && place.localeCompare(name, undefined, { sensitivity: 'base' }) === 0) return '';
  return place;
}

function yymmdd(ms: number | null | undefined): string {
  if (!ms || ms <= 0) return '<<<<<<';
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getFullYear() % 100)}${p(d.getMonth() + 1)}${p(d.getDate())}`;
}

function field(value: string): string {
  return value + checkDigit(value);
}

function digits(n: number | null | undefined, width: number): string {
  return n === null || n === undefined || !Number.isFinite(n) || n < 0 ? '<'.repeat(width) : String(Math.min(Math.round(n), 10 ** width - 1)).padStart(width, '0');
}

function docNumber(tag: string | null | undefined): string {
  return pad(mrzText(tag ?? '').slice(0, 9), 9);
}

function keysField(n: number | null | undefined): string {
  return n === null || n === undefined ? '<<<' : `K${digits(n, 2)}`;
}

function line1(v: { name: string; path: string }): string {
  const place = mrzText(placeOf(v.path)).slice(0, 14);
  return pad(`NV<${place}<<${mrzText(v.name)}`, 44);
}

export interface LockedFacts {
  name: string;
  path: string;
  size_bytes?: number;
  last_opened?: number;
  volume_tag?: string | null;
  key_slots?: number | null;
  kdf?: { m_kib: number; t: number; p: number } | null;
}

export function mrzLines(v: LockedFacts): [string, string] {
  const size = v.size_bytes && v.size_bytes > 0 ? digits(Math.max(1, v.size_bytes / 1048576), 9) : '<<<<<<<<<';
  const kdf = v.kdf ? mrzText(`${Math.round(v.kdf.m_kib / 1024)}M${v.kdf.t}T${v.kdf.p}P`) : '';
  const body = `${field(docNumber(v.volume_tag))}${field(size)}${field(yymmdd(v.last_opened))}${keysField(v.key_slots)}${pad(kdf, 13)}`;
  const l2 = pad(body, 43);
  return [line1(v), l2 + checkDigit(l2)];
}

export interface InsideFacts {
  created?: number | null;
  files?: number | null;
  changed?: number | null;
  keys?: number | null;
}

export function mrzInside(v: { name: string; path: string; volume_tag?: string | null }, f: InsideFacts): [string, string] {
  const body = `${field(docNumber(v.volume_tag))}${field(yymmdd(f.created))}${field(digits(f.files, 9))}${field(yymmdd(f.changed))}${keysField(f.keys)}`;
  const l2 = pad(body, 43);
  return [line1(v), l2 + checkDigit(l2)];
}
