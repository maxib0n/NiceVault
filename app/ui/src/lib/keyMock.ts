import type { ApiError, MyKey, RecipientCheck } from './types';

export const KEY_PASSWORD = 'chiave';
const BODY_LEN = 1627;
const B64URL = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';

function err(code: string, message: string): ApiError {
  return { code, message };
}

export function samplePublic(seed: string): string {
  let h = 2166136261;
  for (const c of seed) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0;
  let out = 'nvpk1';
  for (let i = 0; i < BODY_LEN; i++) {
    h = Math.imul(h ^ (h >>> 15), 2246822519) >>> 0;
    h = (h + 0x9e3779b9) >>> 0;
    out += B64URL[h & 63];
  }
  return out;
}

export function fingerprintOf(publicKey: string): string {
  let out = '';
  let h = 0x811c9dc5;
  for (let round = 0; out.length < 32; round++) {
    for (let i = 0; i < publicKey.length; i++) h = Math.imul(h ^ publicKey.charCodeAt(i) ^ round, 16777619) >>> 0;
    out += h.toString(16).padStart(8, '0');
  }
  return out.slice(0, 32);
}

export function parsePublic(text: string): string {
  if (text.trimStart().startsWith('NVKEY001')) throw err('secret_key_file', 'This is a secret key file (.nvkey).');
  const s = text.replace(/\s+/g, '');
  const body = s.startsWith('nvpk1') ? s.slice(5) : null;
  if (body === null || body.length !== BODY_LEN || !/^[A-Za-z0-9_-]+$/.test(body)) throw err('invalid_recipient', 'This is not a valid NiceVault public key (nvpk1…).');
  return s;
}

interface Mine {
  public: string;
  created_ms: number;
  password: string;
}

type Exported = { kind: 'nvkey'; public: string; password: string } | { kind: 'nvpub'; public: string };

export interface KeyMockOptions {
  seed: 'mine' | 'shared' | null;
  installed: boolean;
}

export class KeyMock {
  mine: Mine | null = null;
  readonly files = new Map<string, Exported>();
  private persisted: boolean;

  constructor(opts: KeyMockOptions) {
    this.persisted = opts.installed;
    if (opts.seed) this.mine = { public: samplePublic('mine'), created_ms: Date.UTC(2026, 9, 12, 9, 30), password: KEY_PASSWORD };
  }

  view(): MyKey {
    const m = this.mine;
    return { public: m?.public ?? null, fingerprint: m ? fingerprintOf(m.public) : null, created_ms: m?.created_ms ?? null, persisted: this.persisted };
  }

  private check(pub: string): RecipientCheck {
    return { public: pub, fingerprint: fingerprintOf(pub) };
  }

  private write(dest: string, f: Exported) {
    if (this.files.has(dest)) throw err('exists', 'A file with this name already exists.');
    this.files.set(dest, f);
  }

  handles(cmd: string): boolean {
    return cmd.startsWith('my_key_') || cmd === 'recipient_check' || cmd === 'recipient_read_file';
  }

  async invoke(cmd: string, a: Record<string, unknown>, wait: (ms: number) => Promise<void>): Promise<unknown> {
    switch (cmd) {
      case 'my_key_get':
        return this.view();
      case 'my_key_create': {
        const pw = String(a.password ?? '');
        if (!pw) throw err('invalid_password', 'A password is needed.');
        if (this.mine) throw err('exists', 'There is already a personal key on this computer.');
        await wait(900);
        this.mine = { public: samplePublic(`mine-${Date.now()}-${Math.random()}`), created_ms: Date.now(), password: pw };
        return this.view();
      }
      case 'my_key_import': {
        const path = String(a.path);
        const pw = String(a.password ?? '');
        const f = this.files.get(path);
        if ((f && f.kind !== 'nvkey') || (!f && !/\.nvkey$/i.test(path))) throw err('not_a_key', 'This file is not a NiceVault key file (.nvkey).');
        await wait(900);
        const pub = f?.kind === 'nvkey' ? f.public : samplePublic(`file:${path}`);
        if (pw !== (f?.kind === 'nvkey' ? f.password : KEY_PASSWORD)) throw err('wrong_password', 'Wrong password for this key file.');
        if (this.mine) {
          if (this.mine.public === pub) return this.view();
          throw err('exists', 'There is already a different personal key on this computer.');
        }
        this.mine = { public: pub, created_ms: Date.now(), password: pw };
        return this.view();
      }
      case 'my_key_export': {
        if (!this.mine) throw err('no_key', 'There is no personal key on this computer.');
        this.write(String(a.dest), { kind: 'nvkey', public: this.mine.public, password: this.mine.password });
        return null;
      }
      case 'my_key_export_public': {
        if (!this.mine) throw err('no_key', 'There is no personal key on this computer.');
        this.write(String(a.dest), { kind: 'nvpub', public: this.mine.public });
        return null;
      }
      case 'my_key_remove':
        this.mine = null;
        return this.view();
      case 'recipient_check':
        return this.check(parsePublic(String(a.text ?? '')));
      case 'recipient_read_file': {
        const path = String(a.path);
        const f = this.files.get(path);
        if (f?.kind === 'nvkey' || (!f && /\.nvkey$/i.test(path))) throw err('secret_key_file', 'This is a secret key file (.nvkey).');
        const name = path.split(/[\\/]/).pop() ?? path;
        return this.check(f?.public ?? samplePublic(name.replace(/\.[^.]*$/, '')));
      }
    }
    throw err('unknown_command', `Unknown command: ${cmd}`);
  }
}
