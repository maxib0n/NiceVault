import { execFileSync, spawn } from 'node:child_process';
import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { KEY, WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const PASSWORD = 'cavallo batteria graffetta corretta';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const XDO = process.env.NV_E2E_XDO ?? 'xdotool';
const SLOW = Number(process.env.NV_REVEAL_SLOW) || 1;
const HERE = new URL('.', import.meta.url).pathname;
let wd;
let failed = 0;

function frames(name) {
  const dir = join(OUT, `frames-${name}`);
  rmSync(dir, { recursive: true, force: true });
  const p = spawn('python3', [join(HERE, 'capture.py'), dir], { stdio: 'ignore' });
  const exited = new Promise((r) => p.on('exit', r));
  return async () => {
    p.kill('SIGTERM');
    await exited;
    const ms = JSON.parse(readFileSync(join(dir, 'times.json'), 'utf8'));
    return { dir, n: ms.length, ms: ms.at(-1), every: Math.round(ms.at(-1) / Math.max(1, ms.length - 1)) };
  };
}

async function screenRect(css) {
  const r = await wd.exec(`const r = document.querySelector(${JSON.stringify(css)}).getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height, iw: innerWidth, ih: innerHeight }`);
  const geo = (id) => execFileSync(XDO, ['getwindowgeometry', id]).toString().match(/Position: (\d+),(\d+).*\n.*Geometry: (\d+)x(\d+)/s);
  const id = execFileSync(XDO, ['search', '--class', '^nicevault$']).toString().trim().split('\n').find((i) => Number(geo(i)?.[3]) > 100);
  const g = geo(id);
  const ox = Number(g[1]) + (Number(g[3]) - r.iw);
  const oy = Number(g[2]) + (Number(g[4]) - r.ih);
  return { x: Math.round(ox + r.x), y: Math.round(oy + r.y), w: Math.round(r.w), h: Math.round(r.h) };
}

async function noFlash(desc, cap, css, arrive = false) {
  const r = await screenRect(css);
  const out = JSON.parse(execFileSync('python3', [join(HERE, 'frames.py'), cap.dir, r.x, r.y, r.w, r.h, arrive ? 'arrive' : '']).toString());
  const ok = !out.flash && out.first_glyph >= 0;
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${desc}: no plain frame before the glyphs — ${out.frames} frames, first glyph #${out.first_glyph}, first settled #${out.first_final}\n       ${out.labels}\n       region ${r.x},${r.y} ${r.w}x${r.h} -> ${cap.dir}/strip.png`);
}

async function noBlank(desc, cap) {
  const r = await screenRect('body');
  const out = JSON.parse(execFileSync('python3', [join(HERE, 'frames.py'), cap.dir, r.x, r.y, r.w, r.h]).toString());
  const first = out.labels.search(/[GF]/);
  const blank = first >= 0 && out.labels.slice(first).includes('-');
  if (blank || first < 0) failed++;
  console.log(`${!blank && first >= 0 ? 'ok  ' : 'FAIL'} ${desc}: no blank frame — ${out.frames} frames every ~${cap.every} ms\n       ${out.labels}\n       -> ${cap.dir}/strip.png`);
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
    await sleep(60);
  }
  throw new Error(`timeout: ${desc} (last ${JSON.stringify(last)})`);
}

const log = () => wd.exec('return window.__nvReveal || []');
const invoke = (cmd, args) =>
  wd.cmd('POST', '/execute/async', {
    script: 'const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then((r) => done({ ok: r }), (e) => done({ err: e }));',
    args: [cmd, args],
  });

async function played(desc, src) {
  const re = new RegExp(src);
  const entry = await waitFor(`${desc}: decode done`, `return (window.__nvReveal || []).find((e) => e.kind === 'decode' && e.what === 'done' && new RegExp(${JSON.stringify(src)}).test(e.key))`, 8000).catch(() => null);
  const all = await log();
  const p = all.find((e) => e.kind === 'decode' && e.what === 'play' && re.test(e.key));
  const ok = !!(p && entry);
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${desc}: ${ok ? `played ${entry.t - p.t} ms (${p.key})` : JSON.stringify(all.filter((e) => re.test(e.key)))}`);
}

async function mediaPlayed(desc, prefix) {
  await sleep(1200);
  const all = await log();
  const plays = all.filter((e) => e.kind === 'media' && e.key.startsWith(prefix));
  const ok = plays.some((e) => e.what === 'play' || e.what === 'merged') && !(plays.some((e) => e.what === 'play') && plays.some((e) => e.what === 'merged' || e.what === 'shown'));
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${desc}: ${plays.map((e) => e.what).join(', ') || 'nothing logged'}`);
}

