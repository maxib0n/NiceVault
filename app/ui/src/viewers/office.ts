import type { ByteSource } from './source';
import { listZip, readEntry, type ZipEntry } from './zip';

export interface OfficeMeta {
  title?: string;
  author?: string;
  modified?: string;
  app?: string;
}
export type OfficeBody =
  | { kind: 'text'; blocks: { level: number; s: string }[] }
  | { kind: 'slides'; slides: string[][] }
  | { kind: 'sheet'; sheet: string; sheets: number; rows: string[][] };
export interface OfficeDoc {
  meta: OfficeMeta;
  body: OfficeBody;
  truncated: boolean;
}

const MAX_BLOCKS = 4000;
const MAX_ROWS = 500;
const MAX_COLS = 40;
const MAX_SLIDES = 200;
const PART_CAP = 24 * 1024 * 1024;

export const OFFICE_EXTS = new Set(['docx', 'docm', 'dotx', 'xlsx', 'xlsm', 'pptx', 'pptm', 'odt', 'ott', 'ods', 'odp']);

function xml(bytes: Uint8Array): Document {
  const doc = new DOMParser().parseFromString(new TextDecoder('utf-8').decode(bytes), 'application/xml');
  if (doc.getElementsByTagName('parsererror').length) throw new Error('bad xml');
  return doc;
}
const all = (n: Document | Element, local: string) => Array.from(n.getElementsByTagNameNS('*', local));
const first = (n: Document | Element, local: string) => n.getElementsByTagNameNS('*', local)[0] as Element | undefined;
const text = (n: Element | undefined) => n?.textContent?.trim() || undefined;

function ooxmlPara(p: Element): string {
  let s = '';
  const walk = (n: Element) => {
    for (const c of Array.from(n.children)) {
      const l = c.localName;
      if (l === 't') s += c.textContent ?? '';
      else if (l === 'tab') s += '\t';
      else if (l === 'br' || l === 'cr') s += '\n';
      else if (l !== 'pPr' && l !== 'rPr' && l !== 'instrText' && l !== 'delText') walk(c);
    }
  };
  walk(p);
  return s;
}

function odfPara(p: Element): string {
  let s = '';
  const walk = (n: Node) => {
    for (const c of Array.from(n.childNodes)) {
      if (c.nodeType === 3) s += c.nodeValue ?? '';
      else if (c.nodeType === 1) {
        const e = c as Element;
        if (e.localName === 's') s += ' '.repeat(Number(e.getAttributeNS('*', 'c') || e.getAttribute('text:c') || 1));
        else if (e.localName === 'tab') s += '\t';
        else if (e.localName === 'line-break') s += '\n';
        else if (e.localName !== 'note' && e.localName !== 'annotation') walk(e);
      }
    }
  };
  walk(p);
  return s;
}

function colIndex(ref: string): number {
  const m = /^[A-Z]+/.exec(ref);
  if (!m) return -1;
  let n = 0;
  for (const ch of m[0]) n = n * 26 + (ch.charCodeAt(0) - 64);
  return n - 1;
}

