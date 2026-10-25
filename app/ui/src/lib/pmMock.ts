// In-browser mock of the pm_* commands (password manager), plugged into MockBackend.invoke.
// Same names, argument shapes, errors and events as app/src-tauri/src/passwords.rs.
// Seeded volumes get sample entries (TOTP included); real RFC 6238 codes via WebCrypto.

import { switches } from './switches';
import { demoPmSeeds } from './mockDemo';
import type { ApiError, EventMap, EventName, JobDone, JobId, VaultId } from './types';
import type {
  PmCopied,
  PmEntry,
  PmEntryInput,
  PmEntrySummary,
  PmFieldInput,
  PmGenOptions,
  PmGenerated,
  PmHistoryItem,
  PmTotpCheck,
  PmTotpCode,
  PmTotpInfo,
  TotpAlgorithm,
} from './passwords';

export interface PmMockHost {
  /** path of an open volume; throws "locked" like MockBackend.vault() */
  pathOf(vault: VaultId): string;
  /** volumes that start with the sample entries */
  seeded(path: string): boolean;
  wait(ms: number): Promise<void>;
  emit<E extends EventName>(event: E, payload: EventMap[E]): void;
  speed(): number;
  /** extra generated entries in seeded volumes (?pm=N, perf runs); optional */
  extra?(): number;
  /** the global site_icons setting (default on) */
  siteIcons?(): boolean;
}

interface StoredField {
  name: string;
  value: string;
  hidden: boolean;
}

interface StoredEntry {
  s: PmEntrySummary;
  password: string;
  notes: string;
  totp: string | null;
  fields: StoredField[];
  history: { password: string; changed: number }[];
}

const HISTORY_KEEP = 10;
const DAY = 86_400_000;
const SYMBOLS = '!#$%&()*+,-./:;<=>?@[]^_{|}~';
const AMBIGUOUS = '0O1lI|';
/** The backend's lists have 7776 words (entropy is computed with that); the mock samples a few. */
const WORDLIST_SIZE = 7776;
const MOCK_WORDS: Record<'en' | 'it', string[]> = {
  en: ['acorn', 'blanket', 'cactus', 'dolphin', 'ember', 'falcon', 'gravel', 'harbor', 'island', 'jigsaw', 'kettle', 'lantern', 'meadow', 'nectar', 'orbit', 'pebble', 'quartz', 'ripple', 'saddle', 'thicket', 'umpire', 'velvet', 'walnut', 'yodel'],
  it: ['acero', 'bussola', 'cavallo', 'dattero', 'edera', 'faro', 'gelso', 'isola', 'lampada', 'mattone', 'nuvola', 'ombrello', 'palude', 'quercia', 'rondine', 'sentiero', 'tamburo', 'uliveto', 'vela', 'zucca', 'grotta', 'ciliegia', 'fiume', 'mulino'],
};

function err(code: string, message: string): ApiError {
  return { code, message };
}

// --- TOTP ---------------------------------------------------------------------

function base32Decode(s: string): Uint8Array | null {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  const out: number[] = [];
  let buf = 0;
  let bits = 0;
  let n = 0;
  for (const ch of s.toUpperCase()) {
    if (' -=\t\r\n'.includes(ch)) continue;
    const v = A.indexOf(ch);
    if (v < 0) return null;
    n++;
    buf = ((buf << 5) | v) & 0xffff;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((buf >> bits) & 0xff);
    }
  }
  return n > 0 ? new Uint8Array(out) : null;
}

interface ParsedTotp extends PmTotpInfo {
  secret: Uint8Array;
}

function parseTotp(input: string): ParsedTotp {
  const s = input.trim();
  const bad = (m: string) => err('invalid_totp', `Invalid 2FA code: ${m}.`);
  if (!/^otpauth:/i.test(s)) {
    if (/^otpauth-migration:/i.test(s)) throw bad('only time-based codes (otpauth://totp) are supported');
    if (s.includes(':')) throw bad('not a TOTP secret or otpauth://totp URI');
    const secret = base32Decode(s);
    if (!secret) throw bad('secret is not valid base32');
    return { secret, issuer: null, account: null, algorithm: 'SHA1', digits: 6, period: 30 };
  }
  const m = /^otpauth:\/\/([^/]+)\/([^?]*)\??(.*)$/i.exec(s);
  if (!m) throw bad('not a TOTP secret or otpauth://totp URI');
  if (m[1].toLowerCase() !== 'totp') throw bad('only time-based codes (otpauth://totp) are supported');
  const label = decodeURIComponent(m[2]);
  const q = new URLSearchParams(m[3]);
  const [li, la] = label.includes(':') ? label.split(/:(.*)/s) : [null, label];
  const secret = base32Decode(q.get('secret') ?? '');
  if (!secret) throw bad('secret is not valid base32');
  const alg = (q.get('algorithm') ?? 'SHA1').toUpperCase().replace(/[-_]/g, '').replace(/^HMAC/, '');
  if (!['SHA1', 'SHA256', 'SHA512'].includes(alg)) throw bad('digits must be 6..=8 and period 1..=300 seconds');
  const digits = Number(q.get('digits') ?? 6);
  const period = Number(q.get('period') ?? 30);
  if (!(digits >= 6 && digits <= 8) || !(period >= 1 && period <= 300)) throw bad('digits must be 6..=8 and period 1..=300 seconds');
  const issuer = (q.get('issuer') || li || '').trim() || null;
  return { secret, issuer, account: (la ?? '').trim() || null, algorithm: alg as TotpAlgorithm, digits, period };
}

