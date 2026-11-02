import { execFileSync, spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const XDO = process.env.NV_E2E_XDO ?? 'xdotool';
const PASSWORD = 'cavallo batteria graffetta corretta';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const xdo = (...a) => execFileSync(XDO, a.map(String)).toString().trim();
let wd;
let failed = 0;
const ok = (cond, msg) => {
  if (!cond) failed++;
  console.log(`${cond ? 'ok  ' : 'FAIL'} ${msg}`);
};
async function until(fn, timeout = 8000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeout) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
    }
    await sleep(120);
  }
  return null;
}
const invoke = (cmd, args) =>
  wd.cmd('POST', '/execute/async', {
    script: 'const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then((r) => done({ ok: r }), (e) => done({ err: e }));',
    args: [cmd, args],
  });

function appWindow() {
  for (const id of xdo('search', '--class', '^nicevault$').split('\n').filter(Boolean)) {
    const g = geometry(id);
    if (g && g.w > 100) return id;
  }
  return null;
}
function geometry(id) {
  const out = xdo('getwindowgeometry', id);
  const p = out.match(/Position: (-?\d+),(-?\d+)/);
  const s = out.match(/Geometry: (\d+)x(\d+)/);
  return p && s ? { x: +p[1], y: +p[2], w: +s[1], h: +s[2] } : null;
}
const prop = (id, name) => spawnSync('xprop', ['-id', id, name]).stdout.toString().trim();
const shot = (name) => execFileSync('import', ['-window', 'root', '-crop', '1600x1000+0+0', join(OUT, `titlebar-${name}.png`)]);
const bar = () =>
  wd.exec(`
    const b = document.querySelector('[data-testid=titlebar]');
    if (!b) return null;
    const r = b.getBoundingClientRect();
    const btn = [...b.querySelectorAll('button')].map((x) => { const q = x.getBoundingClientRect(); return { label: x.getAttribute('aria-label'), x: q.x + q.width / 2, y: q.y + q.height / 2 }; });
    return { h: r.height, w: r.width, btn, zoom: outerWidth / innerWidth, maximized: document.documentElement.hasAttribute('data-maximized') };
  `);
function screenAt(id, b, x, y) {
  const g = geometry(id);
  return [Math.round(g.x + x * b.zoom), Math.round(g.y + y * b.zoom)];
}
async function drag(from, to) {
  xdo('mousemove', ...from);
  await sleep(150);
  xdo('mousedown', 1);
  await sleep(150);
  xdo('mousemove', from[0] + 4, from[1] + 2);
  await sleep(120);
  for (let i = 1; i <= 8; i++) {
    xdo('mousemove', Math.round(from[0] + ((to[0] - from[0]) * i) / 8), Math.round(from[1] + ((to[1] - from[1]) * i) / 8));
    await sleep(40);
  }
  xdo('mouseup', 1);
  await sleep(500);
}

