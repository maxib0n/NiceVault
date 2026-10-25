// Which in-app viewer shows a file, and how much of it each one will take. Chosen by
// extension first, then — for names that say nothing — by the first bytes (sniff()).

import { extension } from '../lib/filetype';
import { languageFor } from './highlight';
import { OFFICE_EXTS } from './office';

export type ViewerKind = 'image' | 'video' | 'audio' | 'pdf' | 'markdown' | 'text' | 'office' | 'archive' | 'info';

const MB = 1024 * 1024;

/** Above these sizes a viewer shows the "too big" state instead (media stream, so none). */
export const CAPS: Record<ViewerKind, number> = {
  image: 200 * MB,
  video: Infinity,
  audio: Infinity,
  pdf: 1024 * MB,
  markdown: 4 * MB, // bigger Markdown is shown as paged text
  text: Infinity, // paged; at most TEXT_MAX_LOADED in memory
  office: 200 * MB,
  archive: Infinity, // only the directory is read
  info: Infinity,
};
/** the text viewer reads pages of this size, and stops after TEXT_MAX_LOADED */
export const TEXT_PAGE = 256 * 1024;
export const TEXT_MAX_LOADED = 16 * MB;
/** the hex view shows this much of the start */
export const HEX_BYTES = 16 * 1024;

const EXT: Record<string, ViewerKind> = {};
const reg = (k: ViewerKind, exts: string) => exts.split(' ').forEach((e) => (EXT[e] = k));
reg('image', 'jpg jpeg jfif png gif webp avif bmp svg heic heif tif tiff ico jxl');
reg('video', 'mp4 m4v webm mov mkv ogv');
reg('audio', 'mp3 ogg oga opus wav flac m4a aac');
reg('pdf', 'pdf');
reg('markdown', 'md markdown mdown mkd');
reg('text', 'txt text log csv tsv nfo srt vtt rst adoc tex bib pub pem asc key crt cer csr gpg');
reg('archive', 'zip jar apk epub cbz tar tgz gz xz bz2 zst 7z rar');
for (const e of OFFICE_EXTS) EXT[e] = 'office';

export function viewerKind(name: string): ViewerKind {
  const ext = extension(name);
  if (EXT[ext]) return EXT[ext];
  if (languageFor(name)) return 'text';
  if (!ext && /^id_[a-z0-9]+$/i.test(name)) return 'text'; // ssh keys
  return 'info';
}

/** The kind the bytes say, for files whose name gave none (null: keep the info view). */
export function sniff(head: Uint8Array): ViewerKind | null {
  const s = (o: number, sig: number[]) => sig.every((v, k) => head[o + k] === v);
  if (s(0, [0x25, 0x50, 0x44, 0x46, 0x2d])) return 'pdf';
  if (s(0, [0xff, 0xd8, 0xff]) || s(0, [0x89, 0x50, 0x4e, 0x47]) || s(0, [0x47, 0x49, 0x46, 0x38]) || (s(0, [0x52, 0x49, 0x46, 0x46]) && s(8, [0x57, 0x45, 0x42, 0x50]))) return 'image';
  if (s(4, [0x66, 0x74, 0x79, 0x70])) {
    const brand = String.fromCharCode(...head.subarray(8, 12));
    if (/^(avif|avis|heic|heix|mif1|msf1)$/.test(brand)) return 'image';
    if (/^M4A /.test(brand)) return 'audio';
    return 'video';
  }
  if (s(0, [0x1a, 0x45, 0xdf, 0xa3])) return 'video';
  if (s(0, [0x4f, 0x67, 0x67, 0x53]) || s(0, [0x49, 0x44, 0x33]) || s(0, [0x66, 0x4c, 0x61, 0x43])) return 'audio';
  if (s(0, [0x50, 0x4b, 0x03, 0x04]) || s(0, [0x50, 0x4b, 0x05, 0x06]) || s(0, [0x1f, 0x8b]) || s(0, [0x37, 0x7a, 0xbc, 0xaf]) || s(0, [0x52, 0x61, 0x72, 0x21])) return 'archive';
  if (head.length >= 262 && String.fromCharCode(...head.subarray(257, 262)) === 'ustar') return 'archive';
  return null;
}