const SAMPLER = `
  const out = []; let on = true;
  const box = document.querySelector('[role=listbox]').getBoundingClientRect();
  const tick = () => {
    if (!on) return;
    const items = [...document.querySelectorAll('[role=listbox] .file-item')];
    const visible = items.filter((el) => { const r = el.getBoundingClientRect(); return r.bottom > box.top && r.top < box.bottom; });
    const held = visible.filter((el) => el.querySelector('[data-ar-t], [data-ar-m]') || el.matches('[data-ar-t], [data-ar-m]'));
    out.push({ items: items.length, visible: visible.length, held: held.length, skeleton: !!document.querySelector('.tile-skeleton .ascii-skeleton, .tile-skeleton :not(:empty)') });
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  window.__nvSample = { stop: () => { on = false; return out; } };
`;

const QL_SAMPLER = `
  const out = []; let on = true; let srcs = 0; let last = null;
  const tick = () => {
    if (!on) return;
    const img = document.querySelector('dialog.quick-look[open] img');
    if (img) {
      if (img.currentSrc !== last) { if (last !== null) srcs++; last = img.currentSrc; }
      const cs = getComputedStyle(img);
      const visible = img.complete && img.naturalWidth > 0 && cs.opacity !== '0' && cs.visibility !== 'hidden' && !img.hasAttribute('data-ar-m');
      const glyphs = !!img.closest('dialog').querySelector('.ar-overlay canvas');
      out.push({ visible, glyphs });
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  window.__nvQl = { stop: () => { on = false; return out; }, get srcs() { return srcs; } };
`;

async function dbl(name) {
  const el = await waitFor(`item ${name}`, `return [...document.querySelectorAll('[role=option]')].find((o) => o.querySelector('[data-testid=item-name]')?.textContent.trim() === ${JSON.stringify(name)})`);
  const origin = { 'element-6066-11e4-a52e-4f735466cecf': el['element-6066-11e4-a52e-4f735466cecf'] };
  await wd.cmd('POST', '/actions', {
    actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions: [
      { type: 'pointerMove', origin, x: 0, y: 0 },
      { type: 'pointerDown', button: 0 }, { type: 'pointerUp', button: 0 },
      { type: 'pointerDown', button: 0 }, { type: 'pointerUp', button: 0 },
    ] }],
  });
  await wd.cmd('DELETE', '/actions');
}

async function screenRects(css) {
  const rs = await wd.exec(`const box = document.querySelector('[role=listbox]').getBoundingClientRect(); return { iw: innerWidth, ih: innerHeight, list: [...document.querySelectorAll(${JSON.stringify(css)})].map((e) => e.getBoundingClientRect()).filter((r) => r.top >= box.top && r.bottom <= box.bottom && r.width > 4).map((r) => [r.left, r.top, r.width, r.height]) }`);
  const geo = (id) => execFileSync(XDO, ['getwindowgeometry', id]).toString().match(/Position: (\d+),(\d+).*\n.*Geometry: (\d+)x(\d+)/s);
  const id = execFileSync(XDO, ['search', '--class', '^nicevault$']).toString().trim().split('\n').find((i) => Number(geo(i)?.[3]) > 100);
  const g = geo(id);
  const ox = Number(g[1]) + (Number(g[3]) - rs.iw), oy = Number(g[2]) + (Number(g[4]) - rs.ih);
  return rs.list.map(([x, y, w, h]) => [Math.round(ox + x), Math.round(oy + y), Math.round(w), Math.round(h)]);
}

