// Real-app suite: the Tauri shell + real nv-core/nv-mount, driven through the
// real WebKitGTK webview (tauri-driver + WebKitWebDriver) on an X server
// (Xvfb), with real GTK file dialogs and real XDND drags (xdotool + a GTK
// drag source). Started by e2e-real.sh, which provides the environment:
//   NV_E2E_APP   app binary (copied into a fresh "stick" folder)
//   NV_E2E_WORK  scratch dir (stick/, host/, export/)
//   NV_E2E_OUT   screenshot dir
//   NV_E2E_XDO   xdotool binary (LD_LIBRARY_PATH set by the caller)
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, readlinkSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { KEY, WebDriver } from './webdriver.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const XDO = process.env.NV_E2E_XDO ?? 'xdotool';
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const HOST = join(WORK, 'host');
const EXPORT = join(WORK, 'export');
const PASSWORD = 'cavallo batteria graffetta corretta';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha = (p) => createHash('sha256').update(readFileSync(p)).digest('hex');
const xdo = (...a) => execFileSync(XDO, a.map(String)).toString().trim();

let wd;
let shotN = 0;
const results = [];
const screenshots = [];

// ---------------------------------------------------------------------------
// page helpers: semantic lookups only (roles, accessible names, text, testids)

const LIB = `
  const norm = (s) => (s || '').replace(/\\s+/g, ' ').trim();
  const accName = (el) => norm(el.getAttribute('aria-label') || el.textContent);
  const btn = (name) => [...document.querySelectorAll('button,[role=button],[role=switch],[role=radio],[role=menuitem],[role=menuitemradio]')]
    .find((b) => accName(b) === name);
  const option = (name) => [...document.querySelectorAll('[role=listbox] [role=option]')]
    .find((o) => norm(o.querySelector('[data-testid=item-name]')?.textContent) === name);
  const names = () => [...document.querySelectorAll('[role=listbox] [data-testid=item-name]')].map((e) => norm(e.textContent));
  const screen = () => document.querySelector('[data-screen]')?.dataset.screen;
  const byLabel = (text) => { const l = [...document.querySelectorAll('label')].find((l) => norm(l.textContent) === text); return l ? document.getElementById(l.htmlFor) : null; };
  const dialog = (name) => [...document.querySelectorAll('dialog[open]')].find((d) => !name || norm(d.getAttribute('aria-label')).startsWith(name) || norm(d.querySelector('h2')?.textContent).startsWith(name));
  const toastTexts = () => [...document.querySelectorAll('[aria-label=Notifiche] [role=alert] > *, [aria-label=Notifiche] [role=status] > *')].map((t) => norm(t.textContent));
  const rect = (el) => { const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2, left: r.left, top: r.top, width: r.width, height: r.height }; };
`;
const q = (body, ...args) => wd.exec(`${LIB}\n${body}`, ...args);

async function waitFor(desc, body, { timeout = 15000, args = [] } = {}) {
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

async function click(desc, body, ...args) {
  const el = await waitFor(desc, body, { args });
  await wd.click(el);
}
const clickButton = (name) => click(`button "${name}"`, 'return btn(arguments[0])', name);
const clickOption = (name) => click(`item "${name}"`, 'return option(arguments[0])', name);

/** Type text into the focused element through real key events. */
async function typeText(text) {
  const actions = [];
  for (const ch of text) actions.push({ type: 'keyDown', value: ch }, { type: 'keyUp', value: ch });
  await wd.cmd('POST', '/actions', { actions: [{ type: 'key', id: 'kbd', actions }] });
  await wd.cmd('DELETE', '/actions');
}

/** Wait until finite animations (the lid ceremony between screens) are over. */
async function settled() {
  await sleep(100);
  await waitFor('transition over', 'return !document.getAnimations().some((a) => a.playState === "running" && Number.isFinite(a.effect?.getComputedTiming().endTime))', { timeout: 5000 }).catch(() => undefined);
}

async function shot(name) {
  const file = join(OUT, `${String(++shotN).padStart(2, '0')}-${name}.png`);
  writeFileSync(file, await wd.screenshot());
  screenshots.push(file);
  return file;
}
/** The whole X screen (shows GTK dialogs, the drag source, the drag in flight). */
function rootShot(name) {
  const file = join(OUT, `${String(++shotN).padStart(2, '0')}-${name}.png`);
  execFileSync('import', ['-window', 'root', file]);
  screenshots.push(file);
  return file;
}

async function step(name, fn) {
  const t0 = Date.now();
  try {
    await fn();
    results.push({ name, ok: true, ms: Date.now() - t0 });
    console.log(`ok   ${name} (${Date.now() - t0} ms)`);
  } catch (e) {
    results.push({ name, ok: false, error: e.message });
    console.log(`FAIL ${name}: ${e.message}`);
    try {
      await shot(`FAIL-${name.replace(/[^a-z0-9]+/gi, '-').slice(0, 40)}`);
    } catch {
      /* session gone */
    }
  }
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

/** fds of our WebKitWebProcess (its HOME is the suite's) open on a WebKit-Media temp file */
function webMediaFds() {
  const hits = [];
  for (const p of readdirSync('/proc').filter((x) => /^\d+$/.test(x))) {
    try {
      if (!readFileSync(`/proc/${p}/comm`, 'utf8').startsWith('WebKitWebProces')) continue;
      if (!readFileSync(`/proc/${p}/environ`, 'utf8').split('\0').includes(`HOME=${WORK}/home`)) continue;
      for (const fd of readdirSync(`/proc/${p}/fd`)) {
        try {
          const l = readlinkSync(`/proc/${p}/fd/${fd}`);
          if (l.includes('WebKit-Media')) hits.push(`${p}/fd/${fd} -> ${l}`);
        } catch {
          /* closed meanwhile */
        }
      }
    } catch {
      /* gone, or not ours */
    }
  }
  return hits;
}

// ---------------------------------------------------------------------------
// X11 helpers

function appWindow() {
  const ids = xdo('search', '--class', '^nicevault$').split('\n').filter(Boolean);
  for (const id of ids) {
    const g = xdo('getwindowgeometry', id);
    const m = g.match(/Position: (\d+),(\d+).*\n.*Geometry: (\d+)x(\d+)/s);
    if (m && Number(m[3]) > 100) return { id, x: Number(m[1]), y: Number(m[2]), w: Number(m[3]), h: Number(m[4]) };
  }
  throw new Error('app window not found');
}

async function waitWindow(name, timeout = 10000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeout) {
    try {
      const id = xdo('search', '--onlyvisible', '--name', `^${name}$`).split('\n')[0];
      if (id) return id;
    } catch {
      /* not yet */
    }
    await sleep(150);
  }
  throw new Error(`window "${name}" did not appear`);
}

async function waitWindowGone(name, timeout = 10000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeout) {
    try {
      xdo('search', '--onlyvisible', '--name', `^${name}$`);
    } catch {
      return;
    }
    await sleep(150);
  }
  throw new Error(`window "${name}" still open`);
}

