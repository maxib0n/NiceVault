import MarkdownItCtor, { type MarkdownIt, type StateBlock, type StateInline, type Token } from 'markdown-it';
import footnote from 'markdown-it-footnote';
import deflist from 'markdown-it-deflist';
import { full as emoji } from 'markdown-it-emoji';
import sub from 'markdown-it-sub';
import sup from 'markdown-it-sup';
import mark from 'markdown-it-mark';
import abbr from 'markdown-it-abbr';
import anchor from 'markdown-it-anchor';
import DOMPurify from 'dompurify';
import { languageFor, tokenize } from './highlight';

export { safeHref, localImagePath, resolveVolumePath } from './links';
import { safeHref, localImagePath } from './links';

export interface TocItem {
  level: number;
  id: string;
  text: string;
}

export interface Rendered {
  html: string;
  toc: TocItem[];
  front: [string, string][] | null;
  math: boolean;
  mermaid: boolean;
}

export interface Labels {
  note: string;
  tip: string;
  important: string;
  warning: string;
  caution: string;
  done: string;
  todo: string;
  image: string;
  imageBlocked: string;
}

const EN: Labels = {
  note: 'Note',
  tip: 'Tip',
  important: 'Important',
  warning: 'Warning',
  caution: 'Caution',
  done: 'Done',
  todo: 'To do',
  image: 'Image',
  imageBlocked: 'not loaded',
};

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