async function folderEnds(name, tag) {
  const before = (await log()).length;
  const runsBefore = await wd.exec('return (window.__nvRevealRuns || []).length');
  if (SLOW > 1) await wd.exec(`window.__nvRevealSlow = ${SLOW}`);
  const stop = frames(`5-${tag}`);
  await sleep(150);
  await dbl(name);
  await waitFor(`in ${name}`, `return document.querySelector(".breadcrumb [aria-current=page]")?.textContent.trim() === ${JSON.stringify(name)}`);
  await waitFor(`${name}: decode done`, `return (window.__nvReveal || []).slice(${before}).some((e) => e.kind === 'decode' && e.what === 'done')`, 8000 * SLOW).catch(() => null);
  await sleep(1500);
  const cap = await stop();
  await wd.exec('window.__nvRevealSlow = 1');
  const visible = await wd.exec(`const box = document.querySelector('[role=listbox]').getBoundingClientRect(); return [...document.querySelectorAll('[role=listbox] .file-item')].filter((e) => { const r = e.getBoundingClientRect(); return r.bottom > box.top && r.top < box.bottom; }).length`);
  const runs = (await wd.exec('return window.__nvRevealRuns || []')).slice(runsBefore).filter((r) => r.dir > 0 && r.items > 0);
  const run = runs[0];
  const okRun = !!run && run.items >= visible && run.why === 'natural' && run.at >= 0.95 * run.end;
  if (!okRun) failed++;
  console.log(`${okRun ? 'ok  ' : 'FAIL'} ${name}: every item on screen decodes to its end — ${visible} visible, run ${JSON.stringify(run)}`);
  const media = (await log()).slice(before).filter((e) => e.kind === 'media');
  const twice = media.filter((e) => e.what === 'play').length;
  const okOnce = twice === 0;
  if (!okOnce) failed++;
  console.log(`${okOnce ? 'ok  ' : 'FAIL'} ${name}: photos decode once (with their piece) — media ${media.map((e) => e.what).join(', ') || 'none'}`);
  const boxes = await screenRects('[role=listbox] .file-item .visual');
  const bf = join(cap.dir, 'boxes.json');
  writeFileSync(bf, JSON.stringify(boxes));
  const ends = JSON.parse(execFileSync('python3', [join(HERE, 'decode-end.py'), cap.dir, bf]).toString());
  const seen = ends.filter((e) => e.glyphs);
  const worst = seen.reduce((a, e) => (e.last < a.last ? e : a), { last: 1 });
  const okEnd = seen.length >= Math.min(boxes.length, visible) * 0.9 && worst.last >= 0.7;
  if (!okEnd) failed++;
  console.log(`${okEnd ? 'ok  ' : 'FAIL'} ${name}: each piece's last decode frame is resolved — ${seen.length}/${boxes.length} decoded in the frames, worst last frame ${worst.last} (box ${worst.box}: ${JSON.stringify(worst.tail)}) -> ${cap.dir}/end-${worst.box}.png, ${cap.n} frames every ~${cap.every} ms`);
}