/** Answer a real GTK file chooser: focus it, Ctrl+L, type the path, Enter (+ extra keys). */
async function answerFileChooser(title, path, extraKeys = []) {
  const id = await waitWindow(title);
  await sleep(400);
  xdo('windowfocus', '--sync', id);
  // leave the "Recent" view first (there a typed folder can't be accepted): Alt+Home
  xdo('key', '--clearmodifiers', 'alt+Home');
  await sleep(300);
  xdo('key', '--clearmodifiers', 'ctrl+l');
  await sleep(250);
  xdo('type', '--delay', '15', path);
  await sleep(500);
  xdo('key', 'Delete'); // drop GTK's inline completion ("/") if it selected one
  rootShot(`gtk-dialog-${title.replace(/[^a-z]+/gi, '-').toLowerCase()}`);
  xdo('key', 'Return');
  for (const k of extraKeys) {
    // only if the chooser is still up after a generous wait: a stray key would
    // land on the app and could re-trigger the button that opened the dialog
    try {
      await waitWindowGone(title, 2500);
      break;
    } catch {
      xdo('windowfocus', '--sync', id);
      xdo('key', k);
    }
  }
  try {
    await waitWindowGone(title, 4000);
  } catch (e) {
    rootShot('gtk-dialog-still-open');
    throw e;
  }
}

/** Start the GTK drag source offering `files` (text/uri-list) at screen (x, y). */
async function dragSource(x, y, files) {
  const p = spawn('python3', ['-I', join(HERE, 'drag-source.py'), String(x), String(y), ...files], { stdio: ['ignore', 'pipe', 'inherit'] });
  let out = '';
  p.stdout.on('data', (d) => (out += d));
  const t0 = Date.now();
  while (!out.includes('ready')) {
    if (Date.now() - t0 > 8000) throw new Error('drag source did not start');
    await sleep(100);
  }
  await sleep(300);
  return { proc: p, ended: () => out.includes('drag-end'), kill: () => p.kill() };
}

/** Real pointer drag from screen point a to b (XTEST events -> XDND between X clients). */
async function pointerDrag(a, b, { midway } = {}) {
  xdo('mousemove', a.x, a.y);
  await sleep(150);
  xdo('mousedown', '1');
  await sleep(150);
  const steps = 24;
  for (let i = 1; i <= steps; i++) {
    xdo('mousemove', Math.round(a.x + ((b.x - a.x) * i) / steps), Math.round(a.y + ((b.y - a.y) * i) / steps));
    await sleep(30);
  }
  // a couple of small moves on the target so the drop site gets drag-motion
  for (const d of [3, -3, 2]) {
    xdo('mousemove', b.x + d, b.y + d);
    await sleep(120);
  }
  if (midway) await midway();
  xdo('mouseup', '1');
  await sleep(400);
}

/** Viewport point (CSS px, DPR 1) -> screen point. */
function toScreen(p) {
  const w = appWindow();
  return { x: Math.round(w.x + p.x), y: Math.round(w.y + p.y) };
}

// ---------------------------------------------------------------------------

async function startApp() {
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await waitFor('app ready', 'return document.querySelector("[data-ready=true]")');
}

async function quitApp() {
  try {
    await wd?.quit();
  } catch {
    /* already gone */
  }
  wd = null;
  await sleep(1500);
}

const listing = () => q('return names()');

