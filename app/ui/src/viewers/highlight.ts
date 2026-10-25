// A tiny syntax highlighter for the code viewer (no dependency: none was installed, and a
// full grammar engine is far more than a quick look needs). Sticky regexes per language turn
// text into classed tokens; the viewer renders them as text nodes inside <span>s, never as
// HTML. Classes: com (comment), str (string), num, kw (keyword), lit (true/null…), type,
// fn (called name), key (object/config key), tag, attr, meta (decorator, preprocessor,
// section header), del/ins (diff lines), op (punctuation that carries meaning).

export type TokenClass = 'com' | 'str' | 'num' | 'kw' | 'lit' | 'type' | 'fn' | 'key' | 'tag' | 'attr' | 'meta' | 'del' | 'ins' | 'op';
export interface Token {
  c: TokenClass | null;
  s: string;
}

type Rule = [TokenClass | null | 'ident', RegExp];
interface Lang {
  rules: Rule[];
  keywords?: Set<string>;
  literals?: Set<string>;
  types?: Set<string>;
}

const words = (s: string) => new Set(s.split(/\s+/).filter(Boolean));
const y = (re: RegExp) => new RegExp(re.source, re.flags.includes('y') ? re.flags : re.flags + 'y');

const NUM: Rule = ['num', y(/(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)[a-zA-Z\d]*/)];
const IDENT: Rule = ['ident', y(/[A-Za-z_$][\w$]*/)];
const DQ: Rule = ['str', y(/"(?:\\[\s\S]|[^"\\\n])*"?/)];
const SQ: Rule = ['str', y(/'(?:\\[\s\S]|[^'\\\n])*'?/)];
const BLOCK: Rule = ['com', y(/\/\*[\s\S]*?(?:\*\/|$)/)];
const LINE: Rule = ['com', y(/\/\/[^\n]*/)];
const HASH: Rule = ['com', y(/#[^\n]*/)];

const cLike = (kw: string, lit: string, extra: Rule[] = [], types = ''): Lang => ({
  rules: [BLOCK, LINE, ...extra, DQ, SQ, NUM, IDENT],
  keywords: words(kw),
  literals: words(lit),
  types: words(types),
});

const JS_KW =
  'async await break case catch class const continue debugger default delete do else enum export extends finally for from function get if implements import in instanceof interface let new of package private protected public readonly return set static super switch this throw try type typeof var void while with yield as satisfies declare namespace abstract keyof infer is';
const LANGS: Record<string, Lang> = {
  js: cLike(JS_KW, 'true false null undefined NaN Infinity', [['str', y(/`(?:\\[\s\S]|[^`\\])*`?/)], ['meta', y(/@[\w.]+/)]], 'string number boolean any unknown never object symbol bigint void'),
  rust: cLike(
    'as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait type unsafe use where while macro_rules',
    'true false None Some Ok Err',
    [['meta', y(/#!?\[[^\]\n]*\]?/)], ['str', y(/b?r#*"[\s\S]*?"#*/)], ['str', y(/'(?:\\.|[^'\\\n])'/)], ['meta', y(/'[a-z_]\w*/)]],
    'i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str String Vec Option Result Box',
  ),
  go: cLike(
    'break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var',
    'true false nil iota',
    [['str', y(/`[^`]*`?/)]],
    'bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr any',
  ),
  c: cLike(
    'auto break case catch class const constexpr continue default delete do else enum explicit extern for friend goto if inline namespace new noexcept operator private protected public register return sizeof static struct switch template this throw try typedef typename union using virtual volatile while abstract extends final implements import instanceof interface package super synchronized throws transient var val fun when object override suspend lateinit init is in out',
    'true false null nullptr NULL',
    [['meta', y(/#[ \t]*\w+/)], ['meta', y(/@\w+/)]],
    'void char short int long float double signed unsigned bool size_t String Integer Boolean Object',
  ),
  python: {
    rules: [HASH, ['str', y(/[rbfuRBFU]{0,2}"""[\s\S]*?(?:"""|$)/)], ['str', y(/[rbfuRBFU]{0,2}'''[\s\S]*?(?:'''|$)/)], ['str', y(/[rbfuRBFU]{0,2}"(?:\\[\s\S]|[^"\\\n])*"?/)], ['str', y(/[rbfuRBFU]{0,2}'(?:\\[\s\S]|[^'\\\n])*'?/)], ['meta', y(/@[\w.]+/)], NUM, IDENT],
    keywords: words('and as assert async await break class continue def del elif else except finally for from global if import in is lambda match case nonlocal not or pass raise return try while with yield'),
    literals: words('True False None self cls'),
    types: words('int float str bytes bool list dict set tuple object'),
  },
  shell: {
    rules: [HASH, DQ, ['str', y(/'[^']*'?/)], ['meta', y(/\$\{[^}\n]*\}?|\$[\w@#?$!*-]/)], NUM, ['ident', y(/[A-Za-z_][\w-]*/)]],
    keywords: words('if then else elif fi for while until do done case esac in function return local export readonly declare set unset shift exit source alias trap'),
    literals: words('true false'),
  },
  json: {
    rules: [BLOCK, LINE, ['key', y(/"(?:\\[\s\S]|[^"\\\n])*"(?=\s*:)/)], DQ, NUM, ['lit', y(/\b(?:true|false|null)\b/)]],
  },
  css: {
    rules: [BLOCK, DQ, SQ, ['meta', y(/@[\w-]+/)], ['num', y(/#[\da-fA-F]{3,8}\b|-?\d*\.?\d+(?:%|[a-z]+)?/)], ['key', y(/[\w-]+(?=\s*:(?!:))/)], ['tag', y(/[.#]?[\w-]+/)]],
  },
  markup: {
    rules: [
      ['com', y(/<!--[\s\S]*?(?:-->|$)/)],
      ['meta', y(/<![^>]*>?|<\?[\s\S]*?(?:\?>|$)/)],
      ['tag', y(/<\/?[\w:.-]+/)],
      ['tag', y(/\/?>/)],
      ['str', y(/"[^"]*"?|'[^']*'?/)],
      ['attr', y(/[\w:.-]+(?==)/)],
      ['meta', y(/&[#\w]+;/)],
    ],
  },
  yaml: {
    rules: [HASH, ['meta', y(/^---$|^\.\.\.$/m)], ['key', y(/[\w.-]+(?=\s*:(?:\s|$))/)], DQ, SQ, NUM, ['lit', y(/\b(?:true|false|null|yes|no|on|off)\b/i)], ['meta', y(/[&*][\w-]+|![\w!-]*/)]],
  },
  ini: {
    rules: [['com', y(/[#;][^\n]*/)], ['meta', y(/^\s*\[[^\]\n]*\]?/m)], ['key', y(/[\w.-]+(?=\s*=)/)], ['str', y(/"""[\s\S]*?(?:"""|$)/)], DQ, SQ, NUM, ['lit', y(/\b(?:true|false)\b/)]],
  },
  sql: {
    rules: [['com', y(/--[^\n]*/)], BLOCK, SQ, DQ, NUM, IDENT],
    keywords: words(
      'select from where and or not insert into values update set delete create table index view drop alter add column primary key foreign references join left right inner outer on group by order having limit offset as distinct union all case when then else end null is in like between exists begin commit rollback transaction pragma default unique if',
    ),
  },
  diff: {
    rules: [['meta', y(/^(?:@@[^\n]*|diff [^\n]*|index [^\n]*|\+\+\+ [^\n]*|--- [^\n]*)/m)], ['ins', y(/^\+[^\n]*/m)], ['del', y(/^-[^\n]*/m)]],
  },
};

const BY_EXT: Record<string, string> = {};
const map = (lang: string, exts: string) => exts.split(' ').forEach((e) => (BY_EXT[e] = lang));
map('js', 'js mjs cjs jsx ts mts cts tsx svelte vue');
map('rust', 'rs');
map('go', 'go');
map('c', 'c h cpp hpp cc cxx hh java kt kts cs swift scala dart php m mm');
map('python', 'py pyw pyi');
map('shell', 'sh bash zsh fish ksh command');
map('json', 'json jsonc json5 webmanifest ipynb geojson lock');
map('css', 'css scss sass less');
map('markup', 'html htm xhtml xml svg plist xsl xslt rss atom');
map('yaml', 'yaml yml');
map('ini', 'toml ini cfg conf properties env editorconfig gitconfig desktop service');
map('sql', 'sql');
map('diff', 'diff patch');

/** The highlighter's language for a file name, or null for plain text. */
export function languageFor(name: string): string | null {
  const lower = name.toLowerCase();
  if (lower === 'dockerfile' || lower === 'makefile' || lower.endsWith('.mk')) return 'shell';
  if (lower === '.gitignore' || lower === '.env') return 'ini';
  const i = lower.lastIndexOf('.');
  const ext = i >= 0 ? lower.slice(i + 1) : '';
  return BY_EXT[ext] ?? null;
}

export const LANGUAGE_LABEL: Record<string, string> = {
  js: 'JavaScript / TypeScript',
  rust: 'Rust',
  go: 'Go',
  c: 'C / C++ / Java',
  python: 'Python',
  shell: 'Shell',
  json: 'JSON',
  css: 'CSS',
  markup: 'HTML / XML',
  yaml: 'YAML',
  ini: 'TOML / INI',
  sql: 'SQL',
  diff: 'Diff',
};

export function tokenize(text: string, lang: string | null): Token[] {
  const L = lang ? LANGS[lang] : undefined;
  if (!L) return [{ c: null, s: text }];
  const out: Token[] = [];
  let plain = '';
  const push = (c: TokenClass | null, s: string) => {
    if (c === null) {
      plain += s;
      return;
    }
    if (plain) out.push({ c: null, s: plain }), (plain = '');
    out.push({ c, s });
  };
  let i = 0;
  const n = text.length;
  outer: while (i < n) {
    for (const [cls, re] of L.rules) {
      re.lastIndex = i;
      const m = re.exec(text);
      if (!m || m[0].length === 0) continue;
      const s = m[0];
      if (cls === 'ident') {
        // a name glued to the previous word char is part of it (e.g. "x1y")
        let c: TokenClass | null = null;
        const key = lang === 'sql' ? s.toLowerCase() : s;
        if (L.keywords?.has(key)) c = 'kw';
        else if (L.literals?.has(s)) c = 'lit';
        else if (L.types?.has(s)) c = 'type';
        else if (text.charCodeAt(i + s.length) === 40 /* ( */) c = 'fn';
        else if (/^[A-Z][a-z]\w*$/.test(s) && lang !== 'shell' && lang !== 'sql') c = 'type';
        push(c, s);
      } else push(cls, s);
      i += s.length;
      continue outer;
    }
    // no rule: one char of plain text (a run of word chars at once, so numbers inside names stay plain)
    const w = /[\w$]+|\s+|./y;
    w.lastIndex = i;
    const s = w.exec(text)![0];
    push(null, s);
    i += s.length;
  }
  if (plain) out.push({ c: null, s: plain });
  return out;
}

/** Tokens split into lines (a token spanning lines is cut at each newline). */
export function toLines(tokens: Token[]): Token[][] {
  const lines: Token[][] = [[]];
  for (const t of tokens) {
    const parts = t.s.split('\n');
    for (let k = 0; k < parts.length; k++) {
      if (k > 0) lines.push([]);
      if (parts[k]) lines[lines.length - 1].push({ c: t.c, s: parts[k] });
    }
  }
  return lines;
}
