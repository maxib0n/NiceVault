// First-run flicker in the real webview (WebKitGTK): each animated surface is opened twice in
// one session — the first open and the second — and both are measured the same way:
//   - frame times from requestAnimationFrame over the open (a first open that drops frames the
//     second does not = work done once per process: GL programs / textures / layers created on
//     first use);
//   - on X (Xvfb), the screen captured every ~20 ms (capture.py) and the region of the surface
//     classified per frame (flicker.py: a frame darker / lighter than both neighbours, a frame
//     with the surface missing after it was shown).
// Prints one line per open and writes OUT/flicker-<label>.json + OUT/frames-flk-*/strip.png.
//   NV_E2E_SCRIPT=flicker.mjs pnpm e2e:real                (Xvfb: frames + timings)
//   NV_E2E_DISPLAY=wayland NV_E2E_SCRIPT=flicker.mjs ...   (nested KWin, GPU path)
//   (Wayland frames come from the nested KWin's ScreenShot2, capture-kwin.py, half size; the
//   webview is found on screen by locate.py.)
// Env: NV_FLK_LABEL=<name> (default "x11" / "wayland"), NV_FLK_ONLY=a,b (scenario names),
//      NV_FLK_N=<n> opens each surface n times (default 2), NV_FLK_CSS=<css> injects a stylesheet
//      (try a fix without a build). Exits 1 when any open flickered.
// What it found (styles/motion.css): the end of a compositor opacity/transform animation showed one
// stale frame from its middle (a slip flashing half-transparent): "!" in the labels.
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { KEY, WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const XDO = process.env.NV_E2E_XDO ?? 'xdotool';
const X11 = !!process.env.DISPLAY;
const LABEL = process.env.NV_FLK_LABEL ?? (X11 ? 'x11' : 'wayland');
const ONLY = (process.env.NV_FLK_ONLY ?? '').split(',').filter(Boolean);
const PASSWORD = 'cavallo batteria graffetta corretta';
const HERE = new URL('.', import.meta.url).pathname;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let wd;

function frames(name) {
  const dir = join(OUT, `frames-flk-${LABEL}-${name}`);
  rmSync(dir, { recursive: true, force: true });
  // X: the root window; Wayland: the nested KWin's composited screen (ScreenShot2)
  const p = X11 ? spawn('python3', [join(HERE, 'capture.py'), dir], { stdio: 'ignore' }) : spawn('python3', [join(HERE, 'capture-kwin.py'), dir, join(WORK, 'kwin-bus')], { stdio: ['ignore', 'ignore', 'inherit'] });
  const exited = new Promise((r) => p.on('exit', r));
  return async () => {
    p.kill('SIGTERM');
    await exited;
    return dir;
  };
}

async function waitFor(desc, script, timeout = 15000) {
  const t0 = Date.now();
  let last;
  while (Date.now() - t0 < timeout) {
    try {
      last = await wd.exec(script);
      if (last) return last;
    } catch (e) {
      last = e.message;
    }
    await sleep(50);
  }
  throw new Error(`timeout: ${desc} (last ${JSON.stringify(last)})`);
}
const invoke = (cmd, args) =>
  wd.cmd('POST', '/execute/async', {
    script: 'const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then((r) => done({ ok: r }), (e) => done({ err: e }));',
    args: [cmd, args],
  });

/** X screen rect of the first element matching css (the webview sits at the bottom of the window). */
async function screenRect(css) {
  const r = await wd.exec(`const e = document.querySelector(${JSON.stringify(css)}); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height, iw: innerWidth, ih: innerHeight }`);
  if (!r) return null;
  if (!X11) {
    const o = await webviewOrigin();
    return { x: Math.round(o.x + r.x), y: Math.round(o.y + r.y), w: Math.max(4, Math.round(r.w)), h: Math.max(4, Math.round(r.h)) };
  }
  const geo = (id) => execFileSync(XDO, ['getwindowgeometry', id]).toString().match(/Position: (\d+),(\d+).*\n.*Geometry: (\d+)x(\d+)/s);
  const id = execFileSync(XDO, ['search', '--class', '^nicevault$']).toString().trim().split('\n').find((i) => Number(geo(i)?.[3]) > 100);
  const g = geo(id);
  const ox = Number(g[1]) + (Number(g[3]) - r.iw);
  const oy = Number(g[2]) + (Number(g[4]) - r.ih);
  return { x: Math.round(ox + r.x), y: Math.round(oy + r.y), w: Math.max(4, Math.round(r.w)), h: Math.max(4, Math.round(r.h)) };
}

/** Wayland: where the page sits on the captured screen (found once: the window does not move). */
let origin = null;
async function webviewOrigin() {
  if (origin) return origin;
  let dir;
  for (let i = 0; i < 4 && !(dir && existsSync(join(dir, 'f000.png'))); i++) {
    const stop = frames('locate');
    await sleep(1200);
    dir = await stop();
  }
  const page = join(dir, 'page.png');
  writeFileSync(page, await wd.screenshot());
  origin = JSON.parse(execFileSync('python3', [join(HERE, 'locate.py'), join(dir, 'f000.png'), page, existsSync(join(dir, 'scale.txt')) ? readFileSync(join(dir, 'scale.txt'), 'utf8') : '1']).toString());
  console.log('webview at', JSON.stringify(origin));
  return origin;
}

const PROBE = `
  const J = (window.__flk = { t: [], on: true });
  const tick = (t) => { if (!J.on) return; J.t.push(t); requestAnimationFrame(tick); };
  requestAnimationFrame(tick);
`;

async function pointer(css, button = 0, double = false) {
  const el = await waitFor(css, `return document.querySelector(${JSON.stringify(css)})`);
  const origin = { 'element-6066-11e4-a52e-4f735466cecf': el['element-6066-11e4-a52e-4f735466cecf'] };
  const press = [{ type: 'pointerDown', button }, { type: 'pointerUp', button }];
  await wd.cmd('POST', '/actions', {
    actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions: [{ type: 'pointerMove', origin, x: 0, y: 0 }, ...press, ...(double ? press : [])] }],
  });
  await wd.cmd('DELETE', '/actions');
}

