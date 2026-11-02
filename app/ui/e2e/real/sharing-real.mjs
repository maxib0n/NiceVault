import { readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { KEY, WebDriver } from './webdriver.mjs';

const OUT = process.env.NV_E2E_OUT;
const WORK = process.env.NV_E2E_WORK;
const PASSWORD = 'cavallo batteria graffetta corretta';
const KEY_PASSWORD = 'lanterna pendolo ortica 1917';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const LIB = `
  const norm = (s) => (s || '').replace(/\\s+/g, ' ').trim();
  const accName = (el) => norm(el.getAttribute('aria-label') || el.textContent);
  const btn = (name) => [...document.querySelectorAll('button,[role=button]')].find((b) => accName(b) === name);
  const tid = (id) => document.querySelector('[data-testid="' + id + '"]');
  const screen = () => document.querySelector('[data-screen]')?.dataset.screen;
  const byLabel = (text) => { const l = [...document.querySelectorAll('dialog[open] label, main label')].find((l) => norm(l.textContent) === text); return l ? document.getElementById(l.htmlFor) : null; };
`;
const wd = new WebDriver(process.env.NV_E2E_DRIVER);
const q = (body, ...args) => wd.exec(`${LIB}\n${body}`, ...args);
async function waitFor(desc, body, { timeout = 20000, args = [] } = {}) {
  const t0 = Date.now();
  let last;
  while (Date.now() - t0 < timeout) {
    try {
      last = await q(body, ...args);
      if (last) return last;
    } catch (e) {
      last = e.message;
    }
    await sleep(150);
  }
  throw new Error(`timeout waiting for ${desc} (last: ${JSON.stringify(last)})`);
}
const clickButton = async (name) => wd.click(await waitFor(`button ${name}`, 'return btn(arguments[0])', { args: [name] }));
const clickTid = async (id) => wd.click(await waitFor(`[data-testid=${id}]`, 'const e = tid(arguments[0]); return e && !e.disabled ? e : null', { args: [id] }));
let n = 0;
const shot = async (name) => {
  await sleep(400);
  writeFileSync(join(OUT, `sharing-${String(++n).padStart(2, '0')}-${name}.png`), await wd.screenshot());
};
const ok = (cond, what) => {
  if (!cond) throw new Error(`FAIL: ${what}`);
  console.log(`ok   ${what}`);
};

await wd.start({ 'tauri:options': { application: process.env.NV_E2E_APP } });
await waitFor('app ready', 'return document.querySelector("[data-ready=true]")');

await clickButton('Nuovo volume');
await waitFor('wizard', 'return screen() === "create"');
await clickButton('Avanti');
await wd.sendKeys(await waitFor('password', 'return byLabel("Password")'), PASSWORD);
await wd.sendKeys(await waitFor('confirm', 'return byLabel("Ripeti la password")'), PASSWORD);
await waitFor('strong accepted', 'return btn("Crea volume") && !btn("Crea volume").disabled');
await clickButton('Crea volume');
await waitFor('step 3', 'return document.querySelector("[data-step=\\"3\\"]")', { timeout: 60000 });
await clickButton('Apri il volume');
await waitFor('vault', 'return screen() === "vault"');

await wd.chord([KEY.Control], ',');
await waitFor('settings', 'return screen() === "settings" && tid("mykey")?.dataset.state === "none"');
await q('tid("settings-mykey").scrollIntoView({ block: "center" })');
await clickTid('mykey-create');
await wd.sendKeys(await waitFor('key password', 'return byLabel("Password della chiave")'), KEY_PASSWORD);
await wd.sendKeys(await waitFor('repeat', 'return byLabel("Ripeti")'), KEY_PASSWORD);
await clickTid('mykey-create-submit');
await waitFor('key ready', 'return tid("mykey")?.dataset.state === "ready"', { timeout: 60000 });
const pub = await q('return tid("mykey-public").dataset.public');
ok(/^nvpk1[A-Za-z0-9_-]{1627}$/.test(pub), 'public key nvpk1… (1632 characters)');
const fpr = (await q('return tid("mykey-fingerprint").textContent')).replace(/\s+/g, '');
ok(/^[0-9a-f]{32}$/.test(fpr), `fingerprint ${fpr}`);
await q('tid("settings-mykey").scrollIntoView({ block: "center" })');
await shot('settings-mykey');
const file = join(WORK, 'home', '.config', 'NiceVault', 'my-key.json');
const stored = JSON.parse(readFileSync(file, 'utf8'));
ok(stored.public === pub && typeof stored.key_file === 'string', 'my-key.json holds the public key and the .nvkey');
ok((statSync(file).mode & 0o777) === 0o600, 'my-key.json is 0600');
ok(!readFileSync(file, 'utf8').includes(KEY_PASSWORD), 'the key password is not stored');
await clickTid('settings-back');
await waitFor('vault again', 'return screen() === "vault"');

await clickButton('Dettagli');
await waitFor('details', 'return screen() === "details"');
await clickTid('add-recipient');
const area = await waitFor('key field', 'return tid("rcp-key")');
await wd.sendKeys(area, 'nvpk1questa-non-e-una-chiave');
const err = await waitFor('junk refused', 'return tid("rcp-key-error")?.textContent');
ok(/non è una chiave pubblica/i.test(err), `junk key refused: ${err.trim()}`);
await wd.clear(area);
await clickTid('rcp-use-mine');
await waitFor('fingerprint shown', 'return tid("rcp-fingerprint")?.textContent.includes("La tua chiave")');
await shot('add-recipient');
await clickTid('rcp-submit');
await waitFor('mine in the list', 'return tid("slot-mine")');
await shot('slots');

await clickButton('Blocca');
await waitFor('unlock', 'return screen() === "unlock"');
const keyField = await waitFor('key password field', 'return byLabel("Password della mia chiave")', { timeout: 20000 });
await shot('unlock-with-key');
await wd.sendKeys(keyField, PASSWORD);
await wd.keys(KEY.Enter);
const wrong = await waitFor('wrong key password', 'return document.querySelector("#unlock-error")?.textContent', { timeout: 60000 });
ok(/password errata/i.test(wrong), `wrong key password: ${wrong.trim()}`);
await wd.sendKeys(await waitFor('field again', 'return byLabel("Password della mia chiave")'), KEY_PASSWORD);
await wd.keys(KEY.Enter);
await waitFor('vault with the key', 'return screen() === "vault"', { timeout: 60000 });
ok(true, 'opened with my key');
await clickButton('Dettagli');
await waitFor('in use', 'return document.querySelector("[data-kind=recipient][data-in-use=true]")');
ok(true, 'the recipient slot is the one in use');
await wd.quit().catch(() => undefined);
console.log('sharing-real: all good');
process.exit(0);