export async function readOffice(src: ByteSource, ext: string, signal?: AbortSignal): Promise<OfficeDoc> {
  const { entries } = await listZip(src, 20000, signal);
  const byName = new Map(entries.map((e) => [e.name, e]));
  const part = async (name: string): Promise<Document | null> => {
    const e = byName.get(name);
    return e ? xml(await readEntry(src, e, PART_CAP, signal)) : null;
  };
  const meta: OfficeMeta = {};
  let truncated = false;
  const odf = ext.startsWith('od') || ext === 'ott';

  if (odf) {
    const m = await part('meta.xml').catch(() => null);
    if (m) {
      meta.title = text(first(m, 'title'));
      meta.author = text(first(m, 'initial-creator')) ?? text(first(m, 'creator'));
      meta.modified = text(first(m, 'date'));
      meta.app = text(first(m, 'generator'));
    }
    const c = await part('content.xml');
    if (!c) throw new Error('no content.xml');
    const bodyEl = first(c, 'body');
    if (ext === 'ods') {
      const tables = all(c, 'table');
      const t = tables[0];
      const rows: string[][] = [];
      for (const r of t ? all(t, 'table-row') : []) {
        if (rows.length >= MAX_ROWS) {
          truncated = true;
          break;
        }
        const reps = Math.min(Number(r.getAttribute('table:number-rows-repeated') || 1), 50);
        const cells: string[] = [];
        for (const cell of Array.from(r.children).filter((x) => x.localName === 'table-cell' || x.localName === 'covered-table-cell')) {
          const n = Math.min(Number(cell.getAttribute('table:number-columns-repeated') || 1), MAX_COLS);
          const v = all(cell, 'p').map(odfPara).join('\n');
          for (let k = 0; k < n && cells.length < MAX_COLS; k++) cells.push(v);
        }
        while (cells.length && !cells[cells.length - 1]) cells.pop();
        for (let k = 0; k < reps && rows.length < MAX_ROWS; k++) rows.push(cells);
      }
      while (rows.length && rows[rows.length - 1].length === 0) rows.pop();
      return { meta, truncated, body: { kind: 'sheet', sheet: t?.getAttribute('table:name') ?? '', sheets: tables.length, rows } };
    }
    if (ext === 'odp') {
      const slides = all(c, 'page')
        .slice(0, MAX_SLIDES)
        .map((pg) => all(pg, 'p').map(odfPara).filter((s) => s.trim()));
      return { meta, truncated: all(c, 'page').length > MAX_SLIDES, body: { kind: 'slides', slides } };
    }
    const blocks: { level: number; s: string }[] = [];
    const walk = (n: Element) => {
      for (const e of Array.from(n.children)) {
        if (blocks.length >= MAX_BLOCKS) {
          truncated = true;
          return;
        }
        if (e.localName === 'h') blocks.push({ level: Math.min(Number(e.getAttribute('text:outline-level') || 1), 6), s: odfPara(e) });
        else if (e.localName === 'p') blocks.push({ level: 0, s: odfPara(e) });
        else if (e.localName !== 'tracked-changes' && e.localName !== 'sequence-decls') walk(e);
      }
    };
    if (bodyEl) walk(bodyEl);
    return { meta, truncated, body: { kind: 'text', blocks } };
  }

  const core = await part('docProps/core.xml').catch(() => null);
  if (core) {
    meta.title = text(first(core, 'title'));
    meta.author = text(first(core, 'creator'));
    meta.modified = text(first(core, 'modified'));
  }
  const appx = await part('docProps/app.xml').catch(() => null);
  if (appx) meta.app = text(first(appx, 'Application'));

  if (ext.startsWith('xls')) {
    const ss = await part('xl/sharedStrings.xml').catch(() => null);
    const shared = ss ? all(ss, 'si').map((si) => all(si, 't').map((t) => t.textContent ?? '').join('')) : [];
    const wb = await part('xl/workbook.xml').catch(() => null);
    const sheetEls = wb ? all(wb, 'sheet') : [];
    const sheetName = sheetEls[0]?.getAttribute('name') ?? '';
    const ws = entries.filter((e) => /^xl\/worksheets\/sheet\d+\.xml$/.test(e.name)).sort((a, b) => num(a) - num(b))[0];
    const rows: string[][] = [];
    if (ws) {
      const d = xml(await readEntry(src, ws, PART_CAP, signal));
      for (const r of all(d, 'row')) {
        if (rows.length >= MAX_ROWS) {
          truncated = true;
          break;
        }
        const ri = Number(r.getAttribute('r') || rows.length + 1) - 1;
        while (rows.length < ri && rows.length < MAX_ROWS) rows.push([]);
        const cells: string[] = [];
        for (const c of all(r, 'c')) {
          const ci = colIndex(c.getAttribute('r') ?? '');
          if (ci >= MAX_COLS) continue;
          const t = c.getAttribute('t');
          const v = first(c, 'v')?.textContent ?? '';
          const val = t === 's' ? (shared[Number(v)] ?? '') : t === 'inlineStr' ? all(c, 't').map((x) => x.textContent ?? '').join('') : t === 'b' ? (v === '1' ? 'TRUE' : 'FALSE') : v;
          const at = ci >= 0 ? ci : cells.length;
          while (cells.length < at) cells.push('');
          cells[at] = val;
        }
        rows.push(cells);
      }
    }
    return { meta, truncated, body: { kind: 'sheet', sheet: sheetName, sheets: sheetEls.length, rows } };
  }

  if (ext.startsWith('ppt')) {
    const slideEntries = entries.filter((e) => /^ppt\/slides\/slide\d+\.xml$/.test(e.name)).sort((a, b) => num(a) - num(b));
    const slides: string[][] = [];
    for (const e of slideEntries.slice(0, MAX_SLIDES)) {
      const d = xml(await readEntry(src, e, PART_CAP, signal));
      slides.push(all(d, 'p').map(ooxmlPara).filter((s) => s.trim()));
    }
    return { meta, truncated: slideEntries.length > MAX_SLIDES, body: { kind: 'slides', slides } };
  }

  const d = await part('word/document.xml');
  if (!d) throw new Error('no word/document.xml');
  const blocks: { level: number; s: string }[] = [];
  const bodyEl = first(d, 'body');
  for (const p of bodyEl ? all(bodyEl, 'p') : []) {
    if (blocks.length >= MAX_BLOCKS) {
      truncated = true;
      break;
    }
    if (p.parentElement?.closest('p') && p.parentElement.closest('p') !== p) continue;
    const style = first(p, 'pStyle')?.getAttributeNS('*', 'val') ?? first(p, 'pStyle')?.getAttribute('w:val') ?? '';
    const hm = /(?:heading|titolo|berschrift|titre)\s*(\d)/i.exec(style) ?? (/^title$/i.test(style) ? ['', '1'] : null);
    blocks.push({ level: hm ? Math.min(Number(hm[1]) || 1, 6) : 0, s: ooxmlPara(p) });
  }
  return { meta, truncated, body: { kind: 'text', blocks } };
}

function num(e: ZipEntry): number {
  return Number(/(\d+)\.xml$/.exec(e.name)?.[1] ?? 0);
}
