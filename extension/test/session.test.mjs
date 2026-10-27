import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Half, b64, concat, hkdfBits, importPairKey, pairCommit, pairKeys, random, sas, unb64 } from '../src/lib/crypto.js';
import { Session } from '../src/lib/session.js';

const enc = new TextEncoder();
const dec = new TextDecoder();
const subtle = globalThis.crypto.subtle;

function memStore() {
  let v = null;
  return { get: async () => v, set: async (x) => void (v = x), clear: async () => void (v = null), peek: () => v };
}

function fakeApp({ running = true, missing = false, accept = true } = {}) {
  const app = { running, pairs: new Map(), code: null, launches: 0, frames: 0 };
  const listeners = { msg: [], disc: [] };
  let send = null;
  let recv = null;
  let chain = Promise.resolve();
  const port = {
    onMessage: { addListener: (f) => listeners.msg.push(f) },
    onDisconnect: { addListener: (f) => listeners.disc.push(f) },
    disconnect() {},
    postMessage: (m) => setTimeout(() => (chain = chain.then(() => handle(m))), 1),
  };
  const out = (m) => listeners.msg.forEach((f) => f(m));
  async function sealOut(obj) {
    out({ t: 'm', c: b64(await send.seal(enc.encode(JSON.stringify(obj)))) });
  }
  async function handle(m) {
    if (m.bridge === 'hello') return out({ bridge: 'state', app: app.running, version: '0.3.2', os: 'linux' });
    if (m.bridge === 'launch') {
      app.launches++;
      app.running = true;
      return out({ bridge: 'launched', ok: true });
    }
    if (!app.running) return out({ bridge: 'not_running' });
    if (m.t === 'pair_begin') {
      if (!m.commit) return out({ t: 'pair_refused', code: 'old_extension' });
      app.pending = { commit: m.commit, a: await pairKeys() };
      return out({ t: 'pair_wait', epk: b64(app.pending.a.pub) });
    }
    if (m.t === 'pair_reveal') {
      const p = app.pending;
      app.pending = null;
      const epkE = unb64(m.epk);
      if (!p || b64(await pairCommit(epkE)) !== p.commit) return out({ t: 'pair_refused', code: 'bad_commit' });
      const a = p.a;
      app.code = await sas(epkE, a.pub);
      if (!accept) return setTimeout(() => out({ t: 'pair_refused', code: 'pair_refused' }), 5);
      const peer = await subtle.importKey('raw', epkE, { name: 'X25519' }, false, []);
      const shared = new Uint8Array(await subtle.deriveBits({ name: 'X25519', public: peer }, a.kp.privateKey, 256));
      const id = 'p' + app.pairs.size;
      app.pairs.set(id, await hkdfBits(shared, concat(epkE, a.pub), 'nv-browser-pair-v1'));
      return setTimeout(() => out({ t: 'pair_done', pair_id: id }), 5);
    }
    if (m.t === 'hello') {
      const k = app.pairs.get(m.pair_id);
      if (!k) return out({ t: 'unpaired' });
      const K = await importPairKey(k);
      const nA = random(16);
      const salt = concat(unb64(m.n), nA);
      const d = (info) => subtle.deriveKey({ name: 'HKDF', hash: 'SHA-256', salt, info: enc.encode(info) }, K, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
      recv = new Half(await d('nv-session-v1 e2a'));
      send = new Half(await d('nv-session-v1 a2e'));
      const c = await send.seal(enc.encode(JSON.stringify({ app_version: '0.3.2', proto: app.proto ?? [1, 1], portable: false, label: 'x' })));
      return out({ t: 'hello_ok', n: b64(nA), c: b64(c) });
    }
    if (m.t === 'm') {
      const p = await recv.open(unb64(m.c));
      if (!p) return out({ t: 'bad_frame' });
      app.frames++;
      const req = JSON.parse(dec.decode(p));
      if (req.type === 'fail') return sealOut({ id: req.id, ok: false, error: { code: 'mismatch' } });
      if (req.type === 'push') await sealOut({ type: 'volumes_changed' });
      return sealOut({ id: req.id, ok: true, echo: req.type, n: req.n });
    }
  }
  const connect = () => {
    if (missing) {
      setTimeout(() => listeners.disc.forEach((f) => f()), 1);
    }
    return port;
  };
  return { app, connect, lastError: () => (missing ? 'Specified native messaging host not found.' : undefined) };
}

test('no bridge: missing', async () => {
  const f = fakeApp({ missing: true });
  const s = new Session({ connect: f.connect, lastError: f.lastError, store: memStore() });
  assert.equal(await s.ensure(), 'missing');
});

test('closed app: not launched without a gesture, launched with one, then unpaired', async () => {
  const f = fakeApp({ running: false });
  const s = new Session({ connect: f.connect, lastError: f.lastError, store: memStore() });
  assert.equal(await s.ensure(), 'not_running');
  assert.equal(f.app.launches, 0);
  assert.equal(await s.ensure({ launch: true }), 'unpaired');
  assert.equal(f.app.launches, 1);
});

test('pair, then requests in order, pushes, errors', async () => {
  const f = fakeApp();
  const store = memStore();
  const pushes = [];
  const s = new Session({ connect: f.connect, lastError: f.lastError, store, onPush: (m) => pushes.push(m) });
  let shown = null;
  assert.equal(await s.pair('Chrome · Linux', (c) => (shown = c)), 'paired');
  assert.equal(shown, f.app.code);
  assert.equal(s.state, 'ready');
  assert.ok(store.peek().key instanceof CryptoKey);
  assert.equal(store.peek().key.extractable, false);
  const rs = await Promise.all([1, 2, 3, 4, 5].map((n) => s.request('status', { n })));
  assert.deepEqual(rs.map((r) => r.n), [1, 2, 3, 4, 5]);
  assert.equal(f.app.frames, 5);
  await s.request('push');
  assert.deepEqual(pushes, [{ type: 'volumes_changed' }]);
  await assert.rejects(s.request('fail'), { code: 'mismatch' });
  const s2 = new Session({ connect: f.connect, lastError: f.lastError, store });
  assert.equal((await s2.request('status')).echo, 'status');
});

test('refused pairing stores nothing; a forgotten pairing becomes unpaired', async () => {
  const f = fakeApp({ accept: false });
  const store = memStore();
  const s = new Session({ connect: f.connect, lastError: f.lastError, store });
  assert.equal(await s.pair('Firefox', () => {}), 'pair_refused');
  assert.equal(store.peek(), null);
  const k = await importPairKey(new Uint8Array(32));
  await store.set({ pair_id: 'gone', key: k });
  assert.equal(await s.ensure(), 'unpaired');
  assert.equal(store.peek().pair_id, 'gone');
});

test('app too old for this extension', async () => {
  const f = fakeApp();
  f.app.proto = [2, 3];
  const store = memStore();
  const s = new Session({ connect: f.connect, lastError: f.lastError, store });
  assert.equal(await s.pair('x', () => {}), 'paired');
  assert.equal(s.state, 'version_ext');
});

test('B-08: an unauthenticated unpaired keeps the key', async () => {
  const f = fakeApp();
  const store = memStore();
  const s = new Session({ connect: f.connect, lastError: f.lastError, store });
  assert.equal(await s.pair('Chrome', () => {}), 'paired');
  const kept = store.peek();
  f.app.pairs.clear();
  const s2 = new Session({ connect: f.connect, lastError: f.lastError, store });
  assert.equal(await s2.ensure(), 'unpaired');
  assert.equal(store.peek(), kept, 'key deleted by a plaintext unpaired');
  f.app.pairs.set(kept.pair_id, new Uint8Array(32).fill(7));
  const s3 = new Session({ connect: f.connect, lastError: f.lastError, store });
  assert.equal(await s3.ensure(), 'unpaired');
  assert.equal(store.peek(), kept);
  assert.equal(await s3.pair('Chrome', () => {}), 'paired');
  assert.notEqual(store.peek(), kept);
});

test('B-04: pairing commits first, reveals after the app key', async () => {
  const f = fakeApp();
  const seen = [];
  const connect = () => {
    const p = f.connect();
    const post = p.postMessage;
    p.postMessage = (m) => (seen.push(m.t ?? m.bridge), post(m));
    return p;
  };
  const s = new Session({ connect, lastError: f.lastError, store: memStore() });
  assert.equal(await s.pair('Chrome', () => {}), 'paired');
  const i = seen.indexOf('pair_begin');
  assert.ok(i >= 0 && seen.indexOf('pair_reveal') > i);
});
