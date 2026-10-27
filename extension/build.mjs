import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync, readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC = join(HERE, 'src');
const OUT = join(HERE, 'dist');
const VERSION = JSON.parse(readFileSync(join(HERE, '../app/ui/package.json'), 'utf8')).version;
const CHROME_KEY =
  'MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAshPvdwB5preSAQ/sbfJYuqsNRwmTn1lycMcsJjk/BYtxaH6wQByPuCnpVtECItMULCdNwyjE54jG/dLiRlPBR+gH//LSGhaNAYhRUoUUcQfl9iz4BPSnPUaBWd0xcBhbqjR2mhxvkX7xMYQauH9WY38Q7QNJzkbxDxQTDcSw/GywmwWcc2ALZCvvppAfZ3grvlNiwDofvlpY2IW5OcFOIrmMEChSa/kqeba/xeVavKKiHq244gJSm7Y1pt27DseRC91hdFFhBtuP7K4/tiN/c+cgIiA/TrprskHAtbBWXEJDep4FBYzE7M/O/baM3Idi+ggmieS0UPFFxmJrGkE0AwIDAQAB';

const messages = Object.fromEntries(['it', 'en'].map((l) => [l, JSON.parse(readFileSync(join(HERE, 'messages', `${l}.json`), 'utf8'))]));
for (const k of Object.keys(messages.it)) if (!(k in messages.en)) throw new Error(`messages/en.json lacks ${k}`);
for (const k of Object.keys(messages.en)) if (!(k in messages.it)) throw new Error(`messages/it.json lacks ${k}`);

function manifest(target) {
  const icons = { 16: 'icons/icon-16.png', 32: 'icons/icon-32.png', 48: 'icons/icon-48.png', 128: 'icons/icon-128.png' };
  const m = {
    manifest_version: 3,
    name: '__MSG_name__',
    description: '__MSG_description__',
    default_locale: 'en',
    version: VERSION,
    icons,
    permissions: ['nativeMessaging', 'storage', 'contextMenus', 'activeTab'],
    host_permissions: ['<all_urls>'],
    content_scripts: [{ matches: ['<all_urls>'], js: ['content.js'], all_frames: true, match_about_blank: true, run_at: 'document_idle' }],
    action: { default_popup: 'popup.html', default_title: 'NiceVault', default_icon: { 16: 'icons/icon-16.png', 32: 'icons/icon-32.png' } },
    options_ui: { page: 'options.html', open_in_tab: true },
    commands: {
      'open-menu': { suggested_key: { default: 'Alt+Shift+N' }, description: '__MSG_cmd_open__' },
      'fill-totp': { description: '__MSG_cmd_totp__' },
    },
    web_accessible_resources: [{ resources: ['menu.html'], matches: ['<all_urls>'] }],
    content_security_policy: { extension_pages: "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; object-src 'none'; connect-src 'none'" },
  };
  if (target === 'chrome') {
    m.key = CHROME_KEY;
    m.minimum_chrome_version = '133';
    m.background = { service_worker: 'background.js', type: 'module' };
  } else {
    m.background = { scripts: ['background.js'], type: 'module' };
    m.browser_specific_settings = { gecko: { id: 'browser@nicevault.app', strict_min_version: '130.0' } };
  }
  return m;
}

function build(target) {
  const out = join(OUT, target);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  cpSync(SRC, out, { recursive: true, filter: (p) => !p.endsWith('content.js') || p.includes('/lib/') });
  const detect = readFileSync(join(SRC, 'lib/detect.js'), 'utf8').replace(/^export /gm, '');
  const main = readFileSync(join(SRC, 'content.js'), 'utf8');
  writeFileSync(join(out, 'content.js'), `(() => {\n'use strict';\n${detect}\n${main}\n})();\n`);
  writeFileSync(join(out, 'messages.js'), `export const MESSAGES = ${JSON.stringify(messages)};\n`);
  for (const [l, msgs] of Object.entries(messages)) {
    const dir = join(out, '_locales', l);
    mkdirSync(dir, { recursive: true });
    const pick = Object.fromEntries(['name', 'description', 'cmd_open', 'cmd_totp'].map((k) => [k, { message: msgs[k] }]));
    writeFileSync(join(dir, 'messages.json'), JSON.stringify(pick, null, 2));
  }
  writeFileSync(join(out, 'manifest.json'), JSON.stringify(manifest(target), null, 2));
  if (process.argv.includes('--zip')) {
    const zip = join(OUT, `nicevault-${target}-${VERSION}.zip`);
    rmSync(zip, { force: true });
    execFileSync('zip', ['-qr', zip, ...readdirSync(out)], { cwd: out });
  }
  return out;
}

for (const t of ['chrome', 'firefox']) console.log(`built ${build(t)}`);