async function main() {
  const files = {
    photo: join(HOST, 'photo.jpg'),
    photo2: join(HOST, 'photo 2.jpg'),
    pdf: join(HOST, 'document.pdf'),
    video: join(HOST, 'clip.webm'),
    text: join(HOST, 'readme.txt'),
    drop1: join(HOST, 'dropped one.txt'),
    intoFolder: join(HOST, 'into-folder.txt'),
  };
  for (const f of Object.values(files)) assert(existsSync(f), `fixture missing: ${f}`);

  await step('launch: Benvenuto with no volumes', async () => {
    await startApp();
    await waitFor('welcome', 'return screen() === "welcome"');
    // tauri.conf.json app.security.freezePrototype: on in the real webview
    assert((await wd.exec('return Object.isFrozen(Object.prototype)')) === true, 'Object.prototype is not frozen');
    await shot('welcome-empty');
  });

  await step('create wizard: name, weak/mismatch refused, strong creates a real volume', async () => {
    await clickButton('Nuovo volume');
    await waitFor('wizard', 'return screen() === "create"');
    const name = await waitFor('name input', 'return byLabel("Nome")');
    await wd.clear(name);
    await wd.sendKeys(name, 'E2E');
    await waitFor('path preview', 'return document.body.textContent.includes(arguments[0])', { args: [join(WORK, 'stick', 'E2E.nvault')] });
    await shot('create-step1');
    await clickButton('Avanti');
    await waitFor('step 2', 'return document.querySelector("[data-step=\\"2\\"]")');
    const pw = await waitFor('password', 'return byLabel("Password")');
    const confirm = await waitFor('confirm', 'return byLabel("Ripeti la password")');
    await wd.sendKeys(pw, 'ciao');
    await wd.sendKeys(confirm, 'ciao');
    await waitFor('weak refused', 'return btn("Crea volume")?.disabled && document.body.textContent.includes("Almeno 8 caratteri.")');
    await wd.clear(pw);
    await wd.clear(confirm);
    await wd.sendKeys(pw, PASSWORD);
    await wd.sendKeys(confirm, PASSWORD + 'x');
    await waitFor('mismatch', 'return document.body.textContent.includes("Le password non coincidono.") && btn("Crea volume")?.disabled');
    await wd.clear(confirm);
    await wd.sendKeys(confirm, PASSWORD);
    await waitFor('strong accepted', 'return btn("Crea volume") && !btn("Crea volume").disabled');
    await shot('create-step2-key');
    const t0 = Date.now();
    await clickButton('Crea volume');
    await waitFor('step 3', 'return document.querySelector("[data-step=\\"3\\"]")', { timeout: 30000 });
    console.log(`     volume created in ${Date.now() - t0} ms`);
    await shot('create-step3-done');
    const vol = join(WORK, 'stick', 'E2E.nvault');
    assert(existsSync(vol), 'no .nvault on disk');
    const head = readFileSync(vol).subarray(0, 8).toString();
    assert(head === 'NICEVLT1', `bad magic ${head}`);
    await clickButton('Apri il volume');
    await waitFor('vault', 'return screen() === "vault" && document.body.textContent.includes("Volume vuoto")');
    await settled();
    await shot('vault-empty');
  });

  await step('lock, wrong password refused, right password unlocks', async () => {
    await clickButton('Blocca');
    await waitFor('unlock', 'return screen() === "unlock"');
    const pw = await waitFor('password', 'return byLabel("Password")');
    await wd.sendKeys(pw, 'sbagliata' + KEY.Enter);
    await waitFor('wrong password', 'return [...document.querySelectorAll("[role=alert]")].some((a) => a.textContent.includes("Password errata"))', { timeout: 20000 });
    await shot('unlock-wrong-password');
    const pw2 = await waitFor('password', 'return byLabel("Password")');
    await wd.sendKeys(pw2, PASSWORD + KEY.Enter);
    await waitFor('vault', 'return screen() === "vault"', { timeout: 20000 });
  });

  await step('import through the real GTK file dialog (Importa)', async () => {
    for (const f of [files.photo, files.pdf]) {
      const b = await waitFor('Importa', 'return btn("Importa")');
      void wd.click(b).catch(() => undefined); // the native dialog blocks the click's reply
      await answerFileChooser('Importa nel volume', f);
      const base = f.split('/').pop();
      await waitFor(`${base} listed`, 'return names().includes(arguments[0])', { args: [base], timeout: 20000 });
    }
    await shot('imported-via-dialog');
  });

  await step('dialog-less import: Tauri drag-drop event with host paths (same handler as an OS drop)', async () => {
    const at = await q('return rect(document.querySelector("[role=listbox]"))');
    const emit = (event, payload) =>
      wd.execAsync('const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: arguments[0], payload: arguments[1] }).then(() => done(true), (e) => done(String(e)));', event, payload);
    const paths = [files.video, files.text];
    assert((await emit('tauri://drag-enter', { paths, position: { x: at.x, y: at.y + at.height / 3 } })) === true, 'emit failed');
    await waitFor('overlay on enter', 'return document.querySelector("[data-testid=drop-overlay]")');
    await shot('tauri-drag-enter-overlay');
    await emit('tauri://drag-drop', { paths, position: { x: at.x, y: at.y + at.height / 3 } });
    await waitFor('overlay gone', 'return !document.querySelector("[data-testid=drop-overlay]")');
    await waitFor('dropped files listed', 'return names().includes("clip.webm") && names().includes("readme.txt")', { timeout: 20000 });
    await shot('imported-via-tauri-event');
  });

  await step('delete a few hundred files as a job (delete.rs): confirm, toast, gone, the rest untouched', async () => {
    // 300 photos of 64 KB in a folder, imported through the drop handler
    const pile = join(WORK, 'Mucchio');
    mkdirSync(pile, { recursive: true });
    const blob = Buffer.alloc(64 * 1024, 7);
    for (let i = 0; i < 300; i++) writeFileSync(join(pile, `scatto ${String(i).padStart(3, '0')}.jpg`), blob);
    const at = await q('return rect(document.querySelector("[role=listbox]"))');
    const emit = (event, payload) =>
      wd.execAsync('const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: arguments[0], payload: arguments[1] }).then(() => done(true), (e) => done(String(e)));', event, payload);
    await emit('tauri://drag-enter', { paths: [pile], position: { x: at.x, y: at.y + at.height / 3 } });
    await emit('tauri://drag-drop', { paths: [pile], position: { x: at.x, y: at.y + at.height / 3 } });
    await waitFor('Mucchio listed', 'return option("Mucchio")?.dataset.kind === "dir"', { timeout: 30000 });
    await waitFor('import finished', 'return toastTexts().some((t) => t.startsWith("Importato: Mucchio"))', { timeout: 30000 });
    const before = await listing();
    await clickOption('Mucchio');
    await wd.keys(KEY.Delete);
    const ok = await waitFor('confirm dialog', 'const d = dialog("Eliminare"); return d && [...d.querySelectorAll("button")].find((b) => norm(b.textContent) === "Elimina")');
    await wd.click(ok);
    await waitFor('delete toast', 'return toastTexts().some((t) => t === "Eliminati 301 elementi.")', { timeout: 30000 });
    await waitFor('Mucchio gone', 'return !option("Mucchio")');
    const after = await listing();
    assert(JSON.stringify(after) === JSON.stringify(before.filter((n) => n !== 'Mucchio')), `listing after delete: ${after}`);
    assert(!(await q('return document.querySelector("[aria-label=Trasferimenti] [data-kind=delete]")')), 'a finished delete left the tray');
    await shot('deleted-301-items');
  });

  await step('new folder via Ctrl+Shift+N + keyboard rename', async () => {
    await click('listbox', 'return document.querySelector("[role=listbox]")');
    await wd.chord([KEY.Control, KEY.Shift], 'N');
    await waitFor('rename input', 'return document.activeElement?.getAttribute("aria-label") === "Nuovo nome"');
    await wd.chord([KEY.Control], 'a');
    await typeText('Cartella');
    await wd.keys(KEY.Enter);
    await waitFor('folder listed', 'return option("Cartella")?.dataset.kind === "dir"');
  });

  await step('REAL OS drag & drop (XDND from a GTK drag source) onto the window', async () => {
    const list = await q('return rect(document.querySelector("[role=listbox]"))');
    const target = toScreen({ x: list.left + list.width * 0.6, y: list.top + list.height - 60 });
    const src = await dragSource(1300, 120, [files.drop1, files.photo2]);
    try {
      let sawOverlay = false;
      await pointerDrag({ x: 1380, y: 180 }, target, {
        midway: async () => {
          sawOverlay = !!(await q('return document.querySelector("[data-testid=drop-overlay]")'));
          rootShot('xdnd-in-flight');
        },
      });
      await waitFor('XDND files listed', 'return names().includes("dropped one.txt") && names().includes("photo 2.jpg")', { timeout: 20000 });
      await waitFor('overlay gone', 'return !document.querySelector("[data-testid=drop-overlay]")');
      console.log(`     overlay during the real drag: ${sawOverlay}`);
      assert(sawOverlay, 'drop overlay was not shown during the drag');
      await shot('xdnd-dropped');
    } finally {
      src.kill();
    }
  });

  await step('REAL OS drag & drop onto a folder tile imports into that folder', async () => {
    const tile = await q('return rect(option("Cartella"))');
    const src = await dragSource(1300, 120, [files.intoFolder]);
    try {
      await pointerDrag({ x: 1380, y: 180 }, toScreen(tile));
      await waitFor('job done', 'return toastTexts().some((t) => t.includes("Importato: into-folder.txt"))', { timeout: 20000 });
      assert(!(await listing()).includes('into-folder.txt'), 'landed in the current folder, not in Cartella');
      await shot('xdnd-onto-folder');
      await clickOption('Cartella');
      await wd.keys(KEY.Enter);
      await waitFor('inside Cartella', 'return document.querySelector("nav[aria-label=Percorso] [aria-current=page]")?.textContent.trim() === "Cartella"');
      await waitFor('file inside', 'return names().includes("into-folder.txt")');
      await shot('inside-cartella');
      await clickButton('Cartella superiore');
      await waitFor('back at root', 'return names().includes("Cartella")');
    } finally {
      src.kill();
    }
  });

  await step('internal HTML5 move in the real webview: drag a file tile onto a folder (real pointer)', async () => {
    const from = await q('return rect(option("dropped one.txt"))');
    const to = await q('return rect(option("Cartella"))');
    await pointerDrag(toScreen(from), toScreen(to));
    await waitFor('moved out of root', 'return !names().includes("dropped one.txt")', { timeout: 8000 });
    await shot('internal-move');
  });

  await step('thumbnails render through nvault://', async () => {
    await clickButton('Griglia');
    const r = await waitFor(
      'decoded thumbnails',
      `const imgs = [...document.querySelectorAll('[data-testid=thumbnail-image]')];
       const ok = imgs.filter((i) => i.complete && i.naturalWidth > 0);
       return imgs.length >= 2 && ok.length === imgs.length && imgs.map((i) => i.getAttribute('src'));`,
      { timeout: 15000 },
    );
    // pictures come from the backend (nvault://); a video's frame is drawn in the page (blob:, videoThumb.ts)
    assert(r.every((s) => s.startsWith('nvault://') || s.startsWith('blob:')), `thumbnail src not nvault/blob: ${r}`);
    assert(r.some((s) => s.startsWith('nvault://')), `no backend thumbnail: ${r}`);
    await shot('grid-thumbnails');
  });

  await step('folder envelopes: no-photo folders (files only, folders only, mixed) stay tidy in WebKitGTK', async () => {
    // host folders shaped like a game's folders: no images at all
    const B = join(HOST, 'Buste');
    const put = (dir, files, dirs = []) => {
      mkdirSync(join(B, dir), { recursive: true });
      for (const f of files) writeFileSync(join(B, dir, f), 'x'.repeat(64));
      for (const d of dirs) mkdirSync(join(B, dir, d), { recursive: true }), writeFileSync(join(B, dir, d, 'dentro.bin'), 'x');
    };
    put('data', [], ['bg', 'chr', 'etc', 'menu', 'music', 'pic', 'sound', 'movie', 'eff', 'item', 'map', 'demo']);
    put('sh2e', ['sh2pc.exe', 'binkw32.dll', 'local.fsb', 'config.ini'], ['data', 'saves', 'mods', 'tools', 'docs', 'cache', 'logs', 'backup', 'tmp']);
    put('solo file', ['a.bin', 'b.dat', 'c.afs', 'd.pak', 'e.tex']);
    put('una cartella', [], ['dentro']);
    put('un file e cartelle', ['solo.dat'], ['uno', 'due']);
    put('vuota', []);
    const at = await q('return rect(document.querySelector("[role=listbox]"))');
    const pos = { x: at.x, y: at.y + at.height / 3 };
    await wd.execAsync('const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "tauri://drag-drop", payload: arguments[0] }).then(() => done(true), (e) => done(String(e)));', { paths: [B], position: pos });
    await waitFor('Buste imported', 'return names().includes("Buste")', { timeout: 20000 });
    await clickOption('Buste');
    await wd.keys(KEY.Enter);
    await waitFor('in Buste', 'return document.querySelector("nav[aria-label=Percorso] [aria-current=page]")?.textContent.trim() === "Buste"');
    // pieces expected per folder (sheets for files, small envelopes for folders, at most 3)
    const want = { data: 3, sh2e: 3, 'solo file': 3, 'una cartella': 1, 'un file e cartelle': 3, vuota: 0 };
    const bad = await waitFor(
      'envelopes laid out',
      `const want = arguments[0];
       const tiles = [...document.querySelectorAll('[role=listbox] [role=option][data-kind=dir]')];
       if (tiles.length !== 6) return false;
       const out = [];
       const sizes = new Set();
       for (const t of tiles) {
         const name = norm(t.querySelector('[data-testid=item-name]').textContent);
         const env = t.querySelector('[data-envelope]').getBoundingClientRect();
         const box = t.getBoundingClientRect();
         sizes.add(Math.round(env.width) + 'x' + Math.round(env.height));
         const peeks = [...t.querySelectorAll('[data-peek]')];
         if (peeks.length !== want[name]) return false; // listing still loading
         for (const p of peeks) {
           const b = p.getBoundingClientRect();
           if (b.left < box.left - 1 || b.right > box.right + 1 || b.top < env.top - 4 || b.bottom > env.bottom || b.height > env.height * 0.75)
             out.push(name + ' ' + p.dataset.peek + ' ' + JSON.stringify(b.toJSON()) + ' in ' + JSON.stringify(env.toJSON()));
         }
       }
       if (sizes.size !== 1) out.push('tile sizes ' + [...sizes]);
       return { out };`,
      { args: [want], timeout: 15000 },
    );
    await settled();
    await shot('folder-envelopes');
    assert(bad.out.length === 0, `envelope pieces out of place: ${bad.out.join(' | ')}`);
    await clickButton('Cartella superiore');
    await waitFor('back at root', 'return names().includes("photo.jpg")');
  });

  async function quickLook(name, readyBody, shotName, diagnose, after) {
    await clickOption(name);
    await wd.keys(KEY.Space);
    try {
      await waitFor(`quick look ${name}`, 'return dialog("Anteprima di " + arguments[0])', { args: [name] });
      try {
        const r = await waitFor(`${name} rendered`, readyBody, { timeout: 20000 });
        if (typeof r === 'object') console.log(`     ${name}: ${JSON.stringify(r)}`);
      } catch (e) {
        if (diagnose) e.message += ' | ' + JSON.stringify(await wd.execAsync(diagnose));
        throw e;
      }
      await shot(shotName);
      if (after) await after();
    } finally {
      // never leave the modal open for the next step
      if (await q('return !!dialog("Anteprima di")')) {
        await wd.keys(KEY.Escape);
        await waitFor('quick look closed', 'return !dialog("Anteprima di")');
      }
    }
  }

  await step('quick look: image', () =>
    quickLook('photo.jpg', 'const i = dialog("Anteprima di")?.querySelector("img"); return i && i.complete && i.naturalWidth > 0 && i.src.startsWith("nvault://")', 'quicklook-image'),
  );
  await step('quick look: PDF (pdf.js, ranged reads over nvault://): page 2 via ›, zoom 125%', () =>
    quickLook(
      'document.pdf',
      `const d = dialog("Anteprima di"); const v = d?.querySelector('[data-testid="viewer-pdf"]');
       if (!v || v.dataset.status !== 'ready' || !d.querySelector('[aria-label="Pagina 1"] canvas[data-drawn]')) return false;
       if (!v.dataset.e2eStep) { v.dataset.e2eStep = '1'; d.querySelector('button[aria-label="Pagina successiva"]').click(); d.querySelector('button[aria-label="Ingrandisci"]').click(); return false; }
       return v.dataset.page === '2' && v.dataset.zoom === '125' && !!d.querySelector('[aria-label="Pagina 2"] canvas[data-drawn]') && { pages: v.dataset.pages, page: v.dataset.page, zoom: v.dataset.zoom }`,
      'quicklook-pdf',
    ),
  );
  await step('quick look: text', () => quickLook('readme.txt', 'return dialog("Anteprima di")?.textContent.length > 40', 'quicklook-text'));
  await step('quick look: video (loopback stream with Range, viewers.rs): plays and seeks, never a blob, nothing in /var/tmp', () =>
    quickLook(
      'clip.webm',
      `const v = dialog("Anteprima di")?.querySelector("video");
       if (!v || v.readyState < 1 || !v.videoWidth) return false;
       if (!v.currentSrc.startsWith('http://127.0.0.1:')) throw new Error('not streamed: ' + v.currentSrc.split(':')[0]);
       if (!v.loop) throw new Error('the stream does not loop: WebKitGTK buffers it on disk');
       if (!v.dataset.e2eSeek) { v.dataset.e2eSeek = '1'; v.currentTime = 3; return false; }
       return !v.seeking && Math.abs(v.currentTime - 3) < 0.15 && v.readyState >= 2 && { scheme: 'http', width: v.videoWidth, height: v.videoHeight, duration: v.duration, at: v.currentTime }`,
      'quicklook-video',
      `const done = arguments[arguments.length - 1];
       try {
         const v = document.querySelector('dialog[open] video');
         const info = { src: v && v.getAttribute('src'), readyState: v && v.readyState, networkState: v && v.networkState,
           mediaError: v && v.error ? [v.error.code, v.error.message] : null,
           canPlay: v ? [v.canPlayType('video/webm; codecs="vp8"'), v.canPlayType('video/webm; codecs="vp9"'), v.canPlayType('video/webm')] : null };
         if (!v) return done(info);
         fetch(v.getAttribute('src'), { headers: { Range: 'bytes=0-15' } })
           .then(async (r) => done({ ...info, fetch: [r.status, r.headers.get('content-type'), r.headers.get('content-range'), r.headers.get('accept-ranges'), (await r.arrayBuffer()).byteLength] }))
           .catch((e) => done({ ...info, fetch: String(e) }));
       } catch (e) { done({ diagError: String(e) }); }`,
      // GStreamer's download buffer would be an unlinked /var/tmp/WebKit-Media-* in the web process
      async () => {
        const v = 'dialog("Anteprima di")?.querySelector("video")';
        await q(`void ${v}?.play()`);
        // the stream loops (never disk-buffered): MediaViewer stops it just before the end ...
        const end = await waitFor('stopped at the end', `const e = ${v}; return e && e.paused && e.currentTime > e.duration - 0.5 && { at: e.currentTime, duration: e.duration }`, { timeout: 10000 });
        let fds = webMediaFds();
        assert(fds.length === 0, `decrypted media on disk: ${fds.join(', ')}`);
        // ... and the next play starts over from 0, still without a disk buffer
        await q(`void ${v}?.play()`);
        await waitFor('replays from 0', `const e = ${v}; return e && !e.paused && e.currentTime > 0.2 && e.currentTime < 2.5`, { timeout: 10000 });
        fds = webMediaFds();
        assert(fds.length === 0, `decrypted media on disk on replay: ${fds.join(', ')}`);
        console.log(`     stopped at ${end.at.toFixed(2)} of ${end.duration}, replayed, no WebKit-Media file`);
      },
    ),
  );

  await step('Proprietà (Alt+Enter): sniffed type, pixels, original path, SHA-256 equal to the host file', async () => {
    await clickOption('photo.jpg');
    await wd.exec('document.querySelector("[role=listbox]").focus()');
    await wd.chord(['\uE00A'], '\uE007'); // Alt+Enter (WebDriver Alt, Enter)
    await waitFor('props card', 'return document.querySelector("[data-testid=props][data-kind=file]") && document.querySelector("[data-testid=props-dimensions]")');
    const facts = await q('return { dims: norm(document.querySelector("[data-testid=props-dimensions]").textContent), origin: norm(document.querySelector("[data-testid=props-origin]").textContent), text: norm(document.querySelector("[data-testid=props]").textContent) }');
    assert(facts.dims === '960 × 659 px', `dimensions ${facts.dims}`);
    assert(facts.origin === join(HOST, 'photo.jpg'), `origin ${facts.origin}`);
    assert(facts.text.includes('image/jpeg'), 'sniffed type');
    await clickButton('Calcola');
    const hex = await waitFor('sha', 'return norm(document.querySelector("[data-testid=props-sha] code")?.textContent)');
    assert(hex === sha(join(HOST, 'photo.jpg')), `sha ${hex}`);
    await settled();
    await shot('props-file');
    await clickButton('Chiudi');
    await waitFor('closed', 'return !document.querySelector("[data-testid=props]")');
  });

  await step('view sizes: Molto grandi from the toolbar, Ctrl+- in the real webview, sharper thumbnails asked (?thumb=)', async () => {
    await clickButton('Griglia');
    await click('size XL', 'return document.querySelector("[role=radio][aria-label=\\"Molto grandi\\"]")');
    await waitFor('xl', 'return document.querySelector(".file-view-wrap")?.dataset.gridSize === "xl"');
    const src = await waitFor('thumb url', 'const o = option("photo.jpg"); const i = o?.querySelector("img"); return i && i.complete && i.naturalWidth > 0 && { src: i.getAttribute("src"), w: i.naturalWidth }');
    assert(/[?]thumb=\d+/.test(src.src) && src.w <= 1024, `thumbnail ${JSON.stringify(src)}`);
    await settled();
    await shot('view-size-xl');
    await wd.exec('document.querySelector("[role=listbox]").focus()');
    await wd.chord([KEY.Control], '-');
    await waitFor('l', 'return document.querySelector(".file-view-wrap")?.dataset.gridSize === "l"');
  });

  await step('export through the real GTK folder dialog, bytes identical', async () => {
    await clickOption('photo.jpg');
    await wd.cmd('POST', '/actions', {
      actions: [{ type: 'key', id: 'kbd', actions: [{ type: 'keyDown', value: KEY.Control }] }],
    });
    const pdf = await waitFor('pdf', 'return option("document.pdf")');
    await wd.click(pdf);
    await wd.cmd('DELETE', '/actions');
    await waitFor('2 selected', 'return document.querySelectorAll("[role=option][aria-selected=true]").length === 2');
    const b = await waitFor('Esporta', 'return btn("Esporta")');
    void wd.click(b).catch(() => undefined);
    await answerFileChooser('Esporta in…', EXPORT); // folder view + typed folder: Return accepts it
    await waitFor('export toast', 'return toastTexts().some((t) => t.startsWith("Esportato in"))', { timeout: 20000 });
    const out = readdirSync(EXPORT);
    assert(out.includes('photo.jpg') && out.includes('document.pdf'), `export dir has ${out}`);
    assert(sha(join(EXPORT, 'photo.jpg')) === sha(files.photo), 'photo.jpg differs after export');
    assert(sha(join(EXPORT, 'document.pdf')) === sha(files.pdf), 'document.pdf differs after export');
    await shot('exported');
  });

  let mountpoint = null;
  await step('mount toggle (FUSE): real drive, same bytes, write-through, live refresh of OS copies, unmount', async () => {
    const sw = await waitFor('switch', 'return document.querySelector("[role=switch]")');
    const disabled = await q('return arguments[0].disabled', sw);
    if (disabled) {
      const why = await q('return document.getElementById("mount-desc")?.textContent');
      throw new Error(`mount unavailable here: ${why}`);
    }
    await wd.click(sw);
    mountpoint = await waitFor('mounted', 'const s = document.querySelector("[role=switch]"); return s.getAttribute("aria-checked") === "true" && document.querySelector("#mount-desc [title]")?.getAttribute("title")', { timeout: 20000 });
    writeFileSync(join(OUT, 'mountpoint.txt'), mountpoint);
    await shot('mounted');
    const mounts = readFileSync('/proc/mounts', 'utf8');
    assert(mounts.includes(mountpoint.replace(/ /g, '\\040')), `${mountpoint} not in /proc/mounts`);
    const ls = readdirSync(mountpoint);
    assert(ls.includes('photo.jpg') && ls.includes('Cartella'), `mount lists ${ls}`);
    assert(sha(join(mountpoint, 'photo.jpg')) === sha(files.photo), 'photo.jpg differs through FUSE');
    writeFileSync(join(mountpoint, 'via-fuse.txt'), 'scritto dal disco montato\n');
    // live refresh: what a file manager copies onto the drive shows up in the open folder
    // with no user action (fs-changed from fs_events.rs)
    execFileSync('cp', [files.pdf, join(mountpoint, 'copiato-col-cp.pdf')]);
    await waitFor('cp copy listed without user action', 'return names().includes("copiato-col-cp.pdf")', { timeout: 3000 });
    await waitFor('via-fuse.txt listed without user action', 'return names().includes("via-fuse.txt")', { timeout: 3000 });
    await shot('live-refresh');
    await click('switch', 'return document.querySelector("[role=switch]")');
    await waitFor('unmounted', 'return document.querySelector("[role=switch]").getAttribute("aria-checked") === "false"', { timeout: 20000 });
    await sleep(500);
    assert(!readFileSync('/proc/mounts', 'utf8').includes(mountpoint.replace(/ /g, '\\040')), 'still mounted after toggle off');
    // the file written through the drive is in the volume
    await clickButton('Elenco');
    // reload the listing by stepping into a folder and back
    await clickOption('Cartella');
    await wd.keys(KEY.Enter);
    await waitFor('in Cartella', 'return document.querySelector("nav[aria-label=Percorso] [aria-current=page]")?.textContent.trim() === "Cartella"');
    await clickButton('Cartella superiore');
    await waitFor('via-fuse.txt listed', 'return names().includes("via-fuse.txt")');
    await shot('after-unmount-list');
  });

  await step('drive removal: the volume file vanishes -> locked with "Unità scollegata"', async () => {
    const vol = join(WORK, 'stick', 'E2E.nvault');
    const away = join(WORK, 'E2E.nvault.away');
    renameSync(vol, away); // what the watcher sees when the stick is pulled
    try {
      const reason = await waitFor('locked (removed)', 'return screen() === "unlock" && document.querySelector("[data-reason=removed]")?.textContent.trim()', { timeout: 10000 });
      assert(reason === 'Unità scollegata', `reason: ${reason}`);
      await settled();
      await shot('removed-locked');
    } finally {
      renameSync(away, vol); // stick back in
    }
    const pw = await waitFor('password', 'return byLabel("Password")');
    await wd.sendKeys(pw, PASSWORD + KEY.Enter);
    await waitFor('vault again', 'return screen() === "vault" && names().includes("photo.jpg")', { timeout: 20000 });
  });

  await step('password manager in the real volume: create an entry, reveal it, live 2FA code', async () => {
    await click('Password nav', 'return document.querySelector("[data-testid=nav-passwords]")');
    await waitFor('passwords screen', 'return screen() === "passwords" && document.querySelector("[data-testid=pm-new]")');
    await click('Nuova voce', 'return document.querySelector("[data-testid=pm-new]")');
    const into = async (sel, text) => {
      await waitFor(sel, `const e = document.querySelector(${JSON.stringify(sel)}); if (e && e.getClientRects().length) { e.focus(); return true; }`);
      await typeText(text);
    };
    await into('#pm-title', 'Router');
    await into('[data-testid=pm-editor] input[autocomplete=new-password]', 'segreto-del-router-42');
    await into('[data-testid=totp-input]', 'JBSWY3DPEHPK3PXP');
    await waitFor('totp checked by the backend', 'return document.querySelector("[data-testid=totp-check]")?.dataset.state === "ok"');
    await click('Salva', 'return document.querySelector("[data-testid=pm-save]")');
    await waitFor('saved', 'return !document.querySelector("[data-testid=pm-editor]") && document.querySelector("[data-testid=pm-detail]")?.textContent.includes("Router")');
    await click('Mostra', 'return document.querySelector("[data-testid=pm-reveal]")');
    await waitFor('revealed', 'return document.querySelector("[data-testid=pm-password]")?.textContent === "segreto-del-router-42"');
    await waitFor('live code', 'return /^\\d{3} \\d{3}$/.test(document.querySelector("[data-testid=totp-code]")?.textContent ?? "")');
    await shot('passwords');
    await click('File nav', 'return document.querySelector("[data-testid=nav-files]")');
    await waitFor('vault', 'return screen() === "vault"');
  });

  await step('password manager: CSV import through the GTK dialog maps raw export columns (websites, dates)', async () => {
    await click('Password nav', 'return document.querySelector("[data-testid=nav-passwords]")');
    await waitFor('passwords screen', 'return screen() === "passwords" && document.querySelector("[data-testid=pm-open-import]")');
    await click('Importa', 'return document.querySelector("[data-testid=pm-open-import]")');
    await click('CSV', 'return document.querySelector("[data-testid=pm-import] [data-kind=csv]")');
    await click('Avanti', 'return document.querySelector("[data-testid=imp-next]")');
    const choose = await waitFor('Scegli file', 'return document.querySelector("[data-testid=imp-choose]")');
    void wd.click(choose).catch(() => undefined);
    await answerFileChooser('Esportazione da importare', join(HOST, 'export.csv'));
    await click('Importa', 'const b = document.querySelector("[data-testid=imp-start]"); return b && !b.disabled && b');
    const n = await waitFor('imported', 'return document.querySelector("[data-testid=pm-import-result]")?.dataset.imported', { timeout: 20000 });
    assert(n === '2', `imported ${n}`);
    await click('Chiudi', 'return [...document.querySelectorAll("dialog[open] button")].find((b) => norm(b.textContent) === "Chiudi")');
    await click('entry', 'return document.querySelector(\'[data-testid=pm-entry][data-title="login.microsoftonline.com"]\')');
    const d = await waitFor('detail', 'const d = document.querySelector("[data-testid=pm-detail]"); return d?.textContent.includes("login.microsoftonline.com") && d.textContent');
    assert(d.includes('https://account.example.com/'), 'AUTOFILLURLS did not become a website');
    assert(!/AUTOFILLURLS|CREATETIME|MODIFYTIME/i.test(d), 'raw export fields still shown as custom fields');
    const dates = await waitFor('dates', 'return document.querySelector("[data-testid=pm-dates]")?.textContent');
    assert(/30 mar 2026/.test(dates) && /28 set 2026/.test(dates.replace(/\s+/g, ' ')), `dates: ${dates}`);
    // nothing raw left: the repair finds nothing (it ran once at open, and the import mapped everything)
    await click('Importa', 'return document.querySelector("[data-testid=pm-open-import]")');
    await click('Ripara', 'return document.querySelector("[data-testid=pm-repair]")');
    await waitFor('repair toast', 'return toastTexts().some((t) => t.startsWith("Nessuna voce da riparare"))');
    await click('Annulla', 'return [...document.querySelectorAll("dialog[open] button")].find((b) => norm(b.textContent) === "Annulla")');
    await shot('passwords-imported');
    await click('File nav', 'return document.querySelector("[data-testid=nav-files]")');
    await waitFor('vault', 'return screen() === "vault"');
  });

  await step('Impostazioni (Ctrl+,): language it -> en -> it in place, kept in settings.json (installed)', async () => {
    await wd.chord([KEY.Control], ',');
    await waitFor('settings', 'return screen() === "settings" && document.querySelector("[data-testid=app-version]")?.textContent.trim() !== "—"');
    // installed (a folder on the computer, like ~/.local/bin): the Avvio card with its three switches,
    // "Resta nel vassoio" on by default
    const startup = await waitFor('Avvio card', 'const c = document.querySelector("[data-testid=settings-startup]"); return c && [...c.querySelectorAll("[role=switch]")].map((s) => s.dataset.testid + "=" + s.getAttribute("aria-checked")).join(" ")');
    assert(startup === 'start-at-login=false start-minimized=false close-to-tray=true', `Avvio switches: ${startup}`);
    await q('document.querySelector("[data-testid=settings-startup]").scrollIntoView({ block: "center" })');
    await shot('settings-startup');
    await q('document.querySelector(".settings").scrollTop = 0');
    await click('English', 'return document.querySelector("[data-lang=en]")');
    await waitFor('english', 'return document.querySelector("h1")?.textContent.trim() === "Settings" && document.documentElement.lang === "en"');
    await shot('settings-en');
    const cfg = join(WORK, 'home', '.config', 'NiceVault', 'settings.json');
    await waitFor('settings.json written', 'return true');
    assert(existsSync(cfg) && JSON.parse(readFileSync(cfg, 'utf8')).language === 'en', `settings.json: ${existsSync(cfg) ? readFileSync(cfg, 'utf8') : 'missing'}`);
    await click('Italiano', 'return document.querySelector("[data-lang=it]")');
    await waitFor('italian', 'return document.querySelector("h1")?.textContent.trim() === "Impostazioni"');
    await click('Indietro', 'return document.querySelector("[data-testid=settings-back]")');
    await waitFor('vault', 'return screen() === "vault"');
  });

  await step('lock and quit; reopen the app, recent volume, unlock, data persisted', async () => {
    await clickButton('Blocca');
    await waitFor('unlock', 'return screen() === "unlock"');
    await settled();
    await shot('locked');
    await quitApp();
    assert(!existsSync(join(WORK, 'stick', 'E2E.nvault-wal')), 'WAL left behind after a clean close');
    await startApp();
    await waitFor('welcome with recent', 'return screen() === "welcome" && [...document.querySelectorAll("button")].some((b) => norm(b.textContent).includes("E2E"))');
    await shot('reopen-welcome');
    await click('recent E2E', 'return [...document.querySelectorAll("button")].find((b) => norm(b.textContent).includes("E2E") && !b.getAttribute("aria-label"))');
    const pw = await waitFor('password', 'return screen() === "unlock" && byLabel("Password")');
    await wd.sendKeys(pw, PASSWORD + KEY.Enter);
    await waitFor('vault', 'return screen() === "vault"', { timeout: 20000 });
    await waitFor('persisted', 'const n = names(); return ["photo.jpg", "document.pdf", "clip.webm", "readme.txt", "photo 2.jpg", "Cartella", "via-fuse.txt"].every((x) => n.includes(x)) && n', { timeout: 10000 });
    // the view size is the volume's own (Settings.grid_size): still Grandi after a restart
    await waitFor('view size kept', 'return document.querySelector(".file-view-wrap")?.dataset.gridSize === "l"');
    await settled();
    await shot('reopened-vault');
    await clickButton('Blocca');
    await waitFor('unlock', 'return screen() === "unlock"');
  });

  await step('live drives (fake source): plug a stick -> Benvenuto + Nuovo volume show it; pull -> gone', async () => {
    // e2e-real.sh points NICEVAULT_FAKE_DRIVES at this file; the app re-reads it like an OS signal
    const file = join(WORK, 'drives.json');
    const usb = join(WORK, 'usb', 'FAKE STICK');
    mkdirSync(usb, { recursive: true });
    copyFileSync(join(WORK, 'stick', 'E2E.nvault'), join(usb, 'Chiavetta.nvault'));
    const stick = { id: '/dev/fake1', label: 'FAKE STICK', mount_path: usb, fs_type: 'exfat', total_bytes: 32 * 2 ** 30, free_bytes: 20 * 2 ** 30, removable: true };
    const write = (list) => {
      writeFileSync(file + '.tmp', JSON.stringify(list));
      renameSync(file + '.tmp', file);
    };
    await clickButton('Altri volumi');
    await waitFor('welcome', 'return screen() === "welcome"');
    const passport = (n) => `return !!document.querySelector('[data-testid=passport][data-name="${n}"]')`;
    assert(!(await q(passport('Chiavetta'))), 'Chiavetta listed before the stick is plugged');
    write([stick]);
    await waitFor('volume on the plugged stick', passport('Chiavetta'), { timeout: 8000 });
    await settled();
    await shot('drives-plugged-welcome');
    await clickButton('Nuovo volume');
    await waitFor('drive card', 'return [...document.querySelectorAll("[data-testid=drive-name]")].some((e) => norm(e.textContent) === "FAKE STICK")');
    await click('FAKE STICK card', 'return [...document.querySelectorAll("[data-testid=drive]")].find((e) => norm(e.querySelector("[data-testid=drive-name]")?.textContent) === "FAKE STICK")');
    await waitFor('path on the stick', 'return document.body.textContent.includes(arguments[0])', { args: [join(usb, 'Nuovo volume.nvault')] });
    await settled();
    await shot('drives-picker');
    write([]);
    await waitFor('card gone + told why', 'return ![...document.querySelectorAll("[data-testid=drive-name]")].some((e) => norm(e.textContent) === "FAKE STICK") && !!document.querySelector("[data-testid=drive-gone]")', { timeout: 8000 });
    await clickButton('Annulla');
    await waitFor('welcome', 'return screen() === "welcome"');
    await waitFor('volume gone with the stick', 'return !' + passport('Chiavetta').slice(7), { timeout: 8000 });
  });

  await quitApp();
  const failed = results.filter((r) => !r.ok);
  writeFileSync(join(OUT, 'results.json'), JSON.stringify({ results, screenshots }, null, 2));
  console.log(`\n${results.length - failed.length}/${results.length} steps passed; screenshots in ${OUT}`);
  process.exit(failed.length ? 1 : 0);
}

main().catch(async (e) => {
  console.error('fatal:', e);
  await quitApp();
  process.exit(2);
});
