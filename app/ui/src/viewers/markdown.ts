// A small CommonMark-ish Markdown parser for the in-app reader. It produces a plain tree
// that MarkdownViewer renders with Svelte elements and text nodes, so nothing in a file can
// become HTML: raw HTML tags are shown as text, images are never fetched (their alt text is
// shown instead), and only http(s)/mailto links survive, opened outside the app on an
// explicit, confirmed click.

export type Inline =
  | { t: 'text'; s: string }
  | { t: 'code'; s: string }
  | { t: 'em' | 'strong' | 'del'; c: Inline[] }
  | { t: 'link'; href: string | null; c: Inline[] }
  | { t: 'img'; alt: string; src: string }
  | { t: 'br' };

export type Block =
  | { t: 'h'; level: 1 | 2 | 3 | 4 | 5 | 6; c: Inline[] }
  | { t: 'p'; c: Inline[] }
  | { t: 'code'; lang: string; s: string }
  | { t: 'quote'; c: Block[] }
  | { t: 'list'; ordered: boolean; start: number; items: { task: boolean | null; c: Block[] }[] }
  | { t: 'hr' }
  | { t: 'table'; align: ('left' | 'center' | 'right' | null)[]; head: Inline[][]; rows: Inline[][][] };

const MAIL_KEYS = new Set(['subject', 'body', 'cc', 'bcc']);
// eslint-disable-next-line no-control-regex
const MAIL_CTRL = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/;

/**
 * A link the reader may open (outside the app). Anything else is shown as text. Same rules
 * as the backend (viewers.rs checked_link): http(s) without credentials; mailto
 * with the address and subject/body/cc/bcc only (no `attach=`); a backtick or `$` refuses it.
 */
