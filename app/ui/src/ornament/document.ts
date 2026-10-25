// What a volume's document prints about itself, formatted from real data only:
// where it lives, and the machine-readable zone (with real ICAO 9303 check digits).
//   locked   (Sblocca): only what the file and its header say in plaintext: the document
//            number (from the header's volume id), the file's size, when it was last opened
//            here (already printed above the MRZ), how many keys open it, the key derivation.
//   unlocked (Dettagli): what the volume holds as well: when it was made, how many files,
//            when its file last changed.
import { checkDigit, mrzText, pad } from './lathe';

/**
 * Where the volume lives, as a place: the label of the drive it is on when that drive is
 * mounted as an external drive ("/run/media/u/STICK/x.nvault" -> "STICK", "/Volumes/Disk/..."),
 * else the folder it sits in ("C:\\Users\\me\\Docs\\x.nvault" -> "Docs"; a drive root -> "C:").
 * With `name`: '' when the place would only repeat the volume's name (SH2RE.nvault on a stick
 * called SH2RE), so a document never prints the same word twice.
 */
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

/** A field with its check digit; unknown = filler (whose check digit is 0, as ICAO prints it). */
function field(value: string): string {
  return value + checkDigit(value);
}

function digits(n: number | null | undefined, width: number): string {
  return n === null || n === undefined || !Number.isFinite(n) || n < 0 ? '<'.repeat(width) : String(Math.min(Math.round(n), 10 ** width - 1)).padStart(width, '0');
}

/** The document number: the volume's tag (9 characters), else filler. */
function docNumber(tag: string | null | undefined): string {
  return pad(mrzText(tag ?? '').slice(0, 9), 9);
}

/** "K" and the number of keys (two digits). */
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

/**
 * The MRZ of a locked volume, two 44-character lines: identity (place, name); then the
 * document number, the file's size in MiB, the last opening (YYMMDD), each with its check
 * digit, the keys (K and a count), the key derivation (memory MiB, passes, lanes) and the
 * composite check digit.
 */
export function mrzLines(v: LockedFacts): [string, string] {
  const size = v.size_bytes && v.size_bytes > 0 ? digits(Math.max(1, v.size_bytes / 1048576), 9) : '<<<<<<<<<';
  const kdf = v.kdf ? mrzText(`${Math.round(v.kdf.m_kib / 1024)}M${v.kdf.t}T${v.kdf.p}P`) : '';
  const body = `${field(docNumber(v.volume_tag))}${field(size)}${field(yymmdd(v.last_opened))}${keysField(v.key_slots)}${pad(kdf, 13)}`;
  const l2 = pad(body, 43);
  return [line1(v), l2 + checkDigit(l2)];
}

export interface InsideFacts {
  /** when the volume was made (unix ms; null = not recorded) */
  created?: number | null;
  files?: number | null;
  /** when its file last changed (unix ms) */
  changed?: number | null;
  keys?: number | null;
}

/**
 * The MRZ of an open volume (Dettagli): identity, then the document number, the day it was
 * made, its files, the day its file last changed (each with its check digit), the keys and the
 * composite check digit.
 */
export function mrzInside(v: { name: string; path: string; volume_tag?: string | null }, f: InsideFacts): [string, string] {
  const body = `${field(docNumber(v.volume_tag))}${field(yymmdd(f.created))}${field(digits(f.files, 9))}${field(yymmdd(f.changed))}${keysField(f.keys)}`;
  const l2 = pad(body, 43);
  return [line1(v), l2 + checkDigit(l2)];
}
