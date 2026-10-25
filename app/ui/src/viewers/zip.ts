// ZIP reading over ranged reads: the central directory is at the end of the file, so a
// listing costs two small requests whatever the archive's size. Entries are inflated in
// memory (DecompressionStream 'deflate-raw') with an output cap, for the Office/ODF text
// view; nothing is ever written out.

import type { ByteSource } from './source';

export interface ZipEntry {
  name: string;
  dir: boolean;
  size: number;
  compressed: number;
  method: number;
  encrypted: boolean;
  /** local header offset */
  offset: number;
  /** ms since epoch (DOS time, local) or null */
  mtime: number | null;
}

export interface ZipListing {
  entries: ZipEntry[];
  /** the archive has more entries than were read */
  truncated: boolean;
  total: number;
}

const u16 = (b: Uint8Array, o: number) => b[o] | (b[o + 1] << 8);
const u32 = (b: Uint8Array, o: number) => (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0;
const u64 = (b: Uint8Array, o: number) => u32(b, o) + u32(b, o + 4) * 2 ** 32;

function dosTime(time: number, date: number): number | null {
  if (!date) return null;
  const d = new Date(1980 + (date >> 9), ((date >> 5) & 15) - 1, date & 31, time >> 11, (time >> 5) & 63, (time & 31) * 2);
  return Number.isNaN(d.getTime()) ? null : d.getTime();
}

const CP437_HI =
  'ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ';
function decodeName(b: Uint8Array, utf8: boolean): string {
  if (utf8) return new TextDecoder('utf-8').decode(b);
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(b);
  } catch {
    let s = '';
    for (const c of b) s += c < 128 ? String.fromCharCode(c) : CP437_HI[c - 128];
    return s;
  }
}

export class ZipError extends Error {}

/** Signature check on the first bytes ("PK\x03\x04", or an empty archive's "PK\x05\x06"). */
export function looksLikeZip(head: Uint8Array): boolean {
  return head.length >= 4 && head[0] === 0x50 && head[1] === 0x4b && (head[2] === 3 || head[2] === 5) && (head[3] === 4 || head[3] === 6);
}

export async function listZip(src: ByteSource, maxEntries = 5000, signal?: AbortSignal): Promise<ZipListing> {
  const size = await src.size();
  if (size < 22) throw new ZipError('too small');
  const tailLen = Math.min(size, 65535 + 22 + 20);
  const tail = await src.read(size - tailLen, size, signal);
  let eocd = -1;
  for (let k = tail.length - 22; k >= 0; k--) {
    if (tail[k] === 0x50 && tail[k + 1] === 0x4b && tail[k + 2] === 5 && tail[k + 3] === 6) {
      eocd = k;
      break;
    }
  }
  if (eocd < 0) throw new ZipError('no end of central directory');
  let total = u16(tail, eocd + 10);
  let cdSize = u32(tail, eocd + 12);
  let cdOff = u32(tail, eocd + 16);
  // ZIP64: the locator sits just before the classic record
  const loc = eocd - 20;
  if ((total === 0xffff || cdOff === 0xffffffff || cdSize === 0xffffffff) && loc >= 0 && u32(tail, loc) === 0x07064b50) {
    const recOff = u64(tail, loc + 8);
    const rec = await src.read(recOff, recOff + 56, signal);
    if (u32(rec, 0) !== 0x06064b50) throw new ZipError('bad zip64 record');
    total = u64(rec, 32);
    cdSize = u64(rec, 40);
    cdOff = u64(rec, 48);
  }
  if (cdOff + cdSize > size) throw new ZipError('central directory out of range');
  // read at most what the capped number of entries can need
  const cd = await src.read(cdOff, cdOff + Math.min(cdSize, 16 * 1024 * 1024), signal);
  const entries: ZipEntry[] = [];
  let p = 0;
  while (p + 46 <= cd.length && entries.length < maxEntries) {
    if (u32(cd, p) !== 0x02014b50) break;
    const flags = u16(cd, p + 8);
    const method = u16(cd, p + 10);
    const time = u16(cd, p + 12);
    const date = u16(cd, p + 14);
    let compressed = u32(cd, p + 20);
    let usize = u32(cd, p + 24);
    const nlen = u16(cd, p + 28);
    const xlen = u16(cd, p + 30);
    const clen = u16(cd, p + 32);
    let offset = u32(cd, p + 42);
    if (p + 46 + nlen > cd.length) break;
    const name = decodeName(cd.subarray(p + 46, p + 46 + nlen), (flags & 0x800) !== 0);
    // ZIP64 extra field: the 0xffffffff fields, in order
    let x = p + 46 + nlen;
    const xend = x + xlen;
    while (x + 4 <= xend && x + 4 <= cd.length) {
      const id = u16(cd, x);
      const len = u16(cd, x + 2);
      if (id === 1) {
        let q = x + 4;
        if (usize === 0xffffffff) (usize = u64(cd, q)), (q += 8);
        if (compressed === 0xffffffff) (compressed = u64(cd, q)), (q += 8);
        if (offset === 0xffffffff) offset = u64(cd, q);
      }
      x += 4 + len;
    }
    entries.push({ name, dir: name.endsWith('/'), size: usize, compressed, method, encrypted: (flags & 1) !== 0, offset, mtime: dosTime(time, date) });
    p += 46 + nlen + xlen + clen;
  }
  return { entries, truncated: entries.length < total, total };
}

async function inflateRaw(data: Uint8Array, cap: number): Promise<Uint8Array> {
  if (typeof DecompressionStream === 'undefined') throw new ZipError('no DecompressionStream');
  const ds = new DecompressionStream('deflate-raw');
  const writer = ds.writable.getWriter();
  void writer.write(data as Uint8Array<ArrayBuffer>).catch(() => undefined);
  void writer.close().catch(() => undefined);
  const reader = ds.readable.getReader();
  const parts: Uint8Array[] = [];
  let n = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    n += value.length;
    if (n > cap) {
      void reader.cancel().catch(() => undefined);
      throw new ZipError('entry too large');
    }
  }
  const out = new Uint8Array(n);
  let o = 0;
  for (const part of parts) out.set(part, o), (o += part.length);
  return out;
}

/** One entry's bytes (stored or deflated), at most `cap` bytes uncompressed. */
export async function readEntry(src: ByteSource, e: ZipEntry, cap = 16 * 1024 * 1024, signal?: AbortSignal): Promise<Uint8Array> {
  if (e.encrypted) throw new ZipError('encrypted entry');
  if (e.size > cap || e.compressed > cap * 4) throw new ZipError('entry too large');
  const lh = await src.read(e.offset, e.offset + 30, signal);
  if (u32(lh, 0) !== 0x04034b50) throw new ZipError('bad local header');
  const start = e.offset + 30 + u16(lh, 26) + u16(lh, 28);
  const data = await src.read(start, start + e.compressed, signal);
  if (e.method === 0) return data;
  if (e.method === 8) return inflateRaw(data, cap);
  throw new ZipError(`method ${e.method}`);
}
