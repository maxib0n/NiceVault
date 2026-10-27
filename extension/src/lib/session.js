import { b64, pairCommit, pairKey, pairKeys, random, sas, sessionKeys, unb64 } from './crypto.js';

export const HOST = 'app.nicevault.bridge';
export const PROTO = 1;
const enc = new TextEncoder();
const dec = new TextDecoder();

function later(f, ms) {
  const t = setTimeout(f, ms);
  t?.unref?.();
  return t;
}

const MISSING = /not found|no such native application|forbidden|not allowed|native messaging host/i;

export class Session {
  constructor(o) {
    this.o = o;
    this.port = null;
    this.sess = null;
    this.state = 'idle';
    this.app = null;
    this.waiters = [];
    this.sendChain = Promise.resolve();
    this.recvChain = Promise.resolve();
    this.ensuring = null;
  }

  setState(s) {
    if (s !== this.state) {
      this.state = s;
      this.o.onState?.(s);
    }
    return s;
  }

  open() {
    if (this.port) return this.port;
    let p;
    try {
      p = this.o.connect();
    } catch {
      this.setState('missing');
      return null;
    }
    this.port = p;
    p.onMessage.addListener((m) => this.onMessage(m));
    p.onDisconnect.addListener(() => this.closed(this.o.lastError?.() ?? p.error?.message ?? ''));
    return p;
  }

  post(m) {
    this.open()?.postMessage(m);
  }

  closed(err) {
    this.port = null;
    this.dropSession();
    const missing = MISSING.test(err || '');
    for (const w of this.waiters.splice(0)) w.resolve({ closed: true, missing });
    this.setState(missing ? 'missing' : 'not_running');
  }

  dropSession() {
    const s = this.sess;
    this.sess = null;
    if (s) for (const [, p] of s.pending) p.reject({ code: 'closed' });
  }

  disconnect() {
    try {
      this.port?.disconnect();
    } catch {
    }
    this.closed('');
  }

  onMessage(m) {
    if (m && m.t === 'm') {
      this.recvChain = this.recvChain.then(() => this.openFrame(m.c));
      return;
    }
    if (m && m.bridge === 'app_closed') {
      this.dropSession();
      this.setState('not_running');
    }
    if (m && m.t === 'unpaired' && this.sess) {
      this.dropSession();
      this.setState('unpaired');
    }
    const i = this.waiters.findIndex((w) => w.pred(m));
    if (i >= 0) this.waiters.splice(i, 1)[0].resolve(m);
  }

  wait(pred, ms) {
    return new Promise((resolve) => {
      const w = { pred, resolve };
      this.waiters.push(w);
      later(() => {
        const i = this.waiters.indexOf(w);
        if (i >= 0) {
          this.waiters.splice(i, 1);
          resolve({ timeout: true });
        }
      }, ms);
    });
  }

  async bridge(cmd, ms = 4000) {
    const want = cmd === 'launch' ? 'launched' : 'state';
    const r = this.wait((m) => m && (m.bridge === want || m.bridge === 'unsupported'), ms);
    this.post({ bridge: cmd });
    return r;
  }

  async openFrame(c) {
    const s = this.sess;
    if (!s) return;
    const p = await s.keys.recv.open(unb64(c));
    if (!p) {
      this.disconnect();
      return;
    }
    let m;
    try {
      m = JSON.parse(dec.decode(p));
    } catch {
      return;
    }
    if (m.id != null && s.pending.has(m.id)) {
      const w = s.pending.get(m.id);
      s.pending.delete(m.id);
      w.resolve(m);
    } else this.o.onPush?.(m);
  }

  ensure({ launch = false } = {}) {
    if (this.sess) return Promise.resolve('ready');
    if (!this.ensuring) {
      this.ensuring = this.connectSession(launch).finally(() => {
        this.ensuring = null;
      });
    }
    return this.ensuring;
  }

