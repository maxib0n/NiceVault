import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Half, b64, concat, hkdfBits, importPairKey, pairCommit, pairKey, pairKeys, sas, sessionKeys, unb64 } from '../src/lib/crypto.js';

const fill = (n, v) => new Uint8Array(n).fill(v);
const hex = (u8) => Buffer.from(u8).toString('hex');

test('known vectors shared with the app', async () => {
  assert.equal(await sas(fill(32, 1), fill(32, 2)), '865608');
  assert.equal(Buffer.from(await pairCommit(fill(32, 1))).toString('hex'), '8fc259ebc18ccfd1c3ec58d6e636035165c06478c63a6af56290c1c77f1f7aa0');
  const k = await hkdfBits(fill(32, 5), concat(fill(32, 1), fill(32, 2)), 'nv-browser-pair-v1');
  assert.equal(hex(k), '1ac778692ad09bc098c4b65cc3a1e61f168bc001b6cf4e4ba6ae4d75ee6673f0');
  const K = await importPairKey(fill(32, 9));
  const { send } = await sessionKeys(K, fill(16, 3), fill(16, 4));
  assert.equal(hex(await send.seal(new TextEncoder().encode('{"id":1}'))), '2bd9518e99be3319d469c9e58fa1b33040df96a0f022738c');
});

test('both sides of a pairing agree, frames open once and in order', async () => {
  const e = await pairKeys();
  const a = await pairKeys();
  const ke = await pairKey(e.kp, e.pub, a.pub);
  assert.ok(ke);
  const subtle = globalThis.crypto.subtle;
  const peer = await subtle.importKey('raw', e.pub, { name: 'X25519' }, false, []);
  const shared = new Uint8Array(await subtle.deriveBits({ name: 'X25519', public: peer }, a.kp.privateKey, 256));
  const kApp = await importPairKey(await hkdfBits(shared, concat(e.pub, a.pub), 'nv-browser-pair-v1'));
  const nE = fill(16, 7);
  const nA = fill(16, 8);
  const ext = await sessionKeys(ke, nE, nA);
  const app = await sessionKeys(kApp, nE, nA);
  const appSend = new Half(app.recv.key);
  const appRecv = new Half(app.send.key);
  const f1 = await ext.send.seal(new TextEncoder().encode('one'));
  const f2 = await ext.send.seal(new TextEncoder().encode('two'));
  assert.equal(await appRecv.open(f2), null, 'out of order');
  assert.equal(new TextDecoder().decode(await appRecv.open(f1)), 'one');
  assert.equal(await appRecv.open(f1), null, 'replay');
  const forged = f2.slice();
  forged[0] ^= 1;
  assert.equal(await appRecv.open(forged), null, 'forged');
  assert.equal(new TextDecoder().decode(await appRecv.open(f2)), 'two');
  const back = await appSend.seal(new TextEncoder().encode('ok'));
  assert.equal(new TextDecoder().decode(await ext.recv.open(back)), 'ok');
});

test('base64 round trip of large buffers', () => {
  const big = new Uint8Array(200_000).map((_, i) => i & 0xff);
  assert.deepEqual(unb64(b64(big)), big);
});