async function totpCode(t: ParsedTotp, nowMs = Date.now()): Promise<PmTotpCode> {
  const secs = Math.floor(nowMs / 1000);
  const counter = Math.floor(secs / t.period);
  const msg = new Uint8Array(8);
  new DataView(msg.buffer).setBigUint64(0, BigInt(counter));
  const hash = t.algorithm === 'SHA1' ? 'SHA-1' : t.algorithm === 'SHA256' ? 'SHA-256' : 'SHA-512';
  const key = await crypto.subtle.importKey('raw', t.secret as BufferSource, { name: 'HMAC', hash }, false, ['sign']);
  const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, msg));
  const off = mac[mac.length - 1] & 0x0f;
  const bin = ((mac[off] & 0x7f) << 24) | (mac[off + 1] << 16) | (mac[off + 2] << 8) | mac[off + 3];
  const code = String(bin % 10 ** t.digits).padStart(t.digits, '0');
  return { code, remaining: t.period - (secs % t.period), period: t.period, digits: t.digits };
}

/** Exposed for tests: RFC 6238 vectors in the mock. */
export const _totp = { parseTotp, totpCode };

// --- generator ------------------------------------------------------------------

function below(n: number): number {
  const zone = 0x1_0000_0000 - (0x1_0000_0000 % n);
  const b = new Uint32Array(1);
  for (;;) {
    crypto.getRandomValues(b);
    if (b[0] < zone) return b[0] % n;
  }
}

function generate(o: PmGenOptions): PmGenerated {
  const bad = (m: string) => err('invalid_options', `Invalid options: ${m}.`);
  if (o.kind === 'passphrase') {
    const words = o.words ?? 5;
    const lang = o.language ?? 'en';
    const sep = o.separator ?? '-';
    if (!(words >= 3 && words <= 20)) throw bad('words must be 3..=20');
    if (!(lang in MOCK_WORDS)) throw bad('unknown wordlist language');
    if ([...sep].length > 3) throw bad('separator is longer than 3 characters');
    const list = MOCK_WORDS[lang];
    const picked = Array.from({ length: words }, () => {
      const w = list[below(list.length)];
      return o.capitalize ? w[0].toUpperCase() + w.slice(1) : w;
    });
    let bits = words * Math.log2(WORDLIST_SIZE);
    if (o.number) {
      picked[below(words)] += String(below(10));
      bits += Math.log2(10) + Math.log2(words);
    }
    return { value: picked.join(sep), entropy_bits: bits };
  }
  const length = o.length ?? 20;
  if (!(length >= 4 && length <= 128)) throw bad('length must be 4..=128');
  const sets = [
    [o.lower ?? true, 'abcdefghijklmnopqrstuvwxyz'],
    [o.upper ?? true, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'],
    [o.digits ?? true, '0123456789'],
    [o.symbols ?? true, SYMBOLS],
  ] as const;
  const classes = sets.filter(([on]) => on).map(([, set]) => [...set].filter((c) => !(o.exclude_ambiguous && AMBIGUOUS.includes(c))));
  if (!classes.length) throw bad('choose at least one character class');
  const pool = classes.flat();
  for (;;) {
    const v = Array.from({ length }, () => pool[below(pool.length)]).join('');
    if (classes.every((c) => c.some((x) => v.includes(x)))) {
      // exact count of strings containing every class (inclusion-exclusion), like the backend
      let total = 0;
      for (let mask = 0; mask < 1 << classes.length; mask++) {
        const size = classes.reduce((s, c, i) => (mask & (1 << i) ? s : s + c.length), 0);
        total += (bitCount(mask) % 2 ? -1 : 1) * size ** length;
      }
      return { value: v, entropy_bits: Math.log2(total) };
    }
  }
}

function bitCount(n: number): number {
  let c = 0;
  for (; n; n &= n - 1) c++;
  return c;
}

// --- import parsing (simplified mirrors of nv_core::passwords::import) ---------------

interface Imported {
  input: PmEntryInput;
  history?: { password: string; changed: number }[];
  created?: number;
  updated?: number;
}

function hostOf(url: string): string {
  const m = /^(?:[a-z]+:\/\/)?(?:[^@/]*@)?([^/:?#]+)/i.exec(url.trim());
  return m ? m[1].replace(/^www\./, '') : '';
}

function csvRows(text: string): string[][] {
  const delim = (text.split('\n')[0].match(/;/g)?.length ?? 0) > (text.split('\n')[0].match(/,/g)?.length ?? 0) ? ';' : ',';
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = '';
  let q = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (q) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') q = false;
      else cell += c;
    } else if (c === '"') q = true;
    else if (c === delim) {
      row.push(cell);
      cell = '';
    } else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++;
      row.push(cell);
      rows.push(row);
      row = [];
      cell = '';
    } else cell += c;
  }
  if (cell || row.length) rows.push([...row, cell]);
  return rows.filter((r) => r.some((c) => c.trim()));
}