  async connectSession(launch) {
    if (!this.open()) return this.state;
    const st = await this.bridge('hello');
    if (st.closed || st.timeout) return this.setState(st.missing || st.timeout ? 'missing' : 'not_running');
    if (!st.app) {
      if (!launch) return this.setState('not_running');
      this.setState('starting');
      const r = await this.bridge('launch', 12000);
      if (!r.ok) return this.setState(r.reason === 'no_app' ? 'missing' : 'launch_failed');
    }
    const pair = await this.o.store.get();
    if (!pair) return this.setState('unpaired');
    return this.hello(pair);
  }

  async hello(pair) {
    const nE = random(16);
    const r = this.wait((m) => m && (m.t === 'hello_ok' || m.t === 'unpaired' || m.bridge === 'not_running'), 6000);
    this.post({ t: 'hello', pair_id: pair.pair_id, n: b64(nE) });
    const m = await r;
    if (m.closed) return this.state;
    if (m.timeout || m.bridge === 'not_running') return this.setState('not_running');
    if (m.t === 'unpaired') return this.setState('unpaired');
    const keys = await sessionKeys(pair.key, nE, unb64(m.n));
    const info = await keys.recv.open(unb64(m.c));
    if (!info) return this.setState('unpaired');
    this.app = JSON.parse(dec.decode(info));
    const [lo, hi] = this.app.proto ?? [1, 1];
    if (hi < PROTO) return this.setState('version_app');
    if (lo > PROTO) return this.setState('version_ext');
    this.sess = { keys, pending: new Map(), next: 1 };
    return this.setState('ready');
  }

  async request(type, payload = {}, { launch = false, timeout = 20000 } = {}) {
    const st = await this.ensure({ launch });
    if (st !== 'ready') throw { code: st };
    const s = this.sess;
    const id = s.next++;
    const body = enc.encode(JSON.stringify({ v: PROTO, id, type, ...payload }));
    const reply = new Promise((resolve, reject) => {
      s.pending.set(id, { resolve, reject });
      later(() => {
        if (s.pending.delete(id)) reject({ code: 'timeout' });
      }, timeout);
    });
    this.sendChain = this.sendChain.then(async () => {
      const c = await s.keys.send.seal(body);
      if (this.sess === s) this.post({ t: 'm', c: b64(c) });
    });
    const r = await reply;
    if (!r.ok) throw r.error ?? { code: 'error' };
    return r;
  }

  async pair(label, onCode) {
    if (!this.open()) return this.state;
    const st = await this.bridge('hello');
    if (st.closed || st.timeout) return st.missing || st.timeout ? 'missing' : 'not_running';
    if (!st.app) {
      const r = await this.bridge('launch', 12000);
      if (!r.ok) return 'not_running';
    }
    let keys;
    try {
      keys = await pairKeys();
    } catch {
      return 'unsupported';
    }
    const w = this.wait((m) => m && (m.t === 'pair_wait' || m.t === 'pair_refused' || m.bridge === 'not_running'), 10000);
    this.post({ t: 'pair_begin', commit: b64(await pairCommit(keys.pub)), browser: label });
    const a = await w;
    if (a.t !== 'pair_wait') return a.code ?? (a.closed && a.missing ? 'missing' : 'not_running');
    const epkA = unb64(a.epk);
    if (epkA.length !== 32) return 'bad_key';
    const done = this.wait((m) => m && (m.t === 'pair_done' || m.t === 'pair_refused' || m.bridge === 'app_closed'), 135000);
    this.post({ t: 'pair_reveal', epk: b64(keys.pub) });
    onCode(await sas(keys.pub, epkA));
    const d = await done;
    if (d.t !== 'pair_done') return d.code ?? 'pair_refused';
    const key = await pairKey(keys.kp, keys.pub, epkA);
    await this.o.store.set({ pair_id: d.pair_id, key, created: Date.now() });
    this.dropSession();
    await this.ensure();
    return 'paired';
  }
}