export function safeHref(raw: string): string | null {
  const h = raw.trim().replace(/^<|>$/g, '');
  if (!/^(https?:\/\/|mailto:)/i.test(h) || /[\s"<>\\`$]/.test(h) || h.length > 2048) return null;
  let u: URL;
  try {
    u = new URL(h);
  } catch {
    return null;
  }
  if (u.protocol === 'http:' || u.protocol === 'https:') return u.username || u.password || !u.hostname ? null : u.href;
  if (u.protocol !== 'mailto:' || !/^[A-Za-z0-9@.\-_+,!#'*^{}~]*$/.test(u.pathname)) return null;
  const q: string[] = [];
  for (const part of u.search.slice(1).split('&')) {
    const i = part.indexOf('=');
    const k = (i < 0 ? part : part.slice(0, i)).toLowerCase();
    if (!MAIL_KEYS.has(k)) continue;
    let v: string;
    try {
      v = decodeURIComponent(i < 0 ? '' : part.slice(i + 1));
    } catch {
      return null;
    }
    if (MAIL_CTRL.test(v)) return null;
    q.push(`${k}=${encodeURIComponent(v)}`);
  }
  return `mailto:${u.pathname}${q.length ? '?' + q.join('&') : ''}`;
}

// ---------------------------------------------------------------- inlines

const PUNCT = /[!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~]/;

function findClose(s: string, from: number, delim: string): number {
  for (let i = from; i <= s.length - delim.length; i++) {
    const ch = s[i];
    if (ch === '\\') {
      i++;
      continue;
    }
    if (ch === '`') {
      // skip code spans
      const run = /^`+/.exec(s.slice(i))![0];
      const end = s.indexOf(run, i + run.length);
      if (end > 0) i = end + run.length - 1;
      continue;
    }
    if (s.startsWith(delim, i) && !/\s/.test(s[i - 1] ?? ' ')) {
      // `*a**` — for single `*`, a following `*` belongs to a longer run
      if (delim.length === 1 && s[i + 1] === delim) {
        i++;
        continue;
      }
      return i;
    }
  }
  return -1;
}

/** `[text](dest)`: index of the matching `]` and the destination, or null */
function linkAt(s: string, i: number): { textEnd: number; dest: string; end: number } | null {
  let depth = 0;
  for (let k = i; k < s.length; k++) {
    const ch = s[k];
    if (ch === '\\') {
      k++;
      continue;
    }
    if (ch === '[') depth++;
    else if (ch === ']') {
      depth--;
      if (depth === 0) {
        if (s[k + 1] !== '(') return null;
        const m = /^\(\s*(<[^>\n]*>|[^\s()]*(?:\([^\s()]*\)[^\s()]*)*)(?:\s+("[^"]*"|'[^']*'|\([^)]*\)))?\s*\)/.exec(s.slice(k + 1));
        if (!m) return null;
        return { textEnd: k, dest: m[1], end: k + 1 + m[0].length };
      }
    }
  }
  return null;
}

export function parseInline(s: string, depth = 0): Inline[] {
  const out: Inline[] = [];
  let buf = '';
  const flush = () => {
    if (buf) out.push({ t: 'text', s: buf });
    buf = '';
  };
  let i = 0;
  while (i < s.length) {
    const ch = s[i];
    if (ch === '\\' && i + 1 < s.length) {
      if (s[i + 1] === '\n') {
        flush();
        out.push({ t: 'br' });
        i += 2;
        continue;
      }
      if (PUNCT.test(s[i + 1])) {
        buf += s[i + 1];
        i += 2;
        continue;
      }
    }
    if (ch === '\n') {
      if (buf.endsWith('  ')) {
        buf = buf.replace(/ +$/, '');
        flush();
        out.push({ t: 'br' });
      } else buf = buf.replace(/ +$/, '') + ' ';
      i++;
      continue;
    }
    if (ch === '`') {
      const run = /^`+/.exec(s.slice(i))![0];
      const end = s.indexOf(run, i + run.length);
      if (end > 0 && s[end + run.length] !== '`') {
        flush();
        let code = s.slice(i + run.length, end).replace(/\n/g, ' ');
        if (/^ .*[^ ].* $/.test(code)) code = code.slice(1, -1);
        out.push({ t: 'code', s: code });
        i = end + run.length;
        continue;
      }
      buf += run;
      i += run.length;
      continue;
    }
    if (depth < 8 && (ch === '[' || (ch === '!' && s[i + 1] === '['))) {
      const img = ch === '!';
      const l = linkAt(s, img ? i + 1 : i);
      if (l) {
        flush();
        const text = s.slice(img ? i + 2 : i + 1, l.textEnd);
        if (img) out.push({ t: 'img', alt: text.replace(/[\\*_`[\]]/g, ''), src: l.dest.replace(/^<|>$/g, '') });
        else out.push({ t: 'link', href: safeHref(l.dest), c: parseInline(text, depth + 1) });
        i = l.end;
        continue;
      }
    }
    if (ch === '<') {
      const m = /^<((?:https?:\/\/|mailto:)[^\s<>]+)>/i.exec(s.slice(i));
      if (m) {
        flush();
        out.push({ t: 'link', href: safeHref(m[1]), c: [{ t: 'text', s: m[1] }] });
        i += m[0].length;
        continue;
      }
    }
    if ((ch === 'h' || ch === 'H') && !/\w/.test(s[i - 1] ?? ' ')) {
      const m = /^https?:\/\/[^\s<>]*[^\s<>.,:;"')\]!?*_~]/i.exec(s.slice(i));
      if (m) {
        flush();
        out.push({ t: 'link', href: safeHref(m[0]), c: [{ t: 'text', s: m[0] }] });
        i += m[0].length;
        continue;
      }
    }
    if (depth < 8 && (ch === '*' || ch === '_' || ch === '~')) {
      const dbl = s[i + 1] === ch;
      const delim = ch === '~' ? (dbl ? '~~' : '') : dbl ? ch + ch : ch;
      const opensWord = !/\s/.test(s[i + delim.length] ?? ' ');
      // `_` inside a word (snake_case) is not emphasis
      const intraword = ch === '_' && /\w/.test(s[i - 1] ?? ' ');
      if (delim && opensWord && !intraword) {
        const close = findClose(s, i + delim.length, delim);
        if (close > i + delim.length && !(ch === '_' && /\w/.test(s[close + delim.length] ?? ' '))) {
          flush();
          const inner = parseInline(s.slice(i + delim.length, close), depth + 1);
          out.push({ t: ch === '~' ? 'del' : delim.length === 2 ? 'strong' : 'em', c: inner });
          i = close + delim.length;
          continue;
        }
      }
      buf += delim || ch;
      i += (delim || ch).length;
      continue;
    }
    buf += ch;
    i++;
  }
  flush();
  return out;
}

// ---------------------------------------------------------------- blocks

const RE_FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^`\s]*)[^`]*$/;
const RE_ATX = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const RE_HR = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const RE_LIST = /^( {0,3})([-*+]|\d{1,9}[.)])([ \t]+|$)/;
const RE_QUOTE = /^ {0,3}> ?/;
const RE_TABLE_SEP = /^ *\|? *:?-+:? *(?:\| *:?-+:? *)*\|? *$/;

function splitRow(line: string): string[] {
  let l = line.trim();
  if (l.startsWith('|')) l = l.slice(1);
  if (l.endsWith('|') && !l.endsWith('\\|')) l = l.slice(0, -1);
  const cells: string[] = [];
  let cur = '';
  for (let i = 0; i < l.length; i++) {
    if (l[i] === '\\' && l[i + 1] === '|') {
      cur += '|';
      i++;
    } else if (l[i] === '|') {
      cells.push(cur.trim());
      cur = '';
    } else cur += l[i];
  }
  cells.push(cur.trim());
  return cells;
}

function startsBlock(line: string): boolean {
  return RE_FENCE.test(line) || RE_ATX.test(line) || RE_HR.test(line) || RE_QUOTE.test(line) || RE_LIST.test(line);
}

export function parseMarkdown(src: string, depth = 0): Block[] {
  const lines = src.replace(/\r\n?/g, '\n').replace(/\t/g, '    ').split('\n');
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (/^\s*$/.test(line)) {
      i++;
      continue;
    }
    let m: RegExpExecArray | null;
    if ((m = RE_FENCE.exec(line))) {
      const fence = m[1];
      const indent = /^ */.exec(line)![0].length;
      const body: string[] = [];
      i++;
      while (i < lines.length && !new RegExp(`^ {0,3}${fence[0]}{${fence.length},}\\s*$`).test(lines[i])) {
        body.push(lines[i].replace(new RegExp(`^ {0,${indent}}`), ''));
        i++;
      }
      i++;
      out.push({ t: 'code', lang: m[2].toLowerCase(), s: body.join('\n') });
      continue;
    }
    if ((m = RE_ATX.exec(line))) {
      out.push({ t: 'h', level: m[1].length as 1, c: parseInline(m[2] ?? '') });
      i++;
      continue;
    }
    if (RE_HR.test(line)) {
      out.push({ t: 'hr' });
      i++;
      continue;
    }
    if (/^ {4}/.test(line)) {
      const body: string[] = [];
      while (i < lines.length && (/^ {4}/.test(lines[i]) || /^\s*$/.test(lines[i]))) body.push(lines[i++].slice(4));
      while (body.length && /^\s*$/.test(body[body.length - 1])) body.pop();
      out.push({ t: 'code', lang: '', s: body.join('\n') });
      continue;
    }
    if (RE_QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && !/^\s*$/.test(lines[i]) && (RE_QUOTE.test(lines[i]) || body.length)) {
        if (!RE_QUOTE.test(lines[i]) && startsBlock(lines[i])) break;
        body.push(lines[i].replace(RE_QUOTE, ''));
        i++;
      }
      out.push({ t: 'quote', c: depth < 12 ? parseMarkdown(body.join('\n'), depth + 1) : [{ t: 'p', c: [{ t: 'text', s: body.join(' ') }] }] });
      continue;
    }
    if ((m = RE_LIST.exec(line))) {
      const ordered = /\d/.test(m[2]);
      const marker = ordered ? m[2].slice(-1) : m[2];
      const start = ordered ? parseInt(m[2], 10) : 1;
      const items: { task: boolean | null; c: Block[] }[] = [];
      while (i < lines.length) {
        const lm = RE_LIST.exec(lines[i]);
        if (!lm || /\d/.test(lm[2]) !== ordered || (ordered ? lm[2].slice(-1) : lm[2]) !== marker) break;
        const contentIndent = lm[0].length || lm[1].length + lm[2].length + 1;
        const body = [lines[i].slice(lm[0].length)];
        i++;
        let blank = false;
        while (i < lines.length) {
          const l = lines[i];
          if (/^\s*$/.test(l)) {
            blank = true;
            body.push('');
            i++;
            continue;
          }
          const ind = /^ */.exec(l)![0].length;
          if (ind >= Math.min(contentIndent, 4)) {
            body.push(l.slice(Math.min(ind, contentIndent)));
            blank = false;
            i++;
            continue;
          }
          if (blank || RE_LIST.test(l) || startsBlock(l)) break;
          body.push(l); // lazy continuation
          i++;
        }
        while (body.length && body[body.length - 1] === '') body.pop();
        let text = body.join('\n');
        let task: boolean | null = null;
        const tm = /^\[([ xX])\][ \t]+/.exec(text);
        if (tm) {
          task = tm[1] !== ' ';
          text = text.slice(tm[0].length);
        }
        items.push({ task, c: depth < 12 ? parseMarkdown(text, depth + 1) : [{ t: 'p', c: [{ t: 'text', s: text }] }] });
        if (blank && i < lines.length && !RE_LIST.test(lines[i])) break;
      }
      out.push({ t: 'list', ordered, start, items });
      continue;
    }
    // table: a header row, then a separator row
    if (line.includes('|') && i + 1 < lines.length && RE_TABLE_SEP.test(lines[i + 1]) && lines[i + 1].includes('-')) {
      const head = splitRow(line);
      const align = splitRow(lines[i + 1]).map((c) => (c.startsWith(':') && c.endsWith(':') ? 'center' : c.endsWith(':') ? 'right' : c.startsWith(':') ? 'left' : null));
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && lines[i].includes('|') && !/^\s*$/.test(lines[i])) {
        const cells = splitRow(lines[i]);
        rows.push(head.map((_, k) => parseInline(cells[k] ?? '')));
        i++;
      }
      out.push({ t: 'table', align, head: head.map((c) => parseInline(c)), rows });
      continue;
    }
    // paragraph (with setext heading underline)
    const para: string[] = [line.trim()];
    i++;
    let setext: 1 | 2 | 0 = 0;
    while (i < lines.length && !/^\s*$/.test(lines[i])) {
      if (/^ {0,3}=+\s*$/.test(lines[i])) {
        setext = 1;
        i++;
        break;
      }
      if (/^ {0,3}-+\s*$/.test(lines[i])) {
        setext = 2;
        i++;
        break;
      }
      if (startsBlock(lines[i])) break;
      para.push(lines[i].replace(/^ +/, ''));
      i++;
    }
    const text = para.join('\n');
    out.push(setext ? { t: 'h', level: setext, c: parseInline(text) } : { t: 'p', c: parseInline(text) });
  }
  return out;
}
