import { test } from 'node:test';
import assert from 'node:assert/strict';
import { allowedFrom, originOf } from '../src/lib/origin.js';
import { acceptFill, styleIntact } from '../src/lib/detect.js';

const EXT = 'chrome-extension://npegemkakgbadljnibnipnaleoffjfbf/';
const ID = 'npegemkakgbadljnibnipnaleoffjfbf';

test('B-02: senders are sorted by role', () => {
  const content = { id: ID, tab: { id: 1 }, frameId: 0, url: 'https://evil.example/', origin: 'https://evil.example' };
  const menu = { id: ID, tab: { id: 1 }, frameId: 3, url: `${EXT}menu.html`, origin: EXT.slice(0, -1) };
  const popup = { id: ID, url: `${EXT}popup.html`, origin: EXT.slice(0, -1) };
  const options = { id: ID, tab: { id: 7 }, frameId: 0, url: `${EXT}options.html`, origin: EXT.slice(0, -1) };
  const other = { id: 'someoneelse', url: 'chrome-extension://someoneelse/x.html' };
  for (const k of ['frame_fields', 'open_menu']) assert.ok(allowedFrom(k, content, EXT, ID), k);
  for (const k of ['popup_search', 'popup_view', 'popup_fill', 'popup_copy', 'popup_unlock', 'menu_fill', 'menu_view', 'menu_unlock', 'menu_launch', 'pair_start', 'show_app', 'unpair_local', 'forget_memory', 'get_state']) {
    assert.ok(!allowedFrom(k, content, EXT, ID), `content script may not send ${k}`);
  }
  for (const k of ['menu_view', 'menu_search', 'menu_fill', 'menu_launch', 'menu_unlock', 'menu_resize', 'menu_close', 'show_app']) assert.ok(allowedFrom(k, menu, EXT, ID), k);
  for (const k of ['popup_fill', 'popup_copy', 'unpair_local', 'pair_start', 'frame_fields']) assert.ok(!allowedFrom(k, menu, EXT, ID), `menu may not send ${k}`);
  for (const k of ['popup_view', 'popup_search', 'popup_fill', 'popup_copy', 'popup_unlock', 'show_app', 'get_state']) assert.ok(allowedFrom(k, popup, EXT, ID), k);
  for (const k of ['frame_fields', 'open_menu', 'menu_fill']) assert.ok(!allowedFrom(k, popup, EXT, ID), `popup may not send ${k}`);
  for (const k of ['unpair_local', 'forget_memory', 'get_state', 'pair_start']) assert.ok(allowedFrom(k, options, EXT, ID), k);
  assert.ok(!allowedFrom('get_state', other, EXT, ID), 'another extension');
  assert.ok(!allowedFrom('nonsense', popup, EXT, ID));
  assert.ok(!allowedFrom('popup_fill', { id: ID, tab: { id: 1 }, url: `https://evil.example/${EXT}popup.html` }, EXT, ID));
});

test('B-03: an opaque origin stays opaque', () => {
  assert.equal(originOf({ origin: 'null', url: 'https://bank.example/u/x.html' }), 'null');
  assert.equal(originOf({ url: 'https://bank.example/u/x.html' }, 'null'), 'null', 'Firefox: the frame says it is opaque');
  assert.equal(originOf({ origin: 'https://bank.example', url: 'https://bank.example/x' }, 'null'), 'null');
  assert.equal(originOf({ origin: 'https://bank.example', url: 'https://bank.example/x' }), 'https://bank.example');
  assert.equal(originOf({ url: 'https://bank.example/x' }, 'https://bank.example'), 'https://bank.example');
  assert.equal(originOf({ url: 'https://bank.example/x' }, 'https://evil.example'), 'https://bank.example', 'a reported origin never widens');
  assert.equal(originOf({ url: 'data:text/html,x' }), 'null');
  assert.equal(originOf({}), 'null');
});

test('B-06: a fill lands only in the origin it was asked for', () => {
  assert.ok(acceptFill({ expect: 'https://bank.example' }, 'https://bank.example'));
  assert.ok(!acceptFill({ expect: 'https://bank.example' }, 'https://evil.example'));
  assert.ok(!acceptFill({}, 'https://bank.example'));
  assert.ok(!acceptFill({ expect: 'null' }, 'null'));
});

test('B-01: styles that hide or distort the menu are refused', () => {
  const ok = { opacity: '1', filter: 'none', clipPath: 'none', maskImage: 'none', mask: 'none', transform: 'none', zoom: '1', mixBlendMode: 'normal', visibility: 'visible', display: 'block' };
  assert.ok(styleIntact(ok, true));
  for (const [k, v] of [['opacity', '0.99'], ['filter', 'opacity(0)'], ['clipPath', 'inset(50%)'], ['maskImage', 'linear-gradient(transparent, transparent)'], ['transform', 'matrix(0.02, 0, 0, 0.02, 0, 0)'], ['zoom', '0.05'], ['mixBlendMode', 'multiply'], ['visibility', 'hidden'], ['display', 'none']]) {
    assert.ok(!styleIntact({ ...ok, [k]: v }, true), `${k}: ${v}`);
  }
  assert.ok(styleIntact({ ...ok, visibility: 'hidden' }, false));
  assert.ok(!styleIntact({ ...ok, transform: 'scale(0.1)' }, false));
});
