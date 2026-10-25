import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { detectEncoding, validUtf8 } from './encoding';
import { languageFor, toLines, tokenize } from './highlight';
import { parseInline, parseMarkdown, safeHref } from './markdown';
import { memorySource, urlSource } from './source';
import { listZip, readEntry } from './zip';
import { listTar, looksLikeTar } from './tar';
import { parseCsv } from './csv';
import { hexRows, magicLabel, sniff, viewerKind } from './kinds';

const sample = (n: string) => new Uint8Array(readFileSync(resolve(__dirname, '../../public/mock/v', n)));

describe('encoding', () => {
  it('BOMs, UTF-8, UTF-16 without BOM, legacy Windows-1252, binary', () => {
    expect(detectEncoding(new Uint8Array([0xef, 0xbb, 0xbf, 0x61]))).toMatchObject({ encoding: 'utf-8', bom: 3 });
    expect(detectEncoding(sample('lettera-utf16.txt'))).toMatchObject({ encoding: 'utf-16le', bom: 2 });
    expect(detectEncoding(new TextEncoder().encode('città è già'))).toMatchObject({ encoding: 'utf-8', binary: false });
    expect(detectEncoding(sample('lettera-1252.txt')).encoding).toBe('windows-1252');
    const le = new Uint8Array([0x63, 0, 0x69, 0, 0x61, 0, 0x6f, 0, 0x0a, 0]);
    expect(detectEncoding(le).encoding).toBe('utf-16le');
    expect(detectEncoding(sample('dati.bin')).binary).toBe(true);
  });
  it('a UTF-8 sequence cut by the sample end is still UTF-8; overlongs are not', () => {
    const b = new TextEncoder().encode('perché');
    expect(validUtf8(b.subarray(0, b.length - 1), true)).toBe(true);
    expect(validUtf8(b.subarray(0, b.length - 1), false)).toBe(false);
    expect(validUtf8(new Uint8Array([0xc0, 0xaf]))).toBe(false);
    expect(validUtf8(new Uint8Array([0xed, 0xa0, 0x80]))).toBe(false); // surrogate
  });
});

describe('highlight', () => {
  it('classes tokens and keeps every character', () => {
    const src = 'fn main() { let x = "a\\"b"; // c\n  42 }';
    const t = tokenize(src, 'rust');
    expect(t.map((x) => x.s).join('')).toBe(src);
    const cls = (s: string) => t.find((x) => x.s === s)?.c;
    expect(cls('fn')).toBe('kw');
    expect(cls('main')).toBe('fn');
    expect(cls('"a\\"b"')).toBe('str');
    expect(cls('// c')).toBe('com');
    expect(cls('42')).toBe('num');
    expect(toLines(t)).toHaveLength(2);
  });
  it('languages by name; plain text untouched', () => {
    expect(languageFor('a.ts')).toBe('js');
    expect(languageFor('Cargo.toml')).toBe('ini');
    expect(languageFor('Makefile')).toBe('shell');
    expect(languageFor('note.txt')).toBeNull();
    expect(tokenize('<b>x</b>', null)).toEqual([{ c: null, s: '<b>x</b>' }]);
    const html = tokenize('<a href="x">t</a><!-- c -->', 'markup');
    expect(html.map((x) => x.s).join('')).toBe('<a href="x">t</a><!-- c -->');
    expect(html.find((x) => x.s === 'href')?.c).toBe('attr');
  });
});

describe('markdown', () => {
  it('blocks', () => {
    const b = parseMarkdown(new TextDecoder().decode(sample('guida.md')));
    const kinds = b.map((x) => x.t);
    expect(kinds).toContain('h');
    expect(kinds).toContain('list');
    expect(kinds).toContain('code');
    expect(kinds).toContain('table');
    expect(kinds).toContain('quote');
    expect(kinds).toContain('hr');
    const tasks = b.filter((x) => x.t === 'list').flatMap((l) => (l.t === 'list' ? l.items.map((i) => i.task) : []));
    expect(tasks).toContain(true);
    expect(tasks).toContain(false);
  });
  it('raw HTML stays text, links only http(s)/mailto, images never fetched', () => {
    const txt = JSON.stringify(parseMarkdown('<script>alert(1)</script>\n\n[x](javascript:alert(1)) ![i](https://t.example/p.png)'));
    expect(txt).toContain('"t":"text","s":"<script>alert(1)</script>"');
    const p = parseInline('[x](javascript:alert(1)) [y](https://a.example/b) <mailto:a@b.it> ![i](https://t.example/p.png)');
    const links = p.filter((n) => n.t === 'link');
    expect(links.map((l) => (l.t === 'link' ? l.href : ''))).toEqual([null, 'https://a.example/b', 'mailto:a@b.it']);
    expect(p.find((n) => n.t === 'img')).toMatchObject({ alt: 'i' });
    expect(safeHref('file:///etc/passwd')).toBeNull();
    expect(safeHref('HTTPS://ok.example')).toBe('https://ok.example/');
  });
  it('links are normalised like viewers.rs checked_link', () => {
    expect(safeHref('mailto:a@b.it?subject=Ciao%20Mario&attach=/etc/passwd&body=x')).toBe('mailto:a@b.it?subject=Ciao%20Mario&body=x');
    expect(safeHref('mailto:a@b.it?Attachment=~/.ssh/id_ed25519')).toBe('mailto:a@b.it');
    expect(safeHref('mailto:a@b.it%3Fattach%3D%2Fetc%2Fpasswd')).toBeNull();
    expect(safeHref('mailto:a@b.it?body=%01')).toBeNull();
    expect(safeHref('https://example.org/a`id`')).toBeNull();
    expect(safeHref('https://example.org/$(id)')).toBeNull();
    expect(safeHref('https://user:pw@example.org/')).toBeNull();
    expect(safeHref('HTTPS://Example.ORG/x?y=1#f')).toBe('https://example.org/x?y=1#f');
  });
  it('emphasis, code spans, snake_case', () => {
    expect(parseInline('**b** *i* `c*d*` snake_case_name ~~s~~').map((n) => n.t)).toEqual(['strong', 'text', 'em', 'text', 'code', 'text', 'del']);
  });
});