const results = [];
/** One open of a surface: frame times over `ms` and (X) the frames of `region`. */
async function measure(name, nth, open, region, close, ms = 700) {
  await wd.exec(PROBE);
  await sleep(120);
  const stop = frames(`${name}-${nth}`);
  await sleep(120);
  const t0 = await wd.exec('return performance.now()');
  await open();
  await sleep(ms);
  const t = await wd.exec('window.__flk.on = false; return window.__flk.t');
  const dir = await stop();
  const after = t.filter((x) => x >= t0 - 1);
  const iv = after.slice(1).map((x, i) => x - after[i]);
  const first = after.length ? Math.round(after[0] - t0) : null;
  const max = iv.length ? Math.round(Math.max(...iv)) : null;
  const over = iv.filter((d) => d > 34).length;
  let px = null;
  if (dir && region) {
    const r = await screenRect(region);
    if (r) px = JSON.parse(execFileSync('python3', [join(HERE, 'flicker.py'), dir, r.x, r.y, r.w, r.h]).toString());
  }
  const rest = await wd.exec(`const e = document.querySelector(${JSON.stringify(region)}); if (!e) return null; const c = getComputedStyle(e); return { transform: c.transform, opacity: c.opacity }`);
  const row = { name, nth, rest, first_frame_ms: first, max_frame_ms: max, frames_over_34ms: over, frames: iv.length, top: [...iv].sort((a, b) => b - a).slice(0, 4).map(Math.round), px };
  results.push(row);
  console.log(`${name.padEnd(14)} #${nth}  first frame ${String(first).padStart(4)} ms  max ${String(max).padStart(4)} ms  >34ms ${over}  top ${row.top.join(',')}${px ? `  flicker ${px.flicker} ${px.labels}` : ''}  rest ${JSON.stringify(rest)}`);
  await close();
  await sleep(500);
}

