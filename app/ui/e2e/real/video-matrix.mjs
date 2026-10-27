import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { KEY, WebDriver } from './webdriver.mjs';

const WORK = process.env.NV_E2E_WORK;
const APP = process.env.NV_E2E_APP;
const OUT = process.env.NV_E2E_OUT;
const PASSWORD = 'cavallo batteria graffetta corretta';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function makeVideos() {
  if (process.env.NV_E2E_VIDEOS) return process.env.NV_E2E_VIDEOS;
  const dir = join(WORK, 'host', 'videos');
  mkdirSync(dir, { recursive: true });
  const src = (size) => ['-f', 'lavfi', '-i', `testsrc2=size=${size}:rate=30`, '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000'];
  const ff = (args, out) => execFileSync('ffmpeg', ['-loglevel', 'error', '-y', ...args, join(dir, out)]);
  const small = src('1280x720');
  ff([...small, '-t', '8', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-movflags', '+faststart'], 'h264.mp4');
  ff([...small, '-t', '8', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac'], 'h264-moov-end.mov');
  ff([...small, '-t', '8', '-c:v', 'libx265', '-tag:v', 'hvc1', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-x265-params', 'log-level=error'], 'hevc.mov');
  ff([...small, '-t', '8', '-c:v', 'libx265', '-tag:v', 'hvc1', '-pix_fmt', 'yuv420p10le', '-c:a', 'aac', '-x265-params', 'log-level=error'], 'hevc-10bit.mp4');
  ff([...small, '-t', '8', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac'], 'h264.mkv');
  ff([...small, '-t', '8', '-c:v', 'libvpx-vp9', '-deadline', 'realtime', '-cpu-used', '8', '-c:a', 'libopus'], 'vp9.webm');
  const big = src('1920x1080');
  ff([...big, '-t', '30', '-c:v', 'libx264', '-preset', 'ultrafast', '-b:v', '12M', '-pix_fmt', 'yuv420p', '-c:a', 'aac'], 'h264-big.mov');
  ff([...big, '-t', '30', '-c:v', 'libx265', '-preset', 'ultrafast', '-b:v', '12M', '-tag:v', 'hvc1', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-x265-params', 'log-level=error'], 'hevc-big.mp4');
  return dir;
}

function webProcesses() {
  const pids = [];
  for (const p of readdirSync('/proc')) {
    if (!/^\d+$/.test(p)) continue;
    try {
      if (!readFileSync(`/proc/${p}/comm`, 'utf8').startsWith('WebKitWebProces')) continue;
      if (readFileSync(`/proc/${p}/environ`, 'utf8').split('\0').includes(`HOME=${WORK}/home`)) pids.push(p);
    } catch {
    }
  }
  return pids.sort();
}

const LIB = `
  const norm = (s) => (s || '').replace(/\\s+/g, ' ').trim();
  const accName = (el) => norm(el.getAttribute('aria-label') || el.textContent);
  const btn = (name) => [...document.querySelectorAll('button,[role=button]')].find((b) => accName(b) === name);
  const option = (name) => [...document.querySelectorAll('[role=listbox] [role=option]')].find((o) => norm(o.querySelector('[data-testid=item-name]')?.textContent) === name);
  const names = () => [...document.querySelectorAll('[role=listbox] [data-testid=item-name]')].map((e) => norm(e.textContent));
  const screen = () => document.querySelector('[data-screen]')?.dataset.screen;
  const byLabel = (text) => { const l = [...document.querySelectorAll('label')].find((l) => norm(l.textContent) === text); return l ? document.getElementById(l.htmlFor) : null; };
  const dialog = (name) => [...document.querySelectorAll('dialog[open]')].find((d) => !name || norm(d.getAttribute('aria-label')).startsWith(name) || norm(d.querySelector('h2')?.textContent).startsWith(name));
`;
let wd;
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
const click = async (desc, body, ...args) => wd.click(await waitFor(desc, body, { args }));
const clickButton = (name) => click(`button "${name}"`, 'return btn(arguments[0])', name);

const dir = makeVideos();
const videos = readdirSync(dir).filter((f) => /\.(mp4|mov|m4v|mkv|webm|avi)$/i.test(f)).sort();
wd = new WebDriver(process.env.NV_E2E_DRIVER);
await wd.start({ 'tauri:options': { application: APP } });
await waitFor('app ready', 'return document.querySelector("[data-ready=true]")');

await clickButton('Nuovo volume');
await waitFor('wizard', 'return screen() === "create"');
const name = await waitFor('name input', 'return byLabel("Nome")');
await wd.clear(name);
await wd.sendKeys(name, 'Video');
await clickButton('Avanti');
await wd.sendKeys(await waitFor('password', 'return byLabel("Password")'), PASSWORD);
await wd.sendKeys(await waitFor('confirm', 'return byLabel("Ripeti la password")'), PASSWORD);
await waitFor('strong accepted', 'return btn("Crea volume") && !btn("Crea volume").disabled');
await clickButton('Crea volume');
await waitFor('step 3', 'return document.querySelector("[data-step=\\"3\\"]")', { timeout: 30000 });
await clickButton('Apri il volume');
await waitFor('vault', 'return screen() === "vault"');

const at = await q('const r = document.querySelector("[role=listbox]").getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 3 }');
await wd.execAsync(
  'const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "tauri://drag-drop", payload: arguments[0] }).then(() => done(true), (e) => done(String(e)));',
  { paths: videos.map((v) => join(dir, v)), position: at },
);
await waitFor('videos imported', 'return arguments[0].every((n) => names().includes(n))', { args: [videos], timeout: 120000 });
await sleep(1000);

{
  const thumbs = await waitFor(
    'video thumbnails (blob: img)',
    'const n = [...document.querySelectorAll("[role=listbox] [role=option] img")].filter((i) => i.src.startsWith("blob:") && i.naturalWidth > 0).length; return n > 0 && n',
    { timeout: 90000 },
  );
  console.log(`video thumbnails shown: ${thumbs}`);
}

const procs0 = webProcesses();
console.log(`web process(es): ${procs0.join(', ')}`);
const results = [];
for (const v of videos) {
  const t0 = Date.now();
  let r = { name: v, ok: false };
  try {
    await click(`item ${v}`, 'return option(arguments[0])', v);
    await wd.keys(KEY.Space);
    await waitFor(`anteprima ${v}`, 'return dialog("Anteprima di " + arguments[0])', { args: [v] });
    const vid = 'dialog("Anteprima di")?.querySelector("video")';
    const meta = await waitFor(
      `${v} metadata`,
      `const e = ${vid}; const via = e?.closest('[data-via]')?.dataset.via;
       // (the nvault:// scheme is refused first on Linux: MediaViewer then swaps in a Blob)
       if (e?.error && via !== 'scheme') return { err: [e.error.code, e.error.message], via };
       const info = document.querySelector('[data-testid=viewer-info]'); if (info) return { info: norm(info.textContent).slice(0, 120) };
       return e && e.readyState >= 1 && e.videoWidth > 0 && { w: e.videoWidth, h: e.videoHeight, dur: e.duration, via: e.closest('[data-via]')?.dataset.via }`,
      { timeout: 30000 },
    );
    if (meta.err || meta.info) throw new Error(`not playable: ${JSON.stringify(meta)}`);
    await q(`const e = ${vid}; e.muted = true; void e.play()`);
    await waitFor(`${v} plays`, `const e = ${vid}; return e && !e.paused && e.currentTime > 0.5`, { timeout: 20000 });
    await q(`const e = ${vid}; e.currentTime = e.duration * 0.6`);
    const after = await waitFor(
      `${v} seeked`,
      `const e = ${vid}; if (e?.error) return { err: [e.error.code, e.error.message] }; return e && !e.seeking && !e.paused && e.currentTime > e.duration * 0.6 + 0.3 && { at: e.currentTime }`,
      { timeout: 20000 },
    );
    if (after.err) throw new Error(`error after seek: ${JSON.stringify(after.err)}`);
    const file = join(OUT, `video-${v.replace(/[^a-z0-9]+/gi, '-')}.png`);
    writeFileSync(file, await wd.screenshot());
    r = { name: v, ok: true, ...meta, at: Number(after.at.toFixed(2)), ms: Date.now() - t0 };
  } catch (e) {
    r.error = e.message;
  }
  try {
    await wd.exec('return 1');
    const procs = webProcesses();
    if (procs.join() !== procs0.join()) r = { ...r, ok: false, error: `${r.error ?? ''} web process changed: ${procs0} -> ${procs}` };
  } catch (e) {
    r = { ...r, ok: false, error: `${r.error ?? ''} APP GONE: ${e.message}` };
    results.push(r);
    console.log(`FAIL ${v}: ${r.error}`);
    break;
  }
  console.log(`${r.ok ? 'ok  ' : 'FAIL'} ${v}: ${JSON.stringify(r)}`);
  results.push(r);
  try {
    if (await q('return !!dialog("Anteprima di")')) {
      await wd.keys(KEY.Escape);
      await waitFor('closed', 'return !dialog("Anteprima di")');
    }
  } catch {
  }
}
await wd.quit().catch(() => undefined);
const failed = results.filter((r) => !r.ok);
console.log(`${results.length - failed.length}/${videos.length} videos played and seeked`);
if (existsSync(join(WORK, 'tauri-driver.log'))) {
  const log = readFileSync(join(WORK, 'tauri-driver.log'), 'utf8').split('\n').filter((l) => /gst|crash|abort|panic|signal|error/i.test(l));
  if (log.length) console.log('--- app / driver log lines:\n' + log.slice(-40).join('\n'));
}
process.exit(failed.length || results.length !== videos.length ? 1 : 0);
