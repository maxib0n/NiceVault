import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from 'playwright';
import { WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const XDO = process.env.NV_E2E_XDO ?? 'xdotool';
const HOME = process.env.HOME;
const ROOT = new URL('../../../../', import.meta.url).pathname;
const EXT_CHROME = join(ROOT, 'extension/dist/chrome');
const EXT_FIREFOX = join(ROOT, 'extension/dist/firefox');
const CHROME_ID = 'npegemkakgbadljnibnipnaleoffjfbf';
const FF_UUID = '6f1b8d52-2a1e-4c39-9a57-6b0d1e2c3a44';
const PW = 'segreto-browser';
const VOLUME = join(WORK, 'host', 'Personale.nvault');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let failed = 0;
function check(ok, msg) {
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${msg}`);
  return ok;
}
async function until(fn, timeout = 10000, every = 150) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeout) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
    }
    await sleep(every);
  }
  return null;
}

function pidsOf(hosts) {
  const out = [];
  for (const p of readdirSync('/proc')) {
    if (!/^\d+$/.test(p)) continue;
    try {
      const a = readFileSync(`/proc/${p}/cmdline`, 'utf8').split('\0');
      const host = a[1]?.startsWith('chrome-extension://') || a[2] === 'browser@nicevault.app';
      if (a[0] === APP && host === hosts) out.push(Number(p));
    } catch {
    }
  }
  return out;
}
const appPids = () => pidsOf(false);
const hostPids = () => pidsOf(true);
const OUT = process.env.NV_E2E_OUT ?? WORK;
function shot(name) {
  spawnSync('import', ['-window', 'root', join(OUT, `browser-${name}.png`)]);
}
const xwins = () => spawnSync(XDO, ['search', '--onlyvisible', '--class', '^nicevault$']).stdout.toString().split('\n').filter(Boolean);
function compactWindow() {
  for (const id of xwins()) {
    const g = spawnSync(XDO, ['getwindowgeometry', id]).stdout.toString().match(/Geometry: (\d+)x(\d+)/);
    if (g && Number(g[1]) < 600 && Number(g[2]) < 600 && Number(g[1]) > 200) return id;
  }
  return null;
}
function typeInto(win, text) {
  spawnSync(XDO, ['windowraise', win]);
  spawnSync(XDO, ['mousemove', '--window', win, '220', '40']);
  spawnSync(XDO, ['windowfocus', '--sync', win]);
  spawnSync(XDO, ['type', '--delay', '30', text]);
  spawnSync(XDO, ['key', 'Return']);
}

let wd = null;
async function startApp() {
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await until(() => wd.exec('return !!document.querySelector("[data-ready=true]")'), 30000);
}
const invoke = (cmd, args = {}) =>
  wd.execAsync('const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]).then((v) => done({ v }), (e) => done({ err: e }));', cmd, args).then((r) => {
    if (r.err) throw new Error(`${cmd}: ${JSON.stringify(r.err)}`);
    return r.v;
  });

const LOGIN = `<!doctype html><meta charset=utf-8><title>Login</title>
<form id=f action="/session" method=post onsubmit="window.__submitted = true; return false">
<input id=u name=username autocomplete=username style="width:320px;height:36px;display:block;margin:20px">
<input id=p name=password type=password style="width:320px;height:36px;display:block;margin:20px">
<button>Accedi</button></form>`;
const OTP = `<!doctype html><meta charset=utf-8><title>2FA</title>
<form onsubmit="window.__submitted = true; return false"><input id=otp autocomplete=one-time-code inputmode=numeric maxlength=6 style="width:240px;height:36px;display:block;margin:20px"></form>`;
const SHOP = `<!doctype html><meta charset=utf-8><title>Shop</title><p>Negozio</p>
<iframe id=fr src="https://accounts.nvtest.example/frame" style="width:420px;height:220px;border:0;display:block;margin:20px"></iframe>`;
const SITE = {
  'https://login.nvtest.example/': LOGIN,
  'https://login.nvtest.example/otp': OTP,
  'http://login.nvtest.example/': LOGIN,
  'https://www.paypa1.com/': LOGIN,
  'https://shop.othersite.example/': SHOP,
  'https://accounts.nvtest.example/frame': LOGIN,
};

async function routeSite(ctx) {
  await ctx.route(/^https?:\/\//, (route) => {
    const u = route.request().url();
    const body = SITE[u];
    return body ? route.fulfill({ status: 200, contentType: 'text/html; charset=utf-8', body }) : route.fulfill({ status: 404, body: '' });
  });
}

async function iconClick(page, sel, frameSel = null) {
  const scope = frameSel ? page.frameLocator(frameSel) : page;
  await until(() => (frameSel ? page.frames().find((f) => f.url().includes('/frame')) : page.mainFrame()).evaluate(() => !!document.querySelector('nicevault-ui')), 8000);
  await sleep(500);
  const box = await scope.locator(sel).boundingBox();
  await page.mouse.click(box.x + box.width - 17, box.y + box.height / 2);
}

async function menuFrame(page) {
  const f = await until(() => page.frames().find((x) => x.url().startsWith(`chrome-extension://${CHROME_ID}/menu.html`)), 8000);
  if (!f) return null;
  await until(() => f.evaluate(() => document.getElementById('root')?.dataset.state !== 'loading'), 15000);
  await sleep(450);
  return f;
}
const menuState = (f) => f.evaluate(() => document.getElementById('root').dataset.state);

async function main() {
  mkdirSync(join(WORK, 'host'), { recursive: true });
  execFileSync(process.execPath, [join(ROOT, 'extension/build.mjs')], { stdio: 'inherit' });

  mkdirSync(join(HOME, '.config', 'chromium'), { recursive: true });
  await startApp();
  await invoke('browser_set', { patch: { link: true } });
  const reg = await until(async () => {
    const i = await invoke('browser_info');
    return i.registered.length ? i : null;
  }, 15000);
  check(!!reg && reg.bridge === APP, `the manifests name the app itself: ${reg?.bridge}`);
  check(!!reg && reg.registered.some((r) => r.browser === 'Google Chrome') && reg.registered.some((r) => r.browser === 'Chromium') && reg.registered.some((r) => r.browser === 'Firefox'), `manifests registered: ${reg?.registered.map((r) => r.browser).join(', ')}`);
  const chromeManifest = join(HOME, '.config/chromium/NativeMessagingHosts/app.nicevault.bridge.json');
  const m = existsSync(chromeManifest) ? JSON.parse(readFileSync(chromeManifest, 'utf8')) : {};
  check(m.path === APP && m.allowed_origins?.[0] === `chrome-extension://${CHROME_ID}/`, 'the Chromium manifest names the app and only our extension');
  check(existsSync(join(WORK, 'run/nicevault/bridge.sock')), 'the app listens on $XDG_RUNTIME_DIR/nicevault/bridge.sock');

  const v = await invoke('vault_create', { path: VOLUME, password: PW, kdf: 'fast' });
  const add = (entry) => invoke('pm_create', { vault: v.id, entry });
  await add({ title: 'Login NV', username: 'mario@nvtest.example', password: 'pw-login-1', urls: ['https://login.nvtest.example'], totp: 'JBSWY3DPEHPK3PXP' });
  await add({ title: 'Accounts', username: 'mario.acc', password: 'pw-accounts', urls: ['https://accounts.nvtest.example'] });
  await add({ title: 'PayPal', username: 'mario@paypal', password: 'pw-paypal', urls: ['https://www.paypal.com'] });
  await invoke('browser_set', { patch: { default_volume: VOLUME } });

  const udd = join(HOME, '.config', 'chromium');
  for (const d of [join(HOME, '.config/google-chrome-for-testing/NativeMessagingHosts')]) {
    mkdirSync(d, { recursive: true });
    copyFileSync(chromeManifest, join(d, 'app.nicevault.bridge.json'));
  }
  const ctx = await chromium.launchPersistentContext(udd, {
    headless: false,
    locale: 'it-IT',
    viewport: { width: 1100, height: 760 },
    args: [`--disable-extensions-except=${EXT_CHROME}`, `--load-extension=${EXT_CHROME}`, '--no-first-run', '--no-default-browser-check'],
  });
  await routeSite(ctx);
  const sw = ctx.serviceWorkers()[0] ?? (await ctx.waitForEvent('serviceworker', { timeout: 15000 }));
  check(sw.url().startsWith(`chrome-extension://${CHROME_ID}/`), `extension loaded with the expected id (${sw.url()})`);

  const pair = await ctx.newPage();
  await pair.goto(`chrome-extension://${CHROME_ID}/pair.html`);
  await pair.getByRole('button', { name: 'Collega' }).click();
  const code = await until(async () => (await pair.getByTestId('pair-code').textContent())?.trim(), 15000);
  const appCode = await until(() => wd.exec('const e = document.querySelector("[data-testid=browser-pair-code]"); return e ? e.textContent.trim() : null'), 15000);
  check(!!code && code === appCode, `the same 6 digits in the browser and in NiceVault (${code} / ${appCode})`);
  await wd.click(await wd.find('[data-testid=browser-pair-accept]'));
  const res = await until(async () => (await pair.locator('body').getAttribute('data-result')) ?? null, 15000);
  check(res === 'paired', `paired (${res})`);
  const hp = hostPids();
  check(hp.length >= 1 && readFileSync(`/proc/${hp[0]}/cmdline`, 'utf8').split('\0')[1] === `chrome-extension://${CHROME_ID}/`, `Chromium runs the app binary as its host (pid ${hp.join(', ')}, no window)`);
  const paired = await invoke('browser_info');
  check(paired.paired.length === 1 && /Chrom/i.test(paired.paired[0].label), `NiceVault lists the browser: ${paired.paired[0]?.label}`);
  const pairs = JSON.parse(readFileSync(join(HOME, '.config/NiceVault/browsers.json'), 'utf8'));
  check(pairs.browsers.length === 1 && (spawnSync('stat', ['-c', '%a', join(HOME, '.config/NiceVault/browsers.json')]).stdout.toString().trim() === '600'), 'browsers.json holds the pairing, 0600');
  await pair.close();

  const page = await ctx.newPage();
  await page.goto('https://login.nvtest.example/');
  await iconClick(page, '#p');
  let f = await menuFrame(page);
  check(!!f && (await menuState(f)) === 'matches', `the menu shows the matches (${f && (await menuState(f))})`);
  const rows = f ? await f.locator('[data-testid=row]').evaluateAll((els) => els.map((e) => e.dataset.title)) : [];
  check(rows[0] === 'Login NV' && !rows.includes('PayPal'), `only this site's logins: ${rows.join(', ')}`);
  await f.locator('[data-testid=row]').first().click();
  await until(async () => (await page.inputValue('#p')) === 'pw-login-1', 8000);
  check((await page.inputValue('#u')) === 'mario@nvtest.example' && (await page.inputValue('#p')) === 'pw-login-1', 'user name and password filled');
  check(!(await page.evaluate(() => window.__submitted === true)), 'the form was not submitted');
  check(!page.frames().some((x) => x.url().includes('menu.html')), 'the menu closed after filling');

  await page.goto('https://login.nvtest.example/otp');
  await iconClick(page, '#otp');
  f = await menuFrame(page);
  check(!!f && (await f.getByTestId('row-totp').count()) === 1, 'the OTP field offers "Codice 2FA di Login NV"');
  if (f) await f.getByTestId('row-totp').click();
  const otp = await until(async () => {
    const x = await page.inputValue('#otp');
    return /^\d{6}$/.test(x) ? x : null;
  }, 8000);
  check(!!otp, `a 6-digit code filled (${otp})`);

  await page.goto('https://shop.othersite.example/');
  await iconClick(page, '#p', '#fr');
  f = await menuFrame(page);
  const note = f ? await f.locator('.note').first().textContent().catch(() => '') : '';
  check(/accounts\.nvtest\.example.*shop\.othersite\.example/.test(note ?? ''), `the menu names the frame and the page: ${note}`);
  if (f) await f.locator('[data-testid=row][data-title=Accounts]').click();
  const inFrame = page.frames().find((x) => x.url().includes('/frame'));
  check(!!(await until(async () => (await inFrame.inputValue('#p')) === 'pw-accounts', 8000)), 'filled inside the iframe');

  await page.goto('https://www.paypa1.com/');
  await iconClick(page, '#p');
  f = await menuFrame(page);
  check(!!f && (await menuState(f)) === 'lookalike' && (await f.locator('[data-testid=row]').count()) === 0, 'paypa1.com: lookalike warning, no fill buttons');
  check(!!f && /paypal\.com/.test((await f.getByTestId('st-lookalike').textContent()) ?? ''), 'it names the real site');

  await page.goto('http://login.nvtest.example/');
  await iconClick(page, '#p');
  f = await menuFrame(page);
  check(!!f && (await f.getByTestId('st-insecure').count()) === 1, 'http page: the insecure warning');
  if (f) {
    await f.locator('[data-testid=row]').first().click();
    await f.getByRole('button', { name: 'Compila comunque' }).click();
  }
  check(!!(await until(async () => (await page.inputValue('#p')) === 'pw-login-1', 8000)), 'filled after "Compila comunque"');

  const popup = await ctx.newPage();
  await popup.goto(`chrome-extension://${CHROME_ID}/popup.html`);
  await popup.locator('.search').fill('paypal');
  await until(async () => (await popup.locator('[data-testid=row]').count()) === 1, 8000);
  await popup.getByRole('button', { name: 'Copia password' }).click();
  check(/Copiato/.test((await until(() => popup.getByTestId('flash').textContent(), 8000)) ?? ''), 'popup: "Copia password" done by the app');
  const acc = await invoke('browser_info');
  check(acc.accesses.some((a) => a.kind === 'copy' && a.title === 'PayPal') && acc.accesses.some((a) => a.kind === 'fill') && acc.accesses.some((a) => a.kind === 'totp'), `"Ultimi accessi": ${acc.accesses.map((a) => a.kind + ':' + a.title).join(', ')}`);
  await popup.close();

  await invoke('vault_lock', { vault: v.id });
  await page.goto('https://login.nvtest.example/');
  await iconClick(page, '#p');
  f = await menuFrame(page);
  check(!!f && (await menuState(f)) === 'locked', `locked: the menu says where the passwords are (${f && (await menuState(f))})`);
  const lockedText = f ? await f.getByTestId('st-locked').textContent() : '';
  check(/Personale/.test(lockedText ?? ''), `site memory: "${lockedText}"`);
  if (f) await f.getByRole('button', { name: 'Sblocca «Personale»' }).click();
  const win = await until(compactWindow, 10000);
  check(!!win, 'NiceVault opened its compact unlock window');
  check(!!f && (await until(async () => (await menuState(f)) === 'waiting', 5000)), 'the menu says to type the password in NiceVault');
  shot('compact-1');
  if (win) typeInto(win, PW);
  await sleep(800);
  shot('compact-2');
  const reopened = await until(async () => (await invoke('vault_list_open')).some((x) => x.path === VOLUME), 15000);
  check(!!reopened, 'the password typed in the compact window opened the volume');
  check(!!(await until(() => !compactWindow(), 8000)), 'the compact window closed by itself');
  check(!!f && !!(await until(async () => (await menuState(f)) === 'matches', 8000)), 'the still-open menu refreshed with the matches');

  await firefox(wd).catch((e) => check(false, `Firefox: ${e.message}`));

  await wd.quit().catch(() => {});
  wd = null;
  check(!!(await until(() => appPids().length === 0, 10000)), 'NiceVault is not running');
  await page.goto('https://login.nvtest.example/');
  await iconClick(page, '#p');
  const started = await until(() => appPids().length > 0, 15000);
  check(!!started, `the click started NiceVault through the bridge (pid ${appPids().join(', ')})`);
  const args = started ? readFileSync(`/proc/${appPids()[0]}/cmdline`, 'utf8').split('\0') : [];
  check(args.includes('--background'), `started with --background (${args.slice(1).join(' ')})`);
  f = await menuFrame(page);
  const win2 = await until(compactWindow, 20000);
  shot('launched');
  check(!!win2, 'straight into the compact unlock window (closed = locked)');
  check(!!f && !!(await until(async () => (await menuState(f)) === 'waiting', 8000)), 'the menu waits for the password typed in NiceVault');
  if (win2) typeInto(win2, PW);
  check(!!f && !!(await until(async () => (await menuState(f)) === 'matches', 20000)), 'unlocked: the menu shows the login');
  if (f && (await menuState(f)) === 'matches') {
    await page.evaluate(() => {
      document.getElementById('u').value = '';
      document.getElementById('p').value = '';
    });
    await f.locator('[data-testid=row]').first().click();
    check(!!(await until(async () => (await page.inputValue('#p')) === 'pw-login-1', 8000)), 'one click, the password, Enter: filled');
  }

  await ctx.close();
}

async function firefox() {
  const bin = ['/usr/bin/firefox', '/usr/lib64/firefox/firefox'].find((p) => existsSync(p));
  if (!bin) return console.log('skip Firefox: not installed');
  const profile = join(WORK, 'ff-profile');
  mkdirSync(profile, { recursive: true });
  const prefs = {
    'extensions.webextensions.uuids': JSON.stringify({ 'browser@nicevault.app': FF_UUID }),
    'browser.shell.checkDefaultBrowser': false,
    'datareporting.policy.dataSubmissionEnabled': false,
    'toolkit.telemetry.reportingpolicy.firstRun': false,
    'browser.startup.homepage_override.mstone': 'ignore',
    'app.update.auto': false,
    'browser.safebrowsing.malware.enabled': false,
    'browser.safebrowsing.phishing.enabled': false,
    'network.captive-portal-service.enabled': false,
    'extensions.update.enabled': false,
    'intl.locale.requested': 'it',
  };
  writeFileSync(join(profile, 'user.js'), Object.entries(prefs).map(([k, v]) => `user_pref(${JSON.stringify(k)}, ${JSON.stringify(v)});`).join('\n') + '\n');
  const port = 9300 + (process.pid % 500);
  const ff = spawn(bin, ['--headless', '--no-remote', '--profile', profile, `--remote-debugging-port=${port}`, '-remote-allow-system-access', 'about:blank'], { stdio: 'ignore', env: { ...process.env, MOZ_HEADLESS: '1' } });
  try {
    const ws = await until(async () => {
      const w = new WebSocket(`ws://127.0.0.1:${port}/session`);
      await new Promise((ok, no) => {
        w.onopen = ok;
        w.onerror = no;
      });
      return w;
    }, 30000, 500);
    if (!ws) throw new Error('no BiDi endpoint');
    let seq = 0;
    const pending = new Map();
    ws.onmessage = (e) => {
      const m = JSON.parse(e.data);
      if (m.id && pending.has(m.id)) {
        pending.get(m.id)(m);
        pending.delete(m.id);
      }
    };
    const bidi = (method, params) =>
      new Promise((ok, no) => {
        const id = ++seq;
        pending.set(id, (m) => (m.type === 'error' ? no(new Error(`${method}: ${m.error} ${m.message}`)) : ok(m.result)));
        ws.send(JSON.stringify({ id, method, params }));
      });
    await bidi('session.new', { capabilities: {} });
    const ext = await bidi('webExtension.install', { extensionData: { type: 'path', path: EXT_FIREFOX } });
    check(ext.extension === 'browser@nicevault.app', `Firefox: the add-on installed temporarily (${ext.extension})`);
    const context = await until(async () => {
      const { contexts } = await bidi('browsingContext.getTree', {});
      return contexts.find((c) => c.url.startsWith(`moz-extension://${FF_UUID}/pair.html`))?.context;
    }, 15000);
    check(!!context, 'Firefox: the install opened the pairing page');
    if (!context) return;
    await sleep(500);
    const evaluate = async (expression) => {
      const r = await bidi('script.evaluate', { expression, target: { context }, awaitPromise: true });
      return r.result?.value;
    };
    await evaluate("window.__r = null; browser.runtime.sendMessage({ kind: 'pair_start' }).then((r) => (window.__r = r.result)); 1");
    const code = await until(async () => {
      const c = await evaluate("document.getElementById('code').textContent");
      return c && c.trim() ? c.trim() : null;
    }, 20000);
    if (!code) console.log(`  Firefox pairing: ${await evaluate('String(window.__r)')} / ${await evaluate("browser.runtime.sendMessage({ kind: 'get_state' }).then((s) => s.state)")}`);
    check(existsSync(join(HOME, '.mozilla/native-messaging-hosts/app.nicevault.bridge.json')), 'Firefox: the app wrote ~/.mozilla/native-messaging-hosts');
    const appCode = await until(() => wd.exec('const e = document.querySelector("[data-testid=browser-pair-code]"); return e ? e.textContent.trim() : null'), 15000);
    check(!!code && code === appCode, `Firefox: the same code in the browser and in NiceVault (${code} / ${appCode})`);
    await wd.click(await wd.find('[data-testid=browser-pair-accept]'));
    const res = await until(() => evaluate('window.__r'), 15000);
    check(res === 'paired', `Firefox: paired (${res})`);
    const st = await evaluate("browser.runtime.sendMessage({ kind: 'get_state' }).then((s) => s.state)");
    check(st === 'ready', `Firefox: encrypted session up (${st})`);
    const found = await evaluate("browser.runtime.sendMessage({ kind: 'popup_search', query: 'login' }).then((v) => (v.entries || []).map((e) => e.title).join(','))");
    check(found === 'Login NV', `Firefox: a search answered through the session (${found})`);
    const info = await invoke('browser_info');
    check(info.paired.length === 2 && info.paired.some((p) => /Firefox/.test(p.label)), `NiceVault lists both browsers: ${info.paired.map((p) => p.label).join(' | ')}`);
    ws.close();
  } finally {
    ff.kill('SIGTERM');
    await sleep(1500);
    ff.kill('SIGKILL');
  }
}

try {
  await main();
} catch (e) {
  failed++;
  console.log(`FAIL ${e.stack ?? e}`);
} finally {
  if (wd) await wd.quit().catch(() => {});
  for (const p of [...appPids(), ...hostPids()]) {
    try {
      process.kill(p, 'SIGTERM');
    } catch {
    }
  }
  await sleep(500);
  for (const p of [...appPids(), ...hostPids()]) {
    try {
      process.kill(p, 'SIGKILL');
    } catch {
    }
  }
}
console.log(failed ? `\n${failed} check(s) FAILED` : '\nall browser-extension checks passed');
process.exit(failed ? 1 : 0);