const CSV_COLS: Record<string, keyof PmEntryInput | 'ignore'> = {
  title: 'title', name: 'title', account: 'title',
  username: 'username', login: 'username', loginusername: 'username', user: 'username', email: 'username',
  password: 'password', loginpassword: 'password',
  url: 'urls', uri: 'urls', loginuri: 'urls', website: 'urls',
  notes: 'notes', note: 'notes', extra: 'notes', comments: 'notes',
  totp: 'totp', otp: 'totp', logintotp: 'totp', otpauth: 'totp',
  folder: 'folder', group: 'folder', grouping: 'folder',
  tags: 'tags', favorite: 'favorite', fav: 'favorite',
  httprealm: 'ignore', formactionorigin: 'ignore', guid: 'ignore', timelastused: 'ignore',
  type: 'ignore', reprompt: 'ignore', archived: 'ignore', fields: 'ignore', icon: 'ignore', vault: 'ignore',
};

/** Mirrors passwords::tidy: names whose values become websites / times instead of custom fields. */
const TIDY: Record<string, 'urls' | 'created' | 'updated'> = {
  autofillurls: 'urls', urls: 'urls', uris: 'urls', websites: 'urls', additionalurls: 'urls', matchurl: 'urls',
  createtime: 'created', creationtime: 'created', created: 'created', createdat: 'created', creationdate: 'created', timecreated: 'created',
  modifytime: 'updated', modificationtime: 'updated', modified: 'updated', lastmodified: 'updated', updatedat: 'updated',
  revisiondate: 'updated', timepasswordchanged: 'updated',
};

/** unix s / ms / µs or ISO 8601 -> ms (null: not a believable time) */
export function parseTime(v: string): number | null {
  const t = v.trim();
  let ms: number;
  if (/^\d+(\.\d+)?$/.test(t)) {
    const n = Number(t);
    ms = n < 1e11 ? n * 1000 : n < 1e14 ? n : n < 1e17 ? n / 1000 : n / 1e6;
  } else if (/^\d{4}-\d{2}-\d{2}([T ]\d{2}:\d{2}:\d{2})?/.test(t)) {
    ms = Date.parse(t.length === 10 ? `${t}T00:00:00Z` : /[zZ]|[+-]\d{2}:?\d{2}$/.test(t) ? t.replace(' ', 'T') : `${t.replace(' ', 'T')}Z`);
  } else return null;
  return Number.isFinite(ms) && ms >= 631_152_000_000 && ms < 7_258_118_400_000 ? Math.floor(ms) : null;
}