async function main() {
  let stop = frames('1-boot');
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await waitFor('app ready', 'return document.querySelector("[data-ready=true]")');
  await played('boot: Benvenuto decodes', '^start:welcome$');
  console.log('     watchdog fired', await wd.exec('return window.__nvRevealDegraded || 0'), 'time(s) at boot');
  await sleep(300);
  let cap = await stop();
  console.log('     frames', JSON.stringify(cap));
  await noFlash('boot: Benvenuto', cap, '.welcome .intro', true);

  const vol = join(WORK, 'stick', 'Prova.nvault');
  const info = await invoke('vault_create', { path: vol, password: PASSWORD, kdf: 'fast' });
  if (!info.ok) throw new Error(JSON.stringify(info));
  const id = info.ok.id;
  const dir = await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Foto' });
  const job = await invoke('fs_import', { vault: id, parent: dir.ok.id, host_paths: [join(WORK, 'host', 'photo.jpg'), join(WORK, 'host', 'photo 2.jpg')] });
  if (!job.ok) throw new Error(JSON.stringify(job));
  const many = await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Molti' });
  const hostMany = [];
  for (let i = 1; i <= 24; i++) {
    const f = join(WORK, 'host', `nota ${String(i).padStart(2, '0')}.txt`);
    writeFileSync(f, `nota ${i}\n`);
    hostMany.push(f);
  }
  for (const n of ['Lettere', 'Ricevute', 'Scansioni', 'Varie']) await invoke('fs_mkdir', { vault: id, parent: many.ok.id, name: n });
  await invoke('fs_import', { vault: id, parent: many.ok.id, host_paths: hostMany });
  const icons = await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Pezzi' });
  for (const n of ['Buste', 'Carte']) await invoke('fs_mkdir', { vault: id, parent: icons.ok.id, name: n });
  await invoke('fs_import', { vault: id, parent: icons.ok.id, host_paths: ['photo.jpg', 'photo 2.jpg', 'document.pdf', 'clip.webm', 'readme.txt', 'export.csv', ...hostMany.slice(0, 6).map((f) => f.split('/').pop())].map((f) => join(WORK, 'host', f)) });
  const lots = await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Tanti' });
  const hostLots = [];
  for (let i = 1; i <= 135; i++) {
    const f = join(WORK, 'host', `voce ${String(i).padStart(3, '0')}.txt`);
    writeFileSync(f, `voce ${i}\n`);
    hostLots.push(f);
  }
  for (let i = 1; i <= 15; i++) {
    const f = join(WORK, 'host', `scatto ${String(i).padStart(2, '0')}.jpg`);
    writeFileSync(f, readFileSync(join(WORK, 'host', i % 2 ? 'photo.jpg' : 'photo 2.jpg')));
    hostLots.push(f);
  }
  await invoke('fs_import', { vault: id, parent: lots.ok.id, host_paths: hostLots });
  await sleep(1500);
  await invoke('vault_lock', { vault: id });
  await wd.exec('location.reload()');
  await sleep(300);
  await waitFor('app ready', 'return document.querySelector("[data-ready=true]") && document.querySelector(".passport .book")');

  stop = frames('2-unlock');
  const book = await wd.find('.passport .book');
  await wd.click(book);
  await played('Sblocca decodes on arrival', '^start:unlock$');
  await sleep(300);
  cap = await stop();
  console.log('     frames', JSON.stringify(cap));
  await noFlash('Sblocca', cap, 'article.document', true);
  stop = frames('2b-ceremony');
  const pw = await wd.find('input[type=password]');
  await wd.sendKeys(pw, PASSWORD + KEY.Enter);
  await waitFor('vault', 'return document.querySelector("[data-screen=vault]")', 20000);
  await played('unlock with Enter: the file view decodes in the ceremony', '^ceremony:');
  await sleep(300);
  cap = await stop();
  console.log('     frames', JSON.stringify(cap));
  await noFlash('after the unlock: the file view', cap, '[role=listbox] .items');
  await noBlank('unlock ceremony', cap);
  const focus = await wd.exec('const a = document.activeElement; return { inVault: !!a?.closest(".vault-screen"), role: a?.getAttribute("role") }');
  const selBefore = await wd.exec('return document.querySelector("[role=listbox]").getAttribute("aria-activedescendant")');
  await wd.keys(KEY.ArrowRight);
  const selAfter = await wd.exec('return document.querySelector("[role=listbox]").getAttribute("aria-activedescendant")');
  const okFocus = focus.inVault && focus.role === 'listbox' && !!selAfter && selAfter !== selBefore;
  if (!okFocus) failed++;
  console.log(`${okFocus ? 'ok  ' : 'FAIL'} after the unlock the keyboard works at once: focus ${JSON.stringify(focus)}, arrow moved ${selBefore} -> ${selAfter}`);

  stop = frames('3-folder');
  await waitFor('Molti listed', 'return [...document.querySelectorAll("[data-testid=item-name]")].some((e) => e.textContent.trim() === "Molti")');
  await wd.exec(SAMPLER);
  await wd.exec('document.querySelector("[role=listbox]").focus()');
  await wd.keys(KEY.Home);
  await wd.keys(KEY.ArrowRight);
  await wd.keys(KEY.Enter);
  await played('folder opened with Enter decodes', '^[0-9a-f]{8,}:\\d+$');
  const s3 = await wd.exec('return window.__nvSample.stop()');
  const loadingAt = s3.findIndex((f) => f.items === 0);
  const after = loadingAt >= 0 ? s3.slice(loadingAt) : [];
  const firstItems = after.find((f) => f.items > 0);
  const maxHeld = Math.max(0, ...after.map((f) => f.held));
  const vis = firstItems?.visible ?? 0;
  const skel = after.some((f) => f.skeleton);
  const handover = firstItems && firstItems.held >= firstItems.visible;
  const every = vis > 0 && maxHeld >= vis;
  if (!skel || !handover || !every) failed++;
  console.log(`${skel && handover && every ? 'ok  ' : 'FAIL'} every visible item decodes: skeleton shown ${skel}, ${vis} visible, ${firstItems?.held} held on the first frame with items, max held ${maxHeld}`);
  console.log('     frames', JSON.stringify(await stop()));

  const before = (await log()).length;
  await wd.exec('window.dispatchEvent(new MouseEvent("mouseup", { button: 3, bubbles: true }))');
  await waitFor('back at the root', 'return document.querySelector(".breadcrumb [aria-current=page]")?.textContent.trim() === "Prova"');
  await sleep(500);
  const sel = await wd.exec('return [...document.querySelectorAll("[role=option][aria-selected=true] [data-testid=item-name]")].map((e) => e.textContent.trim())');
  const again = (await log()).slice(before).filter((e) => e.what === 'play' && e.kind === 'decode');
  const okBack = sel.join() === 'Molti' && again.length === 0;
  if (!okBack) failed++;
  console.log(`${okBack ? 'ok  ' : 'FAIL'} mouse back: parent shown, selection ${JSON.stringify(sel)}, decodes played ${again.length}`);
  await wd.exec('window.dispatchEvent(new MouseEvent("mouseup", { button: 4, bubbles: true }))');
  await waitFor('forward in Molti', 'return document.querySelector(".breadcrumb [aria-current=page]")?.textContent.trim() === "Molti"');
  console.log('ok   mouse forward: Molti again');
  await wd.exec('window.dispatchEvent(new MouseEvent("mouseup", { button: 3, bubbles: true }))');
  await waitFor('back at the root', 'return document.querySelector(".breadcrumb [aria-current=page]")?.textContent.trim() === "Prova"');

  stop = frames('5-quicklook');
  await dbl('Foto');
  await waitFor('in Foto', 'return document.querySelector(".breadcrumb [aria-current=page]")?.textContent.trim() === "Foto"');
  await mediaPlayed('thumbnails decode on first showing', 'thumb:');
  await wd.exec(QL_SAMPLER);
  await dbl('photo.jpg');
  await waitFor('quick look', 'return document.querySelector("dialog.quick-look[open] img")');
  await sleep(1200);
  const q = await wd.exec('return window.__nvQl.stop()');
  const shown = q.findIndex((f) => f.visible);
  const blankAfter = shown >= 0 && q.slice(shown).some((f) => !f.visible && !f.glyphs);
  const srcs = await wd.exec('return window.__nvQl.srcs');
  const okQl = shown >= 0 && !blankAfter && srcs <= 1;
  if (!okQl) failed++;
  console.log(`${okQl ? 'ok  ' : 'FAIL'} quick look: ${q.length} frames, first visible at ${shown}, blank after shown: ${blankAfter}, src changes ${srcs}`);
  console.log('     frames', JSON.stringify(await stop()));

  await wd.keys(KEY.Escape);
  await sleep(1200);

  await wd.exec('window.dispatchEvent(new MouseEvent("mouseup", { button: 3, bubbles: true }))');
  await waitFor('back at the root', 'return document.querySelector(".breadcrumb [aria-current=page]")?.textContent.trim() === "Prova"');
  await sleep(1200);
  await folderEnds('Pezzi', 'pieces');
  await wd.exec('window.dispatchEvent(new MouseEvent("mouseup", { button: 3, bubbles: true }))');
  await waitFor('back at the root', 'return document.querySelector(".breadcrumb [aria-current=page]")?.textContent.trim() === "Prova"');
  await wd.exec(`document.querySelector('[data-testid=view-size] [data-size=s]').closest('button').click()`);
  const scr = await wd.exec('return { w: screen.width, h: screen.height }');
  const big = scr.w >= 2400 && scr.h >= 1500;
  if (big) await wd.cmd('POST', '/window/rect', { x: 0, y: 0, width: scr.w - 40, height: scr.h - 60 }).catch((e) => console.log('     (window not resized:', e.message, ')'));
  await sleep(1200);
  await folderEnds('Tanti', 'many');
  if (big) {
    await wd.cmd('POST', '/window/rect', { x: 0, y: 0, width: 1200, height: 780 }).catch(() => {});
    await sleep(600);
  }
  await wd.exec(`document.querySelector('[data-testid=view-size] [data-size=m]').closest('button').click()`);
  await sleep(400);

  stop = frames('6-lock');
  await sleep(150);
  await wd.exec('document.querySelector("button.btn.lock").click()');
  await waitFor('Sblocca again', 'return document.querySelector("[data-screen=unlock]") && !document.querySelector(".vault-screen")', 8000);
  await sleep(500);
  cap = await stop();
  console.log('     frames', JSON.stringify(cap));
  await noBlank('lock', cap);
  console.log('     watchdog fired', await wd.exec('return window.__nvRevealDegraded || 0'), 'time(s) since the reload');
  console.log('     keys', JSON.stringify(await wd.exec('return window.__k')));
  const all = await log();
  console.log('     log', JSON.stringify(all));
  await wd.quit();
  process.exit(failed ? 1 : 0);
}

main().catch(async (e) => {
  console.log('FAIL', e.message);
  try {
    await wd?.quit();
  } catch {
  }
  process.exit(1);
});
