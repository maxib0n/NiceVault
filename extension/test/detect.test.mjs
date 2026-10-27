import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { otpLike, userLike } from '../src/lib/detect.js';

test('OTP fields', () => {
  assert.ok(otpLike({ autocomplete: 'one-time-code' }));
  assert.ok(otpLike({ type: 'text', inputmode: 'numeric', maxlength: '6' }));
  assert.ok(otpLike({ type: 'tel', maxlength: '8' }));
  assert.ok(otpLike({ type: 'text', name: 'totp' }));
  assert.ok(otpLike({ type: 'text', id: 'otp-code', maxlength: '6' }));
  assert.ok(!otpLike({ type: 'text', name: 'username' }));
  assert.ok(!otpLike({ type: 'email', name: 'otp' }));
  assert.ok(!otpLike({ type: 'text', inputmode: 'numeric', maxlength: '16', name: 'card' }));
  assert.ok(!otpLike({ type: 'text', autocomplete: 'username', name: 'code' }));
});

test('user-name fields', () => {
  assert.ok(userLike({ type: 'email' }));
  assert.ok(userLike({ type: 'text', name: 'login' }));
  assert.ok(userLike({ type: 'text', autocomplete: 'username' }));
  assert.ok(!userLike({ type: 'checkbox' }));
  assert.ok(!userLike({ type: 'text', name: 'q', id: 'search' }));
  assert.ok(!userLike({ type: 'text', autocomplete: 'one-time-code' }));
});

test('the build: one source, two manifests', () => {
  const here = dirname(fileURLToPath(import.meta.url));
  execFileSync(process.execPath, [join(here, '..', 'build.mjs')]);
  const c = JSON.parse(readFileSync(join(here, '../dist/chrome/manifest.json'), 'utf8'));
  const f = JSON.parse(readFileSync(join(here, '../dist/firefox/manifest.json'), 'utf8'));
  assert.equal(c.manifest_version, 3);
  assert.equal(c.background.service_worker, 'background.js');
  assert.ok(c.key);
  assert.deepEqual(f.background.scripts, ['background.js']);
  assert.equal(f.browser_specific_settings.gecko.id, 'browser@nicevault.app');
  assert.ok(!f.key);
  for (const m of [c, f]) {
    assert.ok(m.permissions.includes('nativeMessaging'));
    assert.ok(!('externally_connectable' in m));
    assert.match(m.content_security_policy.extension_pages, /connect-src 'none'/);
  }
  const content = readFileSync(join(here, '../dist/chrome/content.js'), 'utf8');
  assert.ok(!/^\s*(import|export)\b/m.test(content), 'content script is a classic script');
});
