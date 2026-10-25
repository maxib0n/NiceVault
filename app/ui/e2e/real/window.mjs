// The main window's size and place in the real app (window_geom.rs), on the private Xvfb screen
// (1600x1000, no window manager: the work area is the whole screen):
//   1. first start: 80 % of the screen (1280x800), centred;
//   2. moved and resized, then closed (WM_DELETE_WINDOW, the window manager's ×): the next start
//      opens at that size and place (installed: window.json next to settings.json);
//   3. a saved place off every screen (a monitor unplugged since): centred on the screen there is;
//   4. portable (NICEVAULT_PORTABLE=1): nothing written, the first size again.
// Run: NV_E2E_SCRIPT=window.mjs pnpm e2e:real     (X11: xdotool + python-xlib)
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const XDO = process.env.NV_E2E_XDO ?? 'xdotool';
const FILE = join(WORK, 'home', '.config', 'NiceVault', 'window.json');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let wd;
let failed = 0;

function check(ok, msg) {
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${msg}`);
}
async function until(fn, timeout = 10000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeout) {
    if (await fn()) return true;
    await sleep(150);
  }
  return false;
}
const appPids = () => spawnSync('pgrep', ['-f', `${WORK}/stick/nicevault`]).stdout.toString().split('\n').filter(Boolean);
function mainWindow() {
  const ids = spawnSync(XDO, ['search', '--onlyvisible', '--class', '^nicevault$']).stdout.toString().split('\n').filter(Boolean);
  for (const id of ids) {
    const g = geometry(id);
    if (g && g.w > 100) return id;
  }
  return null;
}
function geometry(id) {
  const out = execFileSync(XDO, ['getwindowgeometry', id]).toString();
  const p = out.match(/Position: (-?\d+),(-?\d+)/);
  const s = out.match(/Geometry: (\d+)x(\d+)/);
  return p && s ? { x: Number(p[1]), y: Number(p[2]), w: Number(s[1]), h: Number(s[2]) } : null;
}
function closeWindow(id) {
  execFileSync('python3', ['-c', `
from Xlib import X, display, protocol
d = display.Display(); w = d.create_resource_object('window', ${Number(id)})
p, dl = d.intern_atom('WM_PROTOCOLS'), d.intern_atom('WM_DELETE_WINDOW')
w.send_event(protocol.event.ClientMessage(window=w, client_type=p, data=(32, [dl, X.CurrentTime, 0, 0, 0])), event_mask=X.NoEventMask)
d.flush()`]);
}
async function start() {
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await until(() => wd.exec('return !!document.querySelector("[data-ready=true]")').catch(() => false), 20000);
  let id = null;
  await until(() => (id = mainWindow()), 10000);
  await sleep(400);
  return id;
}
async function stop(id) {
  closeWindow(id);
  const gone = await until(() => appPids().length === 0, 10000);
  await wd.quit().catch(() => undefined);
  return gone;
}
const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;

async function main() {
  rmSync(FILE, { force: true });
  // 1. first start
  let id = await start();
  let g = geometry(id);
  check(near(g.w, 1280) && near(g.h, 800) && near(g.x, 160) && near(g.y, 100), `first start: 80 % of 1600x1000, centred (${JSON.stringify(g)})`);
  execFileSync('import', ['-window', 'root', join(OUT, 'window-first.png')]);

  // 2. moved and resized, closed: kept
  execFileSync(XDO, ['windowsize', id, 1000, 700]);
  await sleep(300);
  execFileSync(XDO, ['windowmove', id, 50, 60]);
  await sleep(700);
  g = geometry(id);
  check(near(g.w, 1000) && near(g.x, 50), `moved and resized (${JSON.stringify(g)})`);
  check(await stop(id), 'closing the window quits (no tray here)');
  const saved = existsSync(FILE) ? JSON.parse(readFileSync(FILE, 'utf8')) : null;
  check(!!saved && near(saved.w, 1000) && near(saved.h, 700) && near(saved.x, 50) && near(saved.y, 60) && saved.maximized === false, `window.json kept (${JSON.stringify(saved)})`);
  id = await start();
  g = geometry(id);
  check(near(g.w, 1000) && near(g.h, 700) && near(g.x, 50) && near(g.y, 60), `next start: the size and place it was closed at (${JSON.stringify(g)})`);
  await stop(id);

  // 3. off every screen: centred on this one
  writeFileSync(FILE, JSON.stringify({ x: 4000, y: 300, w: 1100, h: 720, maximized: false }));
  id = await start();
  g = geometry(id);
  check(near(g.w, 1100) && near(g.h, 720) && near(g.x, 250) && near(g.y, 140), `a place off screen: centred on the screen there is (${JSON.stringify(g)})`);
  await stop(id);

  // 4. portable: nothing kept
  rmSync(FILE, { force: true });
  // (started directly: tauri-driver does not pass an environment to the app)
  const portable = spawn(APP, [], { env: { ...process.env, NICEVAULT_PORTABLE: '1' }, stdio: 'ignore' });
  await until(() => (id = mainWindow()), 20000);
  await sleep(1500);
  execFileSync(XDO, ['windowsize', id, 1000, 700]);
  await sleep(500);
  closeWindow(id);
  await until(() => portable.exitCode !== null, 10000);
  check(!existsSync(FILE), 'portable: no window.json written');
  id = await start();
  g = geometry(id);
  check(near(g.w, 1280) && near(g.h, 800), `portable then installed: the first size again (${JSON.stringify(g)})`);
  await stop(id);

  console.log(failed ? `\n${failed} check(s) failed` : '\nall window checks passed');
  process.exit(failed ? 1 : 0);
}

main().catch(async (e) => {
  console.error('fatal:', e);
  await wd?.quit().catch(() => undefined);
  process.exit(2);
});