async function main() {
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await waitFor('app ready', 'return document.querySelector("[data-ready=true]")');
  // a volume: photos and files at the root, made through the real backend
  const vol = join(WORK, 'stick', 'Prova.nvault');
  const info = await invoke('vault_create', { path: vol, password: PASSWORD, kdf: 'fast' });
  if (!info.ok) throw new Error(JSON.stringify(info));
  const id = info.ok.id;
  await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Cartella' });
  const foto = await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Foto' });
  const shots = [];
  for (let i = 1; i <= 8; i++) {
    const f = join(WORK, 'host', `scatto ${i}.jpg`);
    writeFileSync(f, readFileSync(join(WORK, 'host', i % 2 ? 'photo.jpg' : 'photo 2.jpg')));
    shots.push(f);
  }
  await invoke('fs_import', { vault: id, parent: foto.ok.id, host_paths: shots });
  const job = await invoke('fs_import', { vault: id, parent: 1, host_paths: ['photo.jpg', 'photo 2.jpg', 'document.pdf', 'readme.txt'].map((f) => join(WORK, 'host', f)) });
  if (!job.ok) throw new Error(JSON.stringify(job));
  await sleep(1500);
  await invoke('vault_lock', { vault: id });
  // a fresh process state is what matters (first use of each GL program / texture): the page is
  // reloaded so the session starts like a user's, the process keeps nothing of this setup
  // except what the setup itself drew (Benvenuto)
  await wd.exec('location.reload()');
  await sleep(300);
  await waitFor('app ready', 'return document.querySelector("[data-ready=true]") && document.querySelector(".passport .book")');
  if (process.env.NV_FLK_CSS) await wd.exec('const s = document.createElement("style"); s.textContent = arguments[0]; document.head.appendChild(s)', process.env.NV_FLK_CSS);
  await sleep(1500);
  const book = await wd.find('.passport .book');
  await wd.click(book);
  const pw = await waitFor('password', 'return document.querySelector("input[type=password]")');
  await wd.sendKeys(pw, PASSWORD + KEY.Enter);
  await waitFor('vault', 'return document.querySelector("[data-screen=vault]") && document.querySelectorAll("[role=listbox] .file-item").length >= 5', 20000);
  // the unlock ceremony, the folder decode, thumbnails: over
  await sleep(4000);
  if (!X11) await webviewOrigin();

  const want = (n) => !ONLY.length || ONLY.includes(n);
  const esc = () => wd.keys(KEY.Escape);
  const item = (n) => `[role=listbox] .file-item:has([data-testid=item-name][title="${n}"])`;
  for (let nth = 1; nth <= (Number(process.env.NV_FLK_N) || 2); nth++) {
    // a folder of photos: decodes the first time (thumbnails arriving, glyphs handing over to the
    // pictures); the second time it is a repeated place (no decode): both must be clean
    if (want('folder')) {
      await measure('folder', nth, () => pointer('[role=listbox] .file-item:has([data-testid=item-name][title="Foto"])', 0, true), '[role=listbox]', async () => {
        await wd.keys(KEY.Backspace);
        await sleep(800);
      }, 2200);
    }
    if (want('sort-menu')) await measure('sort-menu', nth, () => pointer('.toolbar .sort, button.sort'), '.menu', esc);
    if (want('context-menu')) await measure('context-menu', nth, () => pointer(item('readme.txt'), 2), '.menu', esc);
    if (want('filter-pop')) {
      if (nth === 1) {
        await wd.exec('document.querySelector("[role=listbox]").focus()');
        await wd.chord([KEY.Control], 'f');
        await waitFor('filter chips', 'return document.querySelector("[data-testid=filter-chip-kind]")');
        await sleep(600);
      }
      await measure('filter-pop', nth, () => pointer('[data-testid=filter-chip-kind]'), '[data-testid=filter-menu]', esc);
      if (nth === (Number(process.env.NV_FLK_N) || 2)) {
        await esc();
        await sleep(400);
      }
    }
    if (want('quicklook')) {
      await measure('quicklook', nth, async () => {
        await pointer(item('photo.jpg'));
        await wd.keys(KEY.Space);
      }, 'dialog.quick-look[open]', esc, 900);
    }
    if (want('props')) {
      await measure('props', nth, async () => {
        await pointer(item('readme.txt'));
        await wd.chord([''], KEY.Enter);
      }, 'dialog[open]', esc);
    }
    if (want('settings')) {
      await measure('settings', nth, () => wd.chord([KEY.Control], ','), '.settings', async () => {
        await pointer('[data-testid=settings-back]');
        await sleep(600);
      });
    }
  }
  writeFileSync(join(OUT, `flicker-${LABEL}.json`), JSON.stringify(results, null, 2));
  await wd.quit().catch(() => undefined);
  const bad = results.filter((r) => r.px?.flicker);
  console.log(bad.length ? `FLICKER in ${bad.map((r) => `${r.name} #${r.nth}`).join(', ')}` : 'no flicker');
  process.exit(bad.length ? 1 : 0);
}

main().catch(async (e) => {
  console.error('fatal:', e);
  await wd?.quit().catch(() => undefined);
  process.exit(2);
});
