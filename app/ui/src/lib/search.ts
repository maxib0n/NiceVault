import type { FsNode, KindGroup, SearchFilters, TagColor, TagId } from './types';

export const KIND_GROUPS: readonly KindGroup[] = ['photos', 'videos', 'audio', 'documents', 'archives', 'code', 'other', 'folders'];

export const TAG_COLORS: readonly TagColor[] = ['velluto', 'prugna', 'prussia', 'bottiglia', 'seppia', 'ardesia', 'ottanio', 'neutral'];
export const MAX_TAG_NAME = 64;

export const tagColor = (c: string): TagColor => ((TAG_COLORS as readonly string[]).includes(c) ? (c as TagColor) : 'neutral');

const SPECIAL: Record<string, string> = { ß: 'ss', ẞ: 'ss', æ: 'ae', Æ: 'ae', œ: 'oe', Œ: 'oe', ø: 'o', Ø: 'o', đ: 'd', Đ: 'd', ł: 'l', Ł: 'l', þ: 'th', Þ: 'th', ı: 'i' };

function foldChar(c: string): string {
  const s = SPECIAL[c];
  if (s) return s;
  return c.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
}

export function fold(s: string): string {
  let out = '';
  for (const c of s) out += foldChar(c);
  return out;
}

function foldMapped(s: string): { norm: string; map: [number, number][] } {
  let norm = '';
  const map: [number, number][] = [];
  let pos = 0;
  for (const c of s) {
    const f = foldChar(c);
    norm += f;
    for (let i = 0; i < f.length; i++) map.push([pos, pos + c.length]);
    pos += c.length;
  }
  return { norm, map };
}

export function terms(text: string): string[] {
  const out: string[] = [];
  const push = (t: string) => {
    const f = fold(t.trim());
    if (f && !out.includes(f)) out.push(f);
  };
  const re = /"([^"]*)"?|([^\s"]+)/g;
  for (let m = re.exec(text); m; m = re.exec(text)) push(m[1] ?? m[2] ?? '');
  return out;
}

const GROUPS: [KindGroup, string][] = [
  ['photos', 'jpg jpeg jpe jfif png gif webp avif bmp svg heic heif tif tiff ico jxl raw cr2 cr3 nef arw dng orf rw2 raf psd xcf'],
  ['videos', 'mp4 m4v webm mov mkv avi wmv flv mpg mpeg 3gp ogv mts m2ts vob'],
  ['audio', 'mp3 ogg oga opus wav flac m4a aac wma aif aiff mid midi amr ape'],
  ['documents', 'pdf doc docx odt rtf pages txt md markdown tex log xls xlsx ods numbers csv tsv ppt pptx odp key epub mobi djvu xps'],
  ['archives', 'zip gz tgz tar 7z rar xz txz bz2 tbz2 zst lz4 lzma iso dmg cab jar apk deb rpm'],
  [
    'code',
    'json yaml yml toml xml html htm css scss sass less js mjs cjs jsx ts tsx rs py rb go java kt kts c h cpp hpp cc hh cs swift php sh bash zsh fish ps1 bat cmd sql lua pl r dart scala vue svelte ini conf cfg ipynb diff patch',
  ],
];
const EXT_GROUP = new Map<string, KindGroup>();
for (const [g, list] of GROUPS) for (const e of list.split(' ')) EXT_GROUP.set(e, g);

export function kindGroup(node: Pick<FsNode, 'name' | 'kind'>): KindGroup {
  if (node.kind === 'dir') return 'folders';
  const i = node.name.lastIndexOf('.');
  if (i <= 0 || i === node.name.length - 1) return 'other';
  const ext = node.name.slice(i + 1);
  if (ext.length > 8) return 'other';
  return EXT_GROUP.get(ext.replace(/[A-Z]/g, (c) => c.toLowerCase())) ?? 'other';
}

export function matchesFilters(node: Pick<FsNode, 'name' | 'kind' | 'size' | 'mtime'>, tags: readonly TagId[], f: SearchFilters | null | undefined): boolean {
  if (!f) return true;
  const g = kindGroup(node);
  if (f.kinds?.length && !f.kinds.includes(g)) return false;
  if (f.modified_after != null && node.mtime < f.modified_after) return false;
  if (f.modified_before != null && node.mtime >= f.modified_before) return false;
  if (f.min_size != null || f.max_size != null) {
    if (g === 'folders') return false;
    if (f.min_size != null && node.size < f.min_size) return false;
    if (f.max_size != null && node.size > f.max_size) return false;
  }
  if (f.tags?.length) {
    const has = (t: TagId) => tags.includes(t);
    if (f.tags_any ? !f.tags.some(has) : !f.tags.every(has)) return false;
  }
  return true;
}

export function rank(norm: string, ts: readonly string[]): number {
  const joined = ts.join(' ');
  const dot = norm.lastIndexOf('.');
  const stem = dot > 0 ? norm.slice(0, dot) : norm;
  if (norm === joined || stem === joined) return 0;
  if (norm.startsWith(ts[0])) return 1;
  const wordStart = (t: string) => {
    for (let i = norm.indexOf(t); i >= 0; i = norm.indexOf(t, i + 1)) {
      if (i === 0 || !/[\p{L}\p{N}]/u.test(norm[i - 1])) return true;
    }
    return false;
  };
  return ts.every(wordStart) ? 2 : 3;
}

export function highlights(name: string, ts: readonly string[]): [number, number][] {
  if (!ts.length) return [];
  const { norm, map } = foldMapped(name);
  const r: [number, number][] = [];
  for (const t of ts) {
    for (let i = norm.indexOf(t); i >= 0; i = norm.indexOf(t, i + t.length)) r.push([map[i][0], map[i + t.length - 1][1]]);
  }
  r.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const out: [number, number][] = [];
  for (const x of r) {
    const l = out[out.length - 1];
    if (l && x[0] <= l[1]) l[1] = Math.max(l[1], x[1]);
    else out.push([x[0], x[1]]);
  }
  return out;
}

export function highlightParts(name: string, ranges: readonly [number, number][]): { text: string; hit: boolean }[] {
  const out: { text: string; hit: boolean }[] = [];
  let at = 0;
  for (const [a, b] of ranges) {
    if (a > at) out.push({ text: name.slice(at, a), hit: false });
    if (b > a) out.push({ text: name.slice(a, b), hit: true });
    at = Math.max(at, b);
  }
  if (at < name.length) out.push({ text: name.slice(at), hit: false });
  return out;
}

export function naturalCmp(a: string, b: string): number {
  const x = [...a].map((c) => c.codePointAt(0) ?? 0);
  const y = [...b].map((c) => c.codePointAt(0) ?? 0);
  const digit = (c: number | undefined) => c !== undefined && c >= 48 && c <= 57;
  let i = 0;
  let j = 0;
  for (;;) {
    if (i >= x.length || j >= y.length) return i >= x.length ? (j >= y.length ? 0 : -1) : 1;
    if (digit(x[i]) && digit(y[j])) {
      let n = i;
      while (digit(x[n])) n++;
      let m = j;
      while (digit(y[m])) m++;
      let p = i;
      while (p < n - 1 && x[p] === 48) p++;
      let q = j;
      while (q < m - 1 && y[q] === 48) q++;
      if (n - p !== m - q) return n - p - (m - q);
      for (let k = 0; k < n - p; k++) if (x[p + k] !== y[q + k]) return x[p + k] - y[q + k];
      if (n - i !== m - j) return n - i - (m - j);
      i = n;
      j = m;
    } else {
      if (x[i] !== y[j]) return x[i] - y[j];
      i++;
      j++;
    }
  }
}