async function main() {
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await until(() => wd.exec('return !!document.querySelector("[data-ready=true]")'), 20000);
  const id = await until(appWindow, 10000);
  xdo('windowsize', id, 1100, 720);
  xdo('windowmove', id, 120, 90);
  await sleep(700);

  const motif = prop(id, '_MOTIF_WM_HINTS');
  ok(/_MOTIF_WM_HINTS\(_MOTIF_WM_HINTS\) = 0x2, 0x0, 0x0/.test(motif), `no system decorations (${motif})`);
  let b = await bar();
  ok(b && b.h >= 30 && b.btn.length === 3, `title bar drawn by the page: ${JSON.stringify(b && { h: b.h, buttons: b.btn.map((x) => x.label) })}`);
  shot('1-dark-welcome');

  let g0 = geometry(id);
  await drag(screenAt(id, b, b.w * 0.3, b.h / 2), screenAt(id, b, b.w * 0.3 + 160, b.h / 2 + 90));
  let g1 = geometry(id);
  ok(Math.abs(g1.x - g0.x - 160) <= 8 && Math.abs(g1.y - g0.y - 90) <= 8, `drag moves the window: ${g0.x},${g0.y} -> ${g1.x},${g1.y}`);
  const dbl = async () => {
    const [x, y] = screenAt(id, b, b.w * 0.3, b.h / 2);
    xdo('mousemove', x, y);
    await sleep(150);
    xdo('click', '--repeat', 2, '--delay', 90, 1);
  };
  await dbl();
  const max = await until(async () => (await bar())?.maximized && geometry(id).w >= 1590);
  ok(!!max, `double click maximises (${JSON.stringify(geometry(id))})`);
  b = await bar();
  shot('2-maximized');
  await dbl();
  const rest = await until(async () => !(await bar())?.maximized && geometry(id).w < 1200);
  ok(!!rest, `double click again restores (${JSON.stringify(geometry(id))})`);
  b = await bar();

  const press = async (label) => {
    const btn = b.btn.find((x) => x.label === label);
    if (!btn) return ok(false, `button ${label} missing: ${JSON.stringify(b.btn)}`);
    xdo('mousemove', ...screenAt(id, b, btn.x, btn.y));
    await sleep(150);
    xdo('click', 1);
  };
  await press('Ingrandisci la finestra');
  ok(!!(await until(async () => (await bar())?.btn.some((x) => x.label === 'Ripristina la finestra'))), 'maximise button: maximised, its label now Ripristina la finestra');
  b = await bar();
  await press('Ripristina la finestra');
  ok(!!(await until(async () => (await bar())?.btn.some((x) => x.label === 'Ingrandisci la finestra') && geometry(id).w < 1200)), 'restore button: back to the window size');
  b = await bar();

  g0 = geometry(id);
  await drag([g0.x + g0.w - 2, g0.y + g0.h - 2], [g0.x + g0.w + 118, g0.y + g0.h + 78]);
  g1 = geometry(id);
  ok(g1.w - g0.w >= 100 && g1.h - g0.h >= 60, `bottom-right edge resizes: ${g0.w}x${g0.h} -> ${g1.w}x${g1.h}`);
  g0 = g1;
  await drag([g0.x + 1, g0.y + g0.h / 2], [g0.x - 99, g0.y + g0.h / 2]);
  g1 = geometry(id);
  ok(g1.w - g0.w >= 80 && g1.x < g0.x, `left edge resizes: ${g0.x}+${g0.w} -> ${g1.x}+${g1.w}`);

  xdo('windowsize', id, 1100, 720);
  await sleep(500);
  await invoke('app_settings_set', { patch: { app_theme: 'light' } });
    const vol = join(WORK, 'stick', 'Dichiarazioni dei redditi e ricevute.nvault');
  const info = await invoke('vault_create', { path: vol, password: PASSWORD, kdf: 'fast' });
  if (info.ok) await invoke('vault_lock', { vault: info.ok.id });
  await wd.exec('location.reload()');
  await until(() => wd.exec('return document.querySelector("[data-ready=true]") && document.querySelector(".passport .book")'), 15000);
  await wd.exec('document.querySelector(".passport .book").click()');
  await until(() => wd.exec('return document.querySelector("[data-screen=unlock]")'));
  xdo('windowsize', id, 900, 600);
  await sleep(900);
  shot('3-light-unlock-900x600');
  await invoke('app_settings_set', { patch: { ui_scale: 150 } });
  await sleep(900);
  b = await bar();
  ok(b && b.btn.length === 3, `150 %: strip and buttons there (zoom ${b?.zoom.toFixed(2)})`);
  const clash = await wd.exec(`
    const m = document.querySelector('.unlock .mrz'), p = document.querySelector('.unlock .data-panel');
    return m && p ? p.getBoundingClientRect().bottom - m.getBoundingClientRect().top : null;`);
  ok(clash !== null && clash <= 1, `150 % at 900x600: Sblocca's data panel clear of the MRZ (${clash})`);
  shot('4-light-unlock-900x600-150');
  await wd.sendKeys(await wd.find('input[type=password]'), PASSWORD + '');
  await until(() => wd.exec('return document.querySelector("[data-screen=vault]")'), 20000);
  await sleep(1200);
  shot('5-light-vault-900x600-150');
  await invoke('app_settings_set', { patch: { ui_scale: 100 } });
  xdo('windowsize', id, 1100, 720);
  await sleep(900);
  shot('6-light-vault');
  b = await bar();

  await press('Riduci a icona');
  const hidden = await until(() => /_NET_WM_STATE_HIDDEN/.test(prop(id, '_NET_WM_STATE')));
  ok(!!hidden, `minimise: ${prop(id, '_NET_WM_STATE')}`);
  xdo('windowactivate', '--sync', id);
  await sleep(600);

  b = await bar();
  await press('Chiudi la finestra');
  const gone = await until(() => spawnSync('pgrep', ['-f', `${WORK}/stick/nicevault`]).stdout.toString().trim() === '', 10000);
  ok(!!gone, 'the close button closes the window (the app quits)');
  await wd.quit().catch(() => undefined);
  process.exit(failed ? 1 : 0);
}

main().catch(async (e) => {
  console.log('FAIL', e.message);
  await wd?.quit().catch(() => undefined);
  process.exit(1);
});