describe('archives', () => {
  it('zip: central directory, names, sizes; deflated entry read back', async () => {
    const src = memorySource(sample('archivio.zip'));
    const z = await listZip(src);
    expect(z.entries.map((e) => e.name)).toContain('Documenti/ricevute/maggio.txt');
    expect(z.entries.map((e) => e.name)).toContain('note à accentate.txt');
    const e = z.entries.find((x) => x.name === 'Documenti/contratto.txt')!;
    expect(e.size).toBe('Contratto di locazione\n'.length * 40);
    expect(e.compressed).toBeLessThan(e.size);
    expect(new TextDecoder().decode(await readEntry(src, e))).toMatch(/^Contratto di locazione\n/);
    await expect(readEntry(src, e, 10)).rejects.toThrow();
  });
  it('tar: header hops', async () => {
    const b = sample('backup.tar');
    expect(looksLikeTar(b.subarray(0, 512))).toBe(true);
    const t = await listTar(memorySource(b));
    expect(t.entries.map((e) => [e.name, e.dir, e.size])).toEqual([
      ['backup/', true, 0],
      ['backup/contatti.vcf', false, 150],
      ['backup/foto/', true, 0],
      ['backup/foto/img_001.txt', false, 3000],
      ['backup/note.txt', false, 5],
    ]);
  });
});

describe('kinds, csv, hex', () => {
  it('by name, then by bytes', () => {
    expect(viewerKind('a.md')).toBe('markdown');
    expect(viewerKind('a.docx')).toBe('office');
    expect(viewerKind('a.rs')).toBe('text');
    expect(viewerKind('a.tar.gz')).toBe('archive');
    expect(viewerKind('id_ed25519')).toBe('text');
    expect(viewerKind('a.dat')).toBe('info');
    expect(sniff(new TextEncoder().encode('%PDF-1.7'))).toBe('pdf');
    expect(sniff(sample('archivio.zip'))).toBe('archive');
    expect(sniff(sample('backup.tar'))).toBe('archive');
    expect(sniff(sample('dati.bin'))).toBeNull();
    expect(magicLabel(sample('scatto.heic'))).toMatch(/HEIF/);
  });
  it('csv: quotes, doubled quotes, ; delimiter', () => {
    const r = parseCsv(new TextDecoder().decode(sample('spese.csv')));
    expect(r.rows[0]).toEqual(['Data', 'Descrizione', 'Categoria', 'Importo']);
    expect(r.rows[2][1]).toBe('Spesa, supermercato');
    expect(r.rows[4][1]).toBe('Cena da "Gigi"');
    expect(r.rows).toHaveLength(6);
  });
  it('hex rows', () => {
    const r = hexRows(new Uint8Array([0x41, 0x00, 0xff]), 16);
    expect(r).toEqual([{ off: '00000010', hex: '41 00 ff', ascii: 'A··' }]);
  });
});

describe('urlSource', () => {
  it('ranges, and a server that ignores Range', async () => {
    const data = new Uint8Array(1000).map((_, i) => i % 256);
    let calls = 0;
    const honour = (async (_u: string, init?: RequestInit) => {
      calls++;
      const [a, b] = /bytes=(\d+)-(\d+)/.exec(String((init?.headers as Record<string, string>).Range))!.slice(1).map(Number);
      const end = Math.min(b, data.length - 1);
      return new Response(data.slice(a, end + 1), { status: 206, headers: { 'content-range': `bytes ${a}-${end}/${data.length}` } });
    }) as typeof fetch;
    const real = globalThis.fetch;
    try {
      globalThis.fetch = honour;
      const s = urlSource('x');
      expect(await s.size()).toBe(1000);
      expect(Array.from(await s.read(10, 14))).toEqual([10, 11, 12, 13]);
      expect((await s.read(990, 2000)).length).toBe(10);
      expect(calls).toBe(3);
      globalThis.fetch = (async () => new Response(data, { status: 200, headers: { 'content-length': '1000' } })) as typeof fetch;
      const w = urlSource('y');
      expect(await w.size()).toBe(1000);
      expect(Array.from(await w.read(300, 302))).toEqual([44, 45]);
    } finally {
      globalThis.fetch = real;
    }
  });
});