/** JSON list of strings / {url|uri|href}, or http(s) URLs separated by spaces / commas (null: not that) */
export function parseUrls(v: string): string[] | null {
  const t = v.trim();
  if (!t) return null;
  if (t.startsWith('[')) {
    try {
      const a = JSON.parse(t);
      if (!Array.isArray(a)) return null;
      const out: string[] = [];
      for (const x of a) {
        const u = typeof x === 'string' ? x : x && typeof x === 'object' ? (x.url ?? x.uri ?? x.href) : null;
        if (typeof u !== 'string') return null;
        if (u.trim() && !out.includes(u.trim())) out.push(u.trim());
      }
      return out;
    } catch {
      return null;
    }
  }
  const parts = t.split(/[\s,;]+/).filter(Boolean);
  return parts.every((p) => /^https?:\/\//i.test(p)) ? [...new Set(parts)] : null;
}

/** Applies TIDY to an imported entry's custom fields (mirrors import::tidy_imported). */
function tidyFields(it: Imported) {
  const keep: PmFieldInput[] = [];
  for (const f of it.input.fields ?? []) {
    const k = TIDY[f.name.toLowerCase().replace(/[^a-z0-9]/g, '')];
    const v = f.value ?? '';
    if (k === 'urls') {
      const u = parseUrls(v);
      if (u) {
        it.input.urls = [...new Set([...(it.input.urls ?? []), ...u])];
        continue;
      }
    } else if (k) {
      const ms = parseTime(v);
      if (ms !== null) {
        if (k === 'created') it.created ??= ms;
        else it.updated ??= ms;
        continue;
      }
    }
    keep.push(f);
  }
  it.input.fields = keep.length ? keep : undefined;
}

function parseCsv(text: string): Imported[] {
  const rows = csvRows(text.replace(/^﻿/, ''));
  const rows0 = rows.shift() ?? [];
  const head = rows0.map((h) => h.toLowerCase().replace(/[^a-z0-9]/g, ''));
  const cols: (keyof PmEntryInput | 'ignore' | 'custom')[] = head.map((h) => CSV_COLS[h] ?? 'custom');
  if (!cols.includes('password') && !cols.includes('username')) throw err('invalid_import', 'Not recognised (CSV: no username or password column).');
  return rows.map((r) => {
    const e: PmEntryInput = { urls: [] };
    const fields: PmFieldInput[] = [];
    r.forEach((raw, i) => {
      const v = raw.trim();
      const c = cols[i];
      if (!v || !c || c === 'ignore') return;
      if (c === 'urls') e.urls!.push(v);
      else if (c === 'tags') e.tags = v.split(/[,;]/).map((t) => t.trim()).filter(Boolean);
      else if (c === 'favorite') e.favorite = /^(1|true|yes|s[iì]|x)$/i.test(v);
      else if (c === 'custom') fields.push({ name: (rows0[i] ?? head[i]).trim(), value: v, hidden: false });
      else (e as Record<string, unknown>)[c] = v;
    });
    if (fields.length) e.fields = fields;
    const it: Imported = { input: e };
    tidyFields(it);
    return it;
  });
}

function parseBitwarden(text: string): Imported[] {
  let j: { encrypted?: boolean; folders?: { id: string; name: string }[]; items?: Record<string, any>[] }; // eslint-disable-line @typescript-eslint/no-explicit-any
  try {
    j = JSON.parse(text);
  } catch (e) {
    throw err('invalid_import', `Not recognised (Bitwarden JSON: ${(e as Error).message}).`);
  }
  if (j.encrypted) throw err('unsupported', 'Encrypted exports are not supported: export unencrypted JSON.');
  if (!Array.isArray(j.items)) throw err('invalid_import', 'Not recognised (Bitwarden JSON: no items).');
  return j.items.map((it) => ({
    input: {
      title: it.name ?? '',
      username: it.login?.username ?? undefined,
      password: it.login?.password ?? undefined,
      urls: (it.login?.uris ?? []).map((u: { uri?: string }) => u.uri).filter(Boolean),
      notes: it.notes ?? undefined,
      totp: it.login?.totp ?? undefined,
      folder: j.folders?.find((f) => f.id === it.folderId)?.name,
      favorite: !!it.favorite,
      fields: (it.fields ?? []).filter((f: { name?: string }) => f.name).map((f: { name: string; value?: unknown; type?: number }) => ({ name: f.name, value: String(f.value ?? ''), hidden: f.type === 1 })),
    },
    history: (it.passwordHistory ?? []).map((h: { password: string; lastUsedDate?: string }) => ({ password: h.password, changed: Date.parse(h.lastUsedDate ?? '') || 0 })),
  }));
}

/** KeePass in the mock: the master password "demo" opens a canned database. */
function cannedKeepass(): Imported[] {
  return [
    { input: { title: 'Router', username: 'admin', password: 'kp-router-77', urls: ['http://192.168.1.1'], folder: 'Casa/Rete' } },
    { input: { title: 'NAS', username: 'mario', password: 'kp-nas-31', folder: 'Casa/Rete', totp: 'otpauth://totp/NAS:mario?secret=GEZDGNBVGY3TQOJQ' } },
    { input: { title: 'Gestionale', username: 'm.rossi', password: 'kp-erp-09', urls: ['https://erp.example.it'], folder: 'Lavoro', fields: [{ name: 'PIN', value: '5521', hidden: true }] } },
  ];
}

// --- seeds ------------------------------------------------------------------------------

function seeds(now: number): { input: PmEntryInput; history?: { password: string; changed: number }[]; age: number }[] {
  if (switches.demo) return demoPmSeeds(now);
  return [
    {
      age: 120,
      input: {
        title: 'Posta personale', username: 'mario.rossi@example.it', password: 'Gufo-Lampada-Remo-81', urls: ['https://mail.example.it'],
        totp: 'otpauth://totp/Posta:mario.rossi@example.it?secret=JBSWY3DPEHPK3PXP&issuer=Posta', folder: 'Personale', tags: ['email'], favorite: true,
        notes: 'Recupero: numero di cellulare.',
      },
      history: [{ password: 'vecchia-posta-2023', changed: now - 200 * DAY }],
    },
    {
      age: 60,
      input: {
        title: 'Banca del Borgo', username: '00412789', password: 'x7#Lq9!vR2@pZ4', urls: ['https://homebanking.example.it'],
        totp: 'otpauth://totp/Banca%20del%20Borgo:00412789?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Banca%20del%20Borgo&algorithm=SHA256&digits=8&period=60',
        folder: 'Personale/Banche', tags: ['banca', 'importante'], favorite: true,
        fields: [{ name: 'Codice cliente', value: '00412789', hidden: false }, { name: 'PIN dispositivo', value: '48213', hidden: true }],
      },
    },
    { age: 300, input: { title: 'Wi-Fi di casa', password: 'ciliegia-mulino-faro-12', folder: 'Casa', notes: 'Rete: Casa-Rossi-5G' } },
    {
      age: 15,
      input: {
        title: 'Server di lavoro', username: 'mrossi', password: 'S3rv3r!Lavoro#2024', urls: ['https://vpn.example.com', 'https://git.example.com'],
        totp: 'JBSWY3DPEHPK3PXP', folder: 'Lavoro', tags: ['lavoro'], fields: [{ name: 'Passphrase chiave SSH', value: 'rondine sentiero vela', hidden: true }],
      },
    },
    { age: 30, input: { title: 'Film in streaming', username: 'famiglia.rossi@example.it', password: 'Popcorn&Divano99', urls: ['https://www.streaming.example.com'], tags: ['condiviso'] } },
    { age: 5, input: { title: 'SPID', username: 'mario.rossi@example.it', password: 'Identita-Digitale-55!', urls: ['https://login.example.gov.it'], folder: 'Personale', tags: ['importante'] } },
  ];
}

const GEN_SITES = ['posta', 'banca', 'forum', 'negozio', 'cloud', 'giochi', 'viaggi', 'lavoro', 'scuola', 'salute', 'news', 'musica'];
const GEN_FOLDERS = ['', 'Personale', 'Personale/Banche', 'Lavoro', 'Lavoro/Clienti', 'Casa', 'Svago', ''];

/** Deterministic filler entry `i` (perf runs with thousands of entries). */
export function generated(i: number, now: number): { input: PmEntryInput; created: number; updated: number } {
  const site = GEN_SITES[i % GEN_SITES.length];
  const n = String(i + 1).padStart(4, '0');
  return {
    input: {
      title: `${site[0].toUpperCase()}${site.slice(1)} ${n}`,
      username: `utente${i % 37}@example.it`,
      password: `pw-${site}-${n}`,
      urls: [`https://${site}${i % 50}.example.com/login`],
      folder: GEN_FOLDERS[i % GEN_FOLDERS.length],
      tags: i % 5 === 0 ? ['importante'] : [],
      favorite: i % 23 === 0,
      totp: i % 9 === 0 ? 'JBSWY3DPEHPK3PXP' : undefined,
    },
    created: now - (i % 900) * DAY,
    updated: now - (i % 300) * DAY,
  };
}

function cleanFolder(f: string): string {
  return f.split(/[/\\]/).map((x) => x.trim()).filter(Boolean).join('/');
}

/** Mirrors passwords::icon_host: a real DNS name from the first http(s) website. */
export function iconHost(urls: string[]): string | null {
  for (const u of urls) {
    const m = /^(?:(https?):\/\/)?(?:[^@/]*@)?([^/:?#]+)/i.exec(u.trim());
    if (!m || (/^[a-z]+:\/\//i.test(u.trim()) && !m[1])) continue;
    const h = m[2].toLowerCase().replace(/\.$/, '');
    if (!h.includes('.') || /^[\d.]+$/.test(h) || /\.(local|localhost|internal)$/.test(h) || !/^[a-z0-9.-]+$/.test(h)) continue;
    return h;
  }
  return null;
}

// --- the mock ---------------------------------------------------------------------------

export class PmMock {
  private stores = new Map<string, Map<string, StoredEntry>>();
  private idSeq = 0;
  private jobSeq = 0;
  /** what pm_copy put on the clipboard (tests read it; '' after the auto-clear) */
  clipboard: string | null = null;

  constructor(private host: PmMockHost) {}

  private iconUrls = new Map<string, string>();

  /**
   * The mock's "stored icon": a 64 px PNG tile with the host's initial (data: URL, no network,
   * drawn once per host like the real app's decoded PNGs; SVG where there is no canvas).
   */
  iconUrl(icon: string): string {
    const host = icon.split('/')[0] ?? '';
    const hit = this.iconUrls.get(host);
    if (hit) return hit;
    let h = 0;
    for (const c of host) h = (h * 31 + c.charCodeAt(0)) % 360;
    const letter = (host.replace(/^www\./, '')[0] ?? '?').toUpperCase();
    let url: string;
    const canvas = typeof document !== 'undefined' ? document.createElement('canvas') : null;
    const g = canvas?.getContext?.('2d');
    if (canvas && g) {
      canvas.width = canvas.height = 64;
      g.fillStyle = `hsl(${h} 55% 45%)`;
      g.beginPath();
      g.roundRect(0, 0, 64, 64, 14);
      g.fill();
      g.fillStyle = '#fff';
      g.font = '700 30px sans-serif';
      g.textAlign = 'center';
      g.fillText(letter, 32, 43);
      url = canvas.toDataURL('image/png');
    } else {
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" rx="14" fill="hsl(${h} 55% 45%)"/><text x="32" y="43" font-family="sans-serif" font-size="30" font-weight="700" text-anchor="middle" fill="#fff">${letter}</text></svg>`;
      url = `data:image/svg+xml,${encodeURIComponent(svg)}`;
    }
    this.iconUrls.set(host, url);
    return url;
  }

  private store(vault: unknown): Map<string, StoredEntry> {
    const path = this.host.pathOf(String(vault));
    let st = this.stores.get(path);
    if (!st) {
      st = new Map();
      this.stores.set(path, st);
      if (this.host.seeded(path)) {
        const now = Date.now();
        for (const s of seeds(now)) {
          const e = this.build(s.input, now - s.age * DAY);
          e.s.updated = now - Math.floor(s.age / 3) * DAY;
          e.history = s.history ?? [];
          st.set(e.s.id, e);
        }
        const n = this.host.extra?.() ?? 0;
        for (let i = 0; i < n; i++) {
          const g = generated(i, now);
          const e = this.build(g.input, g.created);
          e.s.updated = g.updated;
          st.set(e.s.id, e);
        }
      }
    }
    return st;
  }

  private entry(vault: unknown, id: unknown): StoredEntry {
    const e = this.store(vault).get(String(id));
    if (!e) throw err('not_found', 'Entry not found.');
    return e;
  }

  private build(input: PmEntryInput, created = Date.now()): StoredEntry {
    const e: StoredEntry = {
      s: { id: `pm${++this.idSeq}`, title: '', username: '', urls: [], folder: '', tags: [], favorite: false, has_password: false, has_totp: false, created, updated: created, icon: null },
      password: '',
      notes: '',
      totp: null,
      fields: [],
      history: [],
    };
    this.apply(e, input);
    return e;
  }

  /** Mirrors nv_core::passwords::apply. Returns the replaced password, if any. */
  private apply(e: StoredEntry, p: PmEntryInput): string | null {
    const s = e.s;
    const title = p.title !== undefined ? p.title.trim() : s.title;
    if (!title) throw err('invalid_entry', 'Invalid entry: title is required.');
    let totp = e.totp;
    if (p.totp !== undefined) {
      totp = p.totp.trim() ? p.totp.trim() : null;
      if (totp) parseTotp(totp); // throws invalid_totp
    }
    const uniq = (v: string[]) => [...new Set(v.map((x) => x.trim()).filter(Boolean))];
    s.title = title;
    if (p.username !== undefined) s.username = p.username;
    if (p.urls !== undefined) s.urls = uniq(p.urls);
    if (p.tags !== undefined) s.tags = uniq(p.tags);
    if (p.folder !== undefined) s.folder = p.folder.split(/[/\\]/).map((x) => x.trim()).filter(Boolean).join('/');
    if (p.favorite !== undefined) s.favorite = p.favorite;
    if (p.notes !== undefined) e.notes = p.notes;
    e.totp = totp;
    if (p.fields !== undefined) {
      e.fields = p.fields.map((f) => {
        const name = f.name.trim();
        if (!name) throw err('invalid_entry', 'Invalid entry: custom field without a name.');
        const value = f.value ?? e.fields.find((o) => o.name === name)?.value ?? '';
        return { name, value, hidden: !!f.hidden };
      });
    }
    let old: string | null = null;
    if (p.password !== undefined && p.password !== e.password) {
      old = e.password || null;
      e.password = p.password;
    }
    s.has_password = !!e.password;
    s.has_totp = !!e.totp;
    return old;
  }

  private detail(e: StoredEntry): PmEntry {
    let totp: PmTotpInfo | null = null;
    if (e.totp) {
      const t = parseTotp(e.totp);
      totp = { issuer: t.issuer, account: t.account, algorithm: t.algorithm, digits: t.digits, period: t.period };
    }
    return {
      ...structuredClone(e.s),
      notes: e.notes,
      fields: e.fields.map((f) => ({ name: f.name, hidden: f.hidden, value: f.hidden ? null : f.value })),
      totp,
      history_count: e.history.length,
    };
  }

  private sorted(st: Map<string, StoredEntry>): PmEntrySummary[] {
    return [...st.values()]
      .map((e) => structuredClone(e.s))
      .sort((a, b) => a.title.toLowerCase().localeCompare(b.title.toLowerCase()) || a.id.localeCompare(b.id));
  }

  private async reveal(e: StoredEntry, field: string): Promise<string> {
    const nf = () => err('not_found', 'Entry not found.');
    let v: string | null | undefined;
    const m = /^(field|history):(\d+)$/.exec(field);
    if (field === 'password') v = e.password || null;
    else if (field === 'username') return e.s.username;
    else if (field === 'notes') return e.notes;
    else if (field === 'totp') v = e.totp;
    else if (field === 'totp_code') v = e.totp ? (await totpCode(parseTotp(e.totp))).code : null;
    else if (m?.[1] === 'field') v = e.fields[Number(m[2])]?.value || null;
    else if (m?.[1] === 'history') v = [...e.history].sort((a, b) => b.changed - a.changed)[Number(m[2])]?.password;
    else throw err('invalid_field', `Unknown field: ${field}`);
    if (!v) throw nf();
    return v;
  }

  private pushHistory(e: StoredEntry, old: string, when: number) {
    e.history.push({ password: old, changed: when });
    e.history.sort((a, b) => b.changed - a.changed);
    e.history = e.history.slice(0, HISTORY_KEEP);
  }

  private startImport(vault: VaultId, a: Record<string, unknown>): JobId {
    const st = this.store(vault);
    const kind = String(a.kind);
    const data = typeof a.data === 'string' ? a.data : null;
    const path = typeof a.path === 'string' && a.path.trim() ? a.path : null;
    if (!['bitwarden', 'keepass', 'csv'].includes(kind)) throw err('invalid_import', `Unknown format: ${kind}`);
    if (!data && !path) throw err('invalid_import', 'No file to import.');
    const job = `pmjob-${++this.jobSeq}`;
    const done = (error: ApiError | null, imported: number) => this.host.emit('job-done', { job, error, imported } as JobDone);
    setTimeout(async () => {
      let items: Imported[];
      try {
        this.host.emit('job-progress', { job, done_bytes: 0, total_bytes: 0, current: '' });
        await this.host.wait(kind === 'keepass' ? 600 : 150);
        if (kind === 'keepass') {
          if (a.master_password !== 'demo') throw err('wrong_password', 'Wrong KeePass database password or key file.');
          items = cannedKeepass();
        } else if (data) {
          items = kind === 'csv' ? parseCsv(data) : parseBitwarden(data);
        } else {
          // a host path in the browser: canned content
          items = kind === 'csv' ? parseCsv('name,url,username,password\nForum,https://forum.example.org,mario,forum-pw-1\n') : cannedKeepass().slice(0, 2);
        }
      } catch (e) {
        done(e as ApiError, 0);
        return;
      }
      let n = 0;
      for (const it of items) {
        try {
          const urls = it.input.urls ?? [];
          if (!it.input.title?.trim()) it.input.title = hostOf(urls[0] ?? '') || it.input.username?.trim() || 'Senza titolo';
          if (it.input.totp) {
            try {
              parseTotp(it.input.totp);
            } catch {
              it.input.fields = [...(it.input.fields ?? []), { name: 'TOTP', value: it.input.totp, hidden: true }];
              it.input.totp = undefined;
            }
          }
          tidyFields(it);
          const e = this.build(it.input, it.created ?? Date.now());
          e.s.updated = Math.max(it.updated ?? it.created ?? e.s.created, e.s.created);
          for (const h of it.history ?? []) if (h.password && h.password !== e.password) this.pushHistory(e, h.password, h.changed);
          st.set(e.s.id, e);
          n++;
        } catch (e) {
          done(e as ApiError, n);
          return;
        }
        this.host.emit('job-progress', { job, done_bytes: n, total_bytes: items.length, current: it.input.title ?? '' });
        await this.host.wait(40);
      }
      done(null, n);
    }, 0);
    return job;
  }

  async invoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
    switch (cmd) {
      case 'pm_list':
        await this.host.wait(40);
        return this.sorted(this.store(a.vault));
      case 'pm_search': {
        await this.host.wait(20);
        const terms = String(a.query ?? '').toLowerCase().split(/\s+/).filter(Boolean);
        return this.sorted(this.store(a.vault)).filter((s) => {
          const hay = [s.title, s.username, ...s.urls, ...s.tags, s.folder].join('\n').toLowerCase();
          return terms.every((t) => hay.includes(t));
        });
      }
      case 'pm_get':
        return this.detail(this.entry(a.vault, a.id));
      case 'pm_create': {
        await this.host.wait(60);
        const st = this.store(a.vault);
        const e = this.build((a.entry ?? {}) as PmEntryInput);
        st.set(e.s.id, e);
        return this.detail(e);
      }
      case 'pm_update': {
        await this.host.wait(60);
        const e = this.entry(a.vault, a.id);
        const draft: StoredEntry = structuredClone(e);
        const now = Math.max(Date.now(), e.s.updated);
        const old = this.apply(draft, (a.entry ?? {}) as PmEntryInput);
        draft.s.updated = now;
        if (old) this.pushHistory(draft, old, now);
        Object.assign(e, draft);
        return this.detail(e);
      }
      case 'pm_delete': {
        await this.host.wait(40);
        if (!this.store(a.vault).delete(String(a.id))) throw err('not_found', 'Entry not found.');
        return null;
      }
      case 'pm_history': {
        const e = this.entry(a.vault, a.id);
        return [...e.history].sort((x, y) => y.changed - x.changed).map((h, index): PmHistoryItem => ({ index, changed: h.changed }));
      }
      case 'pm_reveal':
        return this.reveal(this.entry(a.vault, a.id), String(a.field));
      case 'pm_copy': {
        const value = await this.reveal(this.entry(a.vault, a.id), String(a.field));
        this.clipboard = value;
        try {
          await navigator.clipboard?.writeText(value);
        } catch {
          /* no clipboard permission in a plain browser tab: the mock keeps its own copy */
        }
        const after = Math.min(Math.max(0, a.clear_after == null ? 30 : Number(a.clear_after)), 600);
        if (after > 0) {
          setTimeout(async () => {
            if (this.clipboard !== value) return;
            this.clipboard = '';
            try {
              if ((await navigator.clipboard?.readText()) === value) await navigator.clipboard.writeText('');
            } catch {
              /* ignore */
            }
          }, after * 1000 * Math.max(this.host.speed(), 0.001));
        }
        const r: PmCopied = { clear_after: after };
        return r;
      }
      case 'pm_totp': {
        const e = this.entry(a.vault, a.id);
        if (!e.totp) throw err('not_found', 'Entry not found.');
        return totpCode(parseTotp(e.totp));
      }
      case 'pm_totp_check': {
        const t = parseTotp(String(a.totp ?? ''));
        const c = await totpCode(t);
        const r: PmTotpCheck = { issuer: t.issuer, account: t.account, algorithm: t.algorithm, ...c };
        return r;
      }
      case 'pm_generate':
        return generate((a.opts as PmGenOptions | null) ?? { kind: 'random' });
      case 'pm_import':
        this.host.pathOf(String(a.vault));
        return this.startImport(String(a.vault), a);
      case 'pm_move': {
        await this.host.wait(30);
        const folder = cleanFolder(String(a.folder ?? ''));
        let n = 0;
        for (const id of (a.ids as string[]) ?? []) {
          const e = this.store(a.vault).get(String(id));
          if (e && e.s.folder !== folder) {
            e.s.folder = folder;
            n++;
          }
        }
        return n;
      }
      case 'pm_folder_rename':
      case 'pm_folder_delete': {
        await this.host.wait(30);
        const from = cleanFolder(String(cmd === 'pm_folder_rename' ? a.from : a.folder));
        const to = cmd === 'pm_folder_rename' ? cleanFolder(String(a.to ?? '')) : from.split('/').slice(0, -1).join('/');
        if (!from || (cmd === 'pm_folder_rename' && !to)) throw err('invalid_entry', 'Invalid entry: folder name is required.');
        if (from === to) return 0;
        let n = 0;
        for (const e of this.store(a.vault).values()) {
          const f = e.s.folder;
          if (f === from || f.startsWith(`${from}/`)) {
            e.s.folder = [to, f.slice(from.length + 1)].filter(Boolean).join('/');
            n++;
          }
        }
        if (!n) throw err('not_found', 'Entry not found.');
        return n;
      }
      case 'pm_repair': {
        await this.host.wait(30);
        let n = 0;
        for (const e of this.store(a.vault).values()) {
          const it: Imported = { input: { fields: e.fields.map((f) => ({ ...f })), urls: [...e.s.urls] } };
          const before = e.fields.length;
          tidyFields(it);
          if ((it.input.fields?.length ?? 0) === before) continue;
          e.fields = (it.input.fields ?? []).map((f) => ({ name: f.name, value: f.value ?? '', hidden: !!f.hidden }));
          e.s.urls = it.input.urls ?? e.s.urls;
          if (it.created) e.s.created = it.created;
          if (it.updated) e.s.updated = Math.max(it.updated, e.s.created);
          n++;
        }
        return n;
      }
      case 'pm_icons_refresh': {
        const st = this.store(a.vault);
        if (this.host.siteIcons && !this.host.siteIcons()) return 0;
        const due = [...st.values()].filter((e) => !e.s.icon && iconHost(e.s.urls));
        if (!due.length) return 0;
        const vault = String(a.vault);
        setTimeout(async () => {
          await this.host.wait(300);
          const v = Date.now();
          for (const e of due) e.s.icon = `${iconHost(e.s.urls)}/${v}`;
          this.host.emit('pm-icons', { vault, done: due.length, total: due.length });
        }, 0);
        return new Set(due.map((e) => iconHost(e.s.urls))).size;
      }
      default:
        throw err('unknown_command', `Unknown command: ${cmd}`);
    }
  }
}
