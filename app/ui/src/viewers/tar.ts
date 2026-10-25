// tar listing over ranged reads: hop from header to header (512-byte blocks), skipping file
// bodies, so a listing reads the headers only. ustar, GNU long names and pax paths.

import type { ByteSource } from './source';

export interface TarEntry {
  name: string;
  dir: boolean;
  size: number;
  mtime: number | null;
}

const WINDOW = 256 * 1024;

function str(b: Uint8Array, o: number, n: number): string {
  let end = o;
  while (end < o + n && b[end] !== 0) end++;
  return new TextDecoder('utf-8').decode(b.subarray(o, end));
}
function octal(b: Uint8Array, o: number, n: number): number {
  // GNU base-256 for big sizes
  if (b[o] & 0x80) {
    let v = 0;
    for (let k = o + 1; k < o + n; k++) v = v * 256 + b[k];
    return v;
  }
  const s = str(b, o, n).trim();
  return s ? parseInt(s, 8) || 0 : 0;
}

export function looksLikeTar(head: Uint8Array): boolean {
  if (head.length < 512) return false;
  if (str(head, 257, 5) === 'ustar') return true;
  // old v7 tar: the header checksum is right
  let sum = 0;
  for (let k = 0; k < 512; k++) sum += k >= 148 && k < 156 ? 32 : head[k];
  return sum === octal(head, 148, 8) && sum > 256;
}

export async function listTar(src: ByteSource, maxEntries = 5000, signal?: AbortSignal): Promise<{ entries: TarEntry[]; truncated: boolean }> {
  const size = await src.size();
  let winStart = -1;
  let win: Uint8Array = new Uint8Array(0);
  const block = async (off: number): Promise<Uint8Array | null> => {
    if (off + 512 > size) return null;
    if (winStart < 0 || off < winStart || off + 512 > winStart + win.length) {
      winStart = off;
      win = await src.read(off, Math.min(size, off + WINDOW), signal);
    }
    const b = win.subarray(off - winStart, off - winStart + 512);
    return b.length === 512 ? b : null;
  };
  const entries: TarEntry[] = [];
  let off = 0;
  let longName: string | null = null;
  let paxPath: string | null = null;
  let reads = 0;
  while (entries.length < maxEntries && reads++ < maxEntries * 3) {
    const h = await block(off);
    if (!h || h.every((x) => x === 0)) return { entries, truncated: false };
    const type = String.fromCharCode(h[156] || 48);
    const sz = octal(h, 124, 12);
    const body = off + 512;
    const next = body + Math.ceil(sz / 512) * 512;
    if (type === 'L' || type === 'x') {
      const data = await src.read(body, body + Math.min(sz, 64 * 1024), signal);
      if (type === 'L') longName = str(data, 0, data.length);
      else {
        const m = /(?:^|\n)\d+ path=([^\n]*)\n/.exec(new TextDecoder().decode(data));
        if (m) paxPath = m[1];
      }
      off = next;
      continue;
    }
    if (type === 'g') {
      off = next;
      continue;
    }
    let name = str(h, 0, 100);
    if (str(h, 257, 5) === 'ustar') {
      const prefix = str(h, 345, 155);
      if (prefix) name = `${prefix}/${name}`;
    }
    name = paxPath ?? longName ?? name;
    paxPath = longName = null;
    const mt = octal(h, 136, 12);
    entries.push({ name, dir: type === '5' || name.endsWith('/'), size: type === '5' ? 0 : sz, mtime: mt ? mt * 1000 : null });
    off = next;
  }
  return { entries, truncated: true };
}