export function splitFrontMatter(src: string): { front: [string, string][] | null; body: string } {
  const m = /^---[ \t]*\n([\s\S]*?)\n(?:---|\.\.\.)[ \t]*(?:\n|$)/.exec(src);
  if (!m) return { front: null, body: src };
  const pairs: [string, string][] = [];
  for (const line of m[1].split('\n')) {
    if (!line.trim() || /^\s*#/.test(line)) continue;
    const kv = /^([^\s:#][^:]*):[ \t]*(.*)$/.exec(line);
    if (kv && !/^\s/.test(line)) pairs.push([kv[1].trim(), kv[2].trim().replace(/^(["'])(.*)\1$/, '$2')]);
    else if (pairs.length) {
      const last = pairs[pairs.length - 1];
      const item = line.trim().replace(/^-\s+/, '');
      last[1] = last[1] && last[1] !== '|' && last[1] !== '>' ? `${last[1]}, ${item}` : item;
    }
  }
  return { front: pairs.length ? pairs.slice(0, 40) : null, body: '\n'.repeat(m[0].split('\n').length - 1) + src.slice(m[0].length) };
}

function mathInline(state: StateInline, silent: boolean): boolean {
  const s = state.src;
  const start = state.pos;
  if (s.charCodeAt(start) !== 0x24 || s.charCodeAt(start + 1) === 0x24) return false;
  const first = s[start + 1];
  if (!first || /\s/.test(first)) return false;
  let end = start + 1;
  for (;;) {
    end = s.indexOf('$', end);
    if (end < 0) return false;
    if (s[end - 1] === '\\') {
      end++;
      continue;
    }
    break;
  }
  if (/\s/.test(s[end - 1]) || /\d/.test(s[end + 1] ?? '')) return false;
  if (!silent) {
    const t = state.push('math_inline', 'span', 0);
    t.content = s.slice(start + 1, end);
    t.markup = '$';
  }
  state.pos = end + 1;
  return true;
}

function mathBlock(state: StateBlock, startLine: number, endLine: number, silent: boolean): boolean {
  let pos = state.bMarks[startLine] + state.tShift[startLine];
  let max = state.eMarks[startLine];
  if (state.sCount[startLine] - state.blkIndent >= 4) return false;
  if (!state.src.startsWith('$$', pos)) return false;
  pos += 2;
  let first = state.src.slice(pos, max);
  let last = '';
  let next = startLine;
  let found = false;
  if (first.trim().endsWith('$$')) {
    first = first.trim().slice(0, -2);
    found = true;
  }
  if (silent) return true;
  while (!found) {
    next++;
    if (next >= endLine) break;
    pos = state.bMarks[next] + state.tShift[next];
    max = state.eMarks[next];
    if (pos < max && state.sCount[next] < state.blkIndent) break;
    const line = state.src.slice(pos, max);
    if (line.trim().endsWith('$$')) {
      last = line.trim().slice(0, -2);
      found = true;
    }
  }
  if (!found) return false;
  state.line = next + 1;
  const t = state.push('math_block', 'div', 0);
  t.block = true;
  t.content = [first, next > startLine ? state.getLines(startLine + 1, next, state.tShift[startLine], true) : '', last].filter((x) => x.trim()).join('\n').trim();
  t.map = [startLine, state.line];
  t.markup = '$$';
  return true;
}

const ALERTS = ['note', 'tip', 'important', 'warning', 'caution'] as const;
type Alert = (typeof ALERTS)[number];

function slugify(s: string): string {
  const base =
    s
      .normalize('NFKD')
      .replace(/[̀-ͯ]/g, '')
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\s-]/gu, '')
      .trim()
      .replace(/\s+/g, '-')
      .slice(0, 64) || 'section';
  return `md-${base}`;
}

function attr(t: Token, name: string): string | null {
  const v = t.attrGet(name);
  return v === null || v === undefined ? null : String(v);
}

function inlineText(t: Token | undefined): string {
  return (t?.children ?? [])
    .filter((c) => c.type === 'text' || c.type === 'code_inline' || c.type === 'emoji')
    .map((c) => c.content)
    .join('');
}

function build(labels: Labels): MarkdownIt {
  const md = new MarkdownItCtor('default', {
    html: false,
    linkify: true,
    typographer: false,
    breaks: false,
    maxNesting: 40,
  });
  md.validateLink = () => true;
  md.linkify.set({ fuzzyEmail: false });
  md.use(footnote).use(deflist).use(emoji).use(sub).use(sup).use(mark).use(abbr).use(anchor, { slugify, tabIndex: false });
  md.inline.ruler.after('escape', 'math_inline', mathInline);
  md.block.ruler.after('blockquote', 'math_block', mathBlock, { alt: ['paragraph', 'reference', 'blockquote', 'list'] });

  md.core.ruler.after('block', 'md_alerts', (state) => {
    const ts = state.tokens;
    for (let i = 0; i + 2 < ts.length; i++) {
      if (ts[i].type !== 'blockquote_open' || ts[i + 1].type !== 'paragraph_open' || ts[i + 2].type !== 'inline') continue;
      const m = /^\[!(note|tip|important|warning|caution)\][ \t]*(?:\n|$)/i.exec(ts[i + 2].content);
      if (!m) continue;
      ts[i].meta = { alert: m[1].toLowerCase() as Alert };
      ts[i + 2].content = ts[i + 2].content.slice(m[0].length);
      if (!ts[i + 2].content.trim()) {
        ts[i + 1].hidden = ts[i + 3].hidden = true;
        ts[i + 2].content = '';
      }
    }
  });

  md.core.ruler.after('inline', 'md_tasks', (state) => {
    const ts = state.tokens;
    for (let i = 2; i < ts.length; i++) {
      if (ts[i].type !== 'inline' || ts[i - 1].type !== 'paragraph_open' || ts[i - 2].type !== 'list_item_open') continue;
      const first = ts[i].children?.[0];
      const m = first?.type === 'text' ? /^\[([ xX])\][ \t]+/.exec(first.content) : null;
      if (!m || !first) continue;
      first.content = first.content.slice(m[0].length);
      const box = new state.Token('md_task', '', 0);
      box.meta = { done: m[1] !== ' ' };
      ts[i].children!.unshift(box);
      ts[i - 2].attrJoin('class', 'md-task');
    }
  });

  md.core.ruler.push('md_align', (state) => {
    for (const t of state.tokens) {
      if (t.type !== 'th_open' && t.type !== 'td_open') continue;
      const st = attr(t, 'style');
      const m = st && /text-align:\s*(left|center|right)/.exec(st);
      t.attrs = null;
      if (m) t.attrSet('class', `md-al-${m[1]}`);
    }
  });

  const r = md.renderer.rules;
  const deadLinks: boolean[] = [];
  r.link_open = (tokens, idx) => {
    const href = attr(tokens[idx], 'href') ?? '';
    if (/^#[^\s"<>]*$/.test(href) && href.length > 1) {
      deadLinks.push(false);
      return `<a href="${esc(href)}" class="md-anchor">`;
    }
    const safe = safeHref(href);
    deadLinks.push(!safe);
    return safe ? `<a href="${esc(safe)}" class="md-link" title="${esc(safe)}">` : '<span class="md-deadlink">';
  };
  r.link_close = () => (deadLinks.pop() ? '</span>' : '</a>');
  r.image = (tokens, idx, _o, env) => {
    const t = tokens[idx];
    const src = attr(t, 'src') ?? '';
    const alt = md.renderer.renderInlineAsText(t.children ?? [], md.options, env);
    const local = localImagePath(src);
    const shown = esc(alt || labels.image);
    if (local) return `<span class="md-img md-img-local" data-path="${esc(local)}" title="${esc(local)}">${shown}</span>`;
    return `<span class="md-img" title="${esc(src.slice(0, 300))}">${shown} <span class="md-img-note">${esc(labels.imageBlocked)}</span></span>`;
  };
  r.md_task = (tokens, idx) => {
    const done = !!tokens[idx].meta?.done;
    return `<span class="md-box" role="img" aria-label="${esc(done ? labels.done : labels.todo)}" data-done="${done ? 1 : 0}"></span>`;
  };
  r.math_inline = (tokens, idx) => `<span class="md-math" data-display="0">${esc(tokens[idx].content)}</span>`;
  r.math_block = (tokens, idx) => `<div class="md-math" data-display="1">${esc(tokens[idx].content)}</div>\n`;
  r.blockquote_open = (tokens, idx) => {
    const a = tokens[idx].meta?.alert as Alert | undefined;
    if (!a) return '<blockquote>\n';
    return `<blockquote class="md-alert md-alert-${a}"><p class="md-alert-title">${esc(labels[a])}</p>\n`;
  };
  const code = (lang: string, body: string) => {
    if (lang === 'mermaid') return `<div class="md-mermaid"><pre class="md-code" data-lang="mermaid"><code>${esc(body)}</code></pre></div>\n`;
    if (lang === 'math' || lang === 'latex' || lang === 'tex' || lang === 'katex') return `<div class="md-math" data-display="1">${esc(body)}</div>\n`;
    const hl = lang ? languageFor(`x.${ALIAS[lang] ?? lang}`) : null;
    const inner = tokenize(body.replace(/\n$/, ''), hl)
      .map((t) => (t.c ? `<span class="t-${t.c}">${esc(t.s)}</span>` : esc(t.s)))
      .join('');
    const label = lang && /^[\w+#.-]{1,24}$/.test(lang) ? ` data-lang="${esc(lang)}"` : '';
    return `<pre class="md-code"${label}><code>${inner}</code></pre>\n`;
  };
  r.fence = (tokens, idx) => {
    const t = tokens[idx];
    return code((t.info.trim().split(/\s+/)[0] ?? '').toLowerCase(), t.content);
  };
  r.code_block = (tokens, idx) => code('', tokens[idx].content);
  r.table_open = () => '<div class="md-table-wrap"><table class="vw-table">\n';
  r.table_close = () => '</table></div>\n';
  return md;
}

const ALIAS: Record<string, string> = {
  javascript: 'js',
  typescript: 'ts',
  shell: 'sh',
  console: 'sh',
  bash: 'sh',
  python: 'py',
  rust: 'rs',
  yml: 'yaml',
  golang: 'go',
  'c++': 'cpp',
  csharp: 'cs',
  html: 'html',
  xml: 'xml',
  svelte: 'svelte',
};

const TAGS = [
  'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'p', 'br', 'hr', 'strong', 'em', 's', 'del', 'code', 'pre', 'blockquote',
  'ul', 'ol', 'li', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'a', 'span', 'div', 'sup', 'sub', 'mark', 'abbr',
  'dl', 'dt', 'dd', 'section',
];
const ATTRS = ['href', 'id', 'class', 'title', 'start', 'role', 'aria-label', 'data-path', 'data-display', 'data-done', 'data-lang'];

let purifier: ReturnType<typeof DOMPurify> | null = null;

export function sanitize(html: string): string {
  purifier ??= DOMPurify(window);
  return purifier.sanitize(html, {
    ALLOWED_TAGS: TAGS,
    ALLOWED_ATTR: ATTRS,
    ALLOW_DATA_ATTR: true,
    ALLOW_ARIA_ATTR: true,
    ALLOWED_URI_REGEXP: /^(?:https?:\/\/|mailto:|#)/i,
    FORBID_ATTR: ['style'],
    RETURN_TRUSTED_TYPE: false,
  }) as string;
}

export function sanitizeMath(html: string): string {
  purifier ??= DOMPurify(window);
  return purifier.sanitize(html, {
    USE_PROFILES: { html: true, mathMl: true },
    FORBID_TAGS: ['a', 'img', 'iframe', 'form', 'input', 'button', 'object', 'embed', 'video', 'audio', 'style', 'link'],
    FORBID_ATTR: ['href', 'src', 'xlink:href', 'action', 'formaction'],
    ALLOW_DATA_ATTR: false,
    RETURN_TRUSTED_TYPE: false,
  }) as string;
}

export function sanitizeSvg(svg: string): string {
  purifier ??= DOMPurify(window);
  return purifier.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true },
    ADD_TAGS: ['style'],
    FORBID_TAGS: ['a', 'foreignObject', 'image', 'use', 'script', 'animate', 'set', 'animateTransform', 'animateMotion'],
    FORBID_ATTR: ['href', 'xlink:href', 'onload', 'onerror'],
    ALLOW_DATA_ATTR: false,
    RETURN_TRUSTED_TYPE: false,
  }) as string;
}

const cache = new Map<string, MarkdownIt>();

export function renderMarkdown(src: string, labels: Labels = EN): Rendered {
  const key = JSON.stringify(labels);
  let md = cache.get(key);
  if (!md) cache.set(key, (md = build(labels)));
  const { front, body } = splitFrontMatter(src.replace(/\r\n?/g, '\n'));
  const env: Record<string, unknown> = { docId: 'md' };
  const tokens = md.parse(body, env);
  const toc: TocItem[] = [];
  let math = false;
  let mermaid = false;
  const walk = (list: Token[]) => {
    for (let i = 0; i < list.length; i++) {
      const t = list[i];
      if (t.type === 'heading_open' && toc.length < 200) {
        const id = attr(t, 'id');
        if (id) toc.push({ level: Number(t.tag.slice(1)), id, text: inlineText(list[i + 1]) });
      }
      if (t.type === 'math_inline' || t.type === 'math_block') math = true;
      if (t.type === 'fence') {
        const lang = t.info.trim().split(/\s+/)[0]?.toLowerCase();
        if (lang === 'mermaid') mermaid = true;
        if (lang === 'math' || lang === 'latex' || lang === 'tex' || lang === 'katex') math = true;
      }
      if (t.children) walk(t.children);
    }
  };
  walk(tokens);
  const html = sanitize(md.renderer.render(tokens, md.options, env));
  return { html, toc, front, math, mermaid };
}
