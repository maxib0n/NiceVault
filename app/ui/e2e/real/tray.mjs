import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const XDO = process.env.NV_E2E_XDO ?? 'xdotool';
const HERE = new URL('.', import.meta.url).pathname;
const LOG = join(WORK, 'sni.log');
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
const xids = (visible) => spawnSync(XDO, ['search', ...(visible ? ['--onlyvisible'] : []), '--class', '^nicevault$']).stdout.toString().split('\n').filter(Boolean);
function mainWindow() {
  for (const id of xids(false)) {
    const g = execFileSync(XDO, ['getwindowgeometry', id]).toString().match(/Geometry: (\d+)x(\d+)/);
    if (g && Number(g[1]) > 100) return id;
  }
  return null;
}
const shown = () => {
  const id = mainWindow();
  return !!id && xids(true).includes(id);
};
function closeWindow() {
  const id = mainWindow();
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
}
const settings = () =>
  wd.cmd('POST', '/execute/async', { script: 'const done = arguments[0]; window.__TAURI_INTERNALS__.invoke("app_settings_get").then(done, (e) => done({ err: e }))', args: [] });
const registered = () => existsSync(LOG) && readFileSync(LOG, 'utf8').trim().length > 0;

async function main() {
  const watcher = spawn('python3', [join(HERE, 'sni-watcher.py'), LOG], { stdio: 'inherit' });
  await sleep(800);
  await start();
  const s = await settings();
  check(s.close_to_tray === true, `"Resta nel vassoio" on by default when installed (${s.close_to_tray})`);
  check(await until(registered, 10000), `the tray icon registered with the tray: ${existsSync(LOG) ? readFileSync(LOG, 'utf8').trim() : 'nothing'}`);
  check(shown(), 'the window is shown at start');

  const inv = (cmd, args) => wd.cmd('POST', '/execute/async', { script: 'const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then((r) => done({ ok: r }), (e) => done({ err: e }))', args: [cmd, args] });
  const made = await inv('vault_create', { path: join(WORK, 'stick', 'Vassoio.nvault'), password: 'cavallo batteria graffetta corretta', kdf: 'fast' });
  check(!!made.ok, `a volume is open before closing (${JSON.stringify(made.err ?? '')})`);
  closeWindow();
  const hid = await until(() => !shown(), 6000);
  await sleep(800);
  const alive = appPids().length > 0;
  const open = await inv('vault_list_open', {});
  check(hid && alive && open.ok?.length === 1, `closing the window hides it, NiceVault keeps running with the volume open (hidden ${hid}, process ${alive}, open ${open.ok?.length})`);

  const second = spawnSync(APP, [], { timeout: 15000, env: process.env });
  const back = await until(shown, 6000);
  check(second.status === 0 && back, `a second launch shows the window again and exits (exit ${second.status}${second.error ? ' ' + second.error.message : ''}, shown ${back})`);

  const menu = spawnSync('python3', [join(HERE, 'sni-menu.py'), LOG, 'Esci']);
  console.log('     tray menu:', menu.stdout.toString().trim(), menu.stderr.toString().trim());
  const gone = await until(() => appPids().length === 0, 8000);
  check(menu.status === 0 && gone, `Esci in the icon's menu quits (${appPids().length} process(es) left)`);
  await wd.quit().catch(() => undefined);

  watcher.kill('SIGTERM');
  await sleep(800);
  await start();
  check(shown(), 'no tray: the window is shown');
  closeWindow();
  const quit = await until(() => appPids().length === 0, 8000);
  check(quit, `no tray: closing the window quits (${appPids().length} process(es) left)`);
  await wd.quit().catch(() => undefined);
  console.log(failed ? `\n${failed} check(s) failed` : '\nall tray checks passed');
  process.exit(failed ? 1 : 0);
}

main().catch(async (e) => {
  console.error('fatal:', e);
  await wd?.quit().catch(() => undefined);
  process.exit(2);
});
