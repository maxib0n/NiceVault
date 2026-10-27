const subtle = globalThis.crypto.subtle;
const enc = new TextEncoder();
const AAD = enc.encode('nv1');

export function concat(...parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

export function b64(u8) {
  let s = '';
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}

export function unb64(s) {
  return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
}

export function random(n) {
  return globalThis.crypto.getRandomValues(new Uint8Array(n));
}

export async function sas(epkE, epkA) {
  const d = new Uint8Array(await subtle.digest('SHA-256', concat(enc.encode('nv-pair'), epkE, epkA)));
  const n = new DataView(d.buffer).getUint32(0) % 1_000_000;
  return String(n).padStart(6, '0');
}

export async function pairCommit(epkE) {
  return new Uint8Array(await subtle.digest('SHA-256', concat(enc.encode('nv-pair-commit'), epkE)));
}

export async function pairKeys() {
  const kp = await subtle.generateKey({ name: 'X25519' }, false, ['deriveBits']);
  return { kp, pub: new Uint8Array(await subtle.exportKey('raw', kp.publicKey)) };
}

export function importPairKey(raw) {
  return subtle.importKey('raw', raw, 'HKDF', false, ['deriveKey', 'deriveBits']);
}

export async function hkdfBits(ikm, salt, info) {
  const base = await subtle.importKey('raw', ikm, 'HKDF', false, ['deriveBits']);
  return new Uint8Array(await subtle.deriveBits({ name: 'HKDF', hash: 'SHA-256', salt, info: enc.encode(info) }, base, 256));
}

export async function pairKey(kp, epkE, epkA) {
  const peer = await subtle.importKey('raw', epkA, { name: 'X25519' }, false, []);
  const shared = new Uint8Array(await subtle.deriveBits({ name: 'X25519', public: peer }, kp.privateKey, 256));
  const k = await hkdfBits(shared, concat(epkE, epkA), 'nv-browser-pair-v1');
  shared.fill(0);
  const key = await importPairKey(k);
  k.fill(0);
  return key;
}

export class Half {
  constructor(key) {
    this.key = key;
    this.ctr = 0n;
  }
  nonce() {
    const n = new Uint8Array(12);
    new DataView(n.buffer).setBigUint64(4, this.ctr);
    return n;
  }
  async seal(bytes) {
    const iv = this.nonce();
    this.ctr++;
    return new Uint8Array(await subtle.encrypt({ name: 'AES-GCM', iv, additionalData: AAD }, this.key, bytes));
  }
  async open(bytes) {
    const iv = this.nonce();
    try {
      const p = new Uint8Array(await subtle.decrypt({ name: 'AES-GCM', iv, additionalData: AAD }, this.key, bytes));
      this.ctr++;
      return p;
    } catch {
      return null;
    }
  }
}

export async function sessionKeys(K, nE, nA) {
  const salt = concat(nE, nA);
  const d = (info) => subtle.deriveKey({ name: 'HKDF', hash: 'SHA-256', salt, info: enc.encode(info) }, K, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
  return { send: new Half(await d('nv-session-v1 e2a')), recv: new Half(await d('nv-session-v1 a2e')) };
}

export async function supported() {
  try {
    await pairKeys();
    return true;
  } catch {
    return false;
  }
}
