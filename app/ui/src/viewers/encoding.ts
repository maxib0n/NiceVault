export type TextEncodingName = 'utf-8' | 'utf-16le' | 'utf-16be' | 'windows-1252';

export interface Detected {
  encoding: TextEncodingName;
  bom: number;
  binary: boolean;
}

export function validUtf8(b: Uint8Array, truncated = true): boolean {
  let i = 0;
  const n = b.length;
  while (i < n) {
    const c = b[i];
    if (c < 0x80) {
      i++;
      continue;
    }
    let need: number;
    let min: number;
    if (c >= 0xc2 && c <= 0xdf) (need = 1), (min = 0x80);
    else if (c >= 0xe0 && c <= 0xef) (need = 2), (min = 0x800);
    else if (c >= 0xf0 && c <= 0xf4) (need = 3), (min = 0x10000);
    else return false;
    if (i + need >= n) {
      if (!truncated) return false;
      for (let k = i + 1; k < n; k++) if ((b[k] & 0xc0) !== 0x80) return false;
      return true;
    }
    let cp = c & (0x3f >> need);
    for (let k = 1; k <= need; k++) {
      const d = b[i + k];
      if ((d & 0xc0) !== 0x80) return false;
      cp = (cp << 6) | (d & 0x3f);
    }
    if (cp < min || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) return false;
    i += need + 1;
  }
  return true;
}

export function detectEncoding(sample: Uint8Array, truncated = true): Detected {
  const b = sample;
  if (b.length >= 3 && b[0] === 0xef && b[1] === 0xbb && b[2] === 0xbf) return { encoding: 'utf-8', bom: 3, binary: false };
  if (b.length >= 2 && b[0] === 0xff && b[1] === 0xfe) return { encoding: 'utf-16le', bom: 2, binary: false };
  if (b.length >= 2 && b[0] === 0xfe && b[1] === 0xff) return { encoding: 'utf-16be', bom: 2, binary: false };
  const n = Math.min(b.length, 64 * 1024);
  if (n === 0) return { encoding: 'utf-8', bom: 0, binary: false };
  let evenNul = 0;
  let oddNul = 0;
  let nul = 0;
  let ctrl = 0;
  for (let i = 0; i < n; i++) {
    const c = b[i];
    if (c === 0) {
      nul++;
      if (i % 2 === 0) evenNul++;
      else oddNul++;
    } else if (c < 0x20 && c !== 9 && c !== 10 && c !== 13 && c !== 12 && c !== 27) ctrl++;
  }
  const pairs = n / 2;
  if (pairs >= 2 && oddNul > pairs * 0.4 && evenNul < pairs * 0.05) return { encoding: 'utf-16le', bom: 0, binary: false };
  if (pairs >= 2 && evenNul > pairs * 0.4 && oddNul < pairs * 0.05) return { encoding: 'utf-16be', bom: 0, binary: false };
  if (nul > 0 || ctrl > n * 0.1) return { encoding: 'utf-8', bom: 0, binary: true };
  if (validUtf8(b.subarray(0, n), truncated || n < b.length)) return { encoding: 'utf-8', bom: 0, binary: false };
  return { encoding: 'windows-1252', bom: 0, binary: false };
}

export function encodingLabel(e: TextEncodingName): string {
  return { 'utf-8': 'UTF-8', 'utf-16le': 'UTF-16 LE', 'utf-16be': 'UTF-16 BE', 'windows-1252': 'Windows-1252' }[e];
}