/** A short description of what the first bytes are (info panel), or null. */
export function magicLabel(head: Uint8Array): string | null {
  const s = (o: number, sig: number[]) => sig.every((v, k) => head[o + k] === v);
  const table: [boolean, string][] = [
    [s(0, [0x25, 0x50, 0x44, 0x46]), 'PDF'],
    [s(0, [0xff, 0xd8, 0xff]), 'JPEG'],
    [s(0, [0x89, 0x50, 0x4e, 0x47]), 'PNG'],
    [s(0, [0x47, 0x49, 0x46, 0x38]), 'GIF'],
    [s(0, [0x52, 0x49, 0x46, 0x46]) && s(8, [0x57, 0x45, 0x42, 0x50]), 'WebP'],
    [s(0, [0x52, 0x49, 0x46, 0x46]) && s(8, [0x57, 0x41, 0x56, 0x45]), 'WAV'],
    [s(4, [0x66, 0x74, 0x79, 0x70]), 'ISO BMFF (MP4/MOV/HEIF)'],
    [s(0, [0x1a, 0x45, 0xdf, 0xa3]), 'Matroska / WebM'],
    [s(0, [0x4f, 0x67, 0x67, 0x53]), 'Ogg'],
    [s(0, [0x49, 0x44, 0x33]), 'MP3 (ID3)'],
    [s(0, [0x66, 0x4c, 0x61, 0x43]), 'FLAC'],
    [s(0, [0x50, 0x4b, 0x03, 0x04]), 'ZIP'],
    [s(0, [0x1f, 0x8b]), 'gzip'],
    [s(0, [0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c]), '7-Zip'],
    [s(0, [0x52, 0x61, 0x72, 0x21]), 'RAR'],
    [s(0, [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]), 'xz'],
    [s(0, [0x42, 0x5a, 0x68]), 'bzip2'],
    [s(0, [0x28, 0xb5, 0x2f, 0xfd]), 'Zstandard'],
    [s(0, [0x7f, 0x45, 0x4c, 0x46]), 'ELF'],
    [s(0, [0x4d, 0x5a]), 'Windows PE (EXE/DLL)'],
    [s(0, [0xcf, 0xfa, 0xed, 0xfe]) || s(0, [0xca, 0xfe, 0xba, 0xbe]), 'Mach-O'],
    [s(0, [0x53, 0x51, 0x4c, 0x69, 0x74, 0x65, 0x20, 0x66]), 'SQLite'],
    [s(0, [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]), 'OLE2 (Office 97–2003)'],
    [s(0, [0x7b, 0x5c, 0x72, 0x74, 0x66]), 'RTF'],
    [s(0, [0x00, 0x01, 0x00, 0x00, 0x00]) || s(0, [0x4f, 0x54, 0x54, 0x4f]) || s(0, [0x77, 0x4f, 0x46, 0x32]), 'Font (TTF/OTF/WOFF2)'],
  ];
  for (const [hit, label] of table) if (hit) return label;
  if (head.length >= 262 && String.fromCharCode(...head.subarray(257, 262)) === 'ustar') return 'tar';
  return null;
}

/** rows of 16 bytes: offset, hex, printable ASCII */
export function hexRows(b: Uint8Array, base = 0): { off: string; hex: string; ascii: string }[] {
  const rows: { off: string; hex: string; ascii: string }[] = [];
  for (let o = 0; o < b.length; o += 16) {
    const chunk = b.subarray(o, o + 16);
    let hex = '';
    let ascii = '';
    for (let k = 0; k < 16; k++) {
      if (k === 8) hex += ' ';
      if (k < chunk.length) {
        hex += chunk[k].toString(16).padStart(2, '0') + ' ';
        ascii += chunk[k] >= 0x20 && chunk[k] < 0x7f ? String.fromCharCode(chunk[k]) : '·';
      } else hex += '   ';
    }
    rows.push({ off: (base + o).toString(16).padStart(8, '0'), hex: hex.trimEnd(), ascii });
  }
  return rows;
}
