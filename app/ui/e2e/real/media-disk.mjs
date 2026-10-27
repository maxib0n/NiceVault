import { execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { readdirSync, readFileSync, readlinkSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { WebDriver } from './webdriver.mjs';

const WORK = process.env.NV_E2E_WORK;
const APP = process.env.NV_E2E_APP;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function bigVideo() {
  if (process.env.NV_E2E_MEDIA) return process.env.NV_E2E_MEDIA;
  const out = join(WORK, 'host', 'big.webm');
  execFileSync('ffmpeg', ['-loglevel', 'error', '-y', '-f', 'lavfi', '-i', 'testsrc2=size=1280x720:rate=30', '-t', '50', '-c:v', 'libvpx', '-b:v', '6M', '-deadline', 'realtime', '-cpu-used', '8', out]);
  return out;
}

function webProcesses() {
  const pids = [];
  for (const p of readdirSync('/proc')) {
    if (!/^\d+$/.test(p)) continue;
    try {
      if (!readFileSync(`/proc/${p}/comm`, 'utf8').startsWith('WebKitWebProces')) continue;
      const env = readFileSync(`/proc/${p}/environ`, 'utf8').split('\0');
      if (env.includes(`HOME=${WORK}/home`)) pids.push(p);
    } catch {
    }
  }
  return pids;
}

function mediaFds() {
  const hits = [];
  for (const p of webProcesses()) {
    let fds = [];
    try {
      fds = readdirSync(`/proc/${p}/fd`);
    } catch (e) {
      hits.push(`${p}: fd list unreadable (${e.code})`);
      continue;
    }
    for (const fd of fds) {
      try {
        const l = readlinkSync(`/proc/${p}/fd/${fd}`);
        if (l.includes('WebKit-Media')) {
          let size = '?';
          try {
            size = statSync(`/proc/${p}/fd/${fd}`).size;
          } catch {
          }
          hits.push(`${p}/fd/${fd} -> ${l} (${size} bytes)`);
        }
      } catch {
      }
    }
  }
  return hits;
}

const video = readFileSync(bigVideo());
const requests = [];
const server = createServer((req, res) => {
  requests.push({ method: req.method, url: req.url, headers: req.headers });
  const size = video.length;
  let start = 0;
  let end = size - 1;
  const m = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range ?? '');
  if (m) {
    if (m[1]) start = Number(m[1]);
    if (m[2]) end = Math.min(Number(m[2]), size - 1);
    if (!m[1] && m[2]) [start, end] = [size - Number(m[2]), size - 1];
    res.writeHead(206, { 'Content-Type': 'video/webm', 'Accept-Ranges': 'bytes', 'Content-Length': end - start + 1, 'Content-Range': `bytes ${start}-${end}/${size}` });
  } else {
    res.writeHead(200, { 'Content-Type': 'video/webm', 'Accept-Ranges': 'bytes', 'Content-Length': size });
  }
  if (req.method === 'HEAD') return res.end();
  res.end(video.subarray(start, end + 1));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

const wd = new WebDriver(process.env.NV_E2E_DRIVER);
await wd.start({ 'tauri:options': { application: APP } });
for (let i = 0; i < 100 && !(await wd.exec('return !!document.querySelector("[data-ready=true]")')); i++) await sleep(150);

const invoke = (cmd, args) =>
  wd.execAsync('const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]).then((v) => done({ ok: v }), (e) => done({ err: e }));', cmd, args);

async function scenario(name, srcExpr, { loop = false } = {}) {
  await wd.execAsync(
    `const done = arguments[arguments.length - 1];
     (async () => {
       document.getElementById('nv-probe')?.remove();
       const v = document.createElement('video');
       v.id = 'nv-probe'; v.muted = true; v.preload = 'metadata'; v.loop = ${loop};
       v.style = 'position:fixed;left:0;top:0;width:320px;z-index:99999';
       document.body.append(v);
       v.src = await (${srcExpr});
       v.play().catch((e) => (window.__nvPlayErr = String(e)));
       done(true);
     })().catch((e) => done(String(e)));`,
  );
  let seen = [];
  for (let i = 0; i < 16; i++) {
    await sleep(500);
    if (i === 8) await wd.exec('const v = document.getElementById("nv-probe"); if (v && Number.isFinite(v.duration)) v.currentTime = v.duration * 0.6;');
    const h = mediaFds();
    if (h.length) seen = h;
  }
  const st = await wd.exec(
    'const v = document.getElementById("nv-probe"); const r = v && { src: v.currentSrc.slice(0, 40), readyState: v.readyState, t: v.currentTime, dur: v.duration, err: v.error && [v.error.code, v.error.message], playErr: window.__nvPlayErr || null }; window.__nvPlayErr = null; return r;',
  );
  await wd.exec('const v = document.getElementById("nv-probe"); if (v) { v.pause(); v.removeAttribute("src"); v.load(); v.remove(); }');
  await sleep(1500);
  console.log(`${seen.length ? 'DISK' : 'ok  '} ${name}: ${JSON.stringify(st)}`);
  for (const h of seen) console.log(`       ${h}`);
  return { name, disk: seen.length > 0, st };
}

const out = [];
console.log(`web processes: ${webProcesses().join(', ') || 'none found'}; WEBKIT_GST_ALLOWED_URI_PROTOCOLS=${process.env.WEBKIT_GST_ALLOWED_URI_PROTOCOLS ?? ''}`);
out.push(await scenario('plain http (node server)', `'http://127.0.0.1:${port}/v.webm'`));
const firstMedia = requests.find((r) => r.method === 'GET');
console.log(`     media request headers: ${JSON.stringify(firstMedia?.headers)}`);
console.log(`     all requests: ${requests.map((r) => `${r.method} ${r.headers.range ?? '-'} origin=${r.headers.origin ?? '-'} referer=${r.headers.referer ?? '-'}`).join(' | ')}`);
out.push(await scenario('http + loop', `'http://127.0.0.1:${port}/v.webm'`, { loop: true }));

const vol = join(WORK, 'stick', 'M.nvault');
const created = await invoke('vault_create', { path: vol, password: 'media disk check pw', kdf: 'fast' });
if (created.err) throw new Error(JSON.stringify(created.err));
const vid = created.ok.id;
const src = process.env.NV_E2E_MEDIA ?? join(WORK, 'host', 'big.webm');
await invoke('fs_import', { vault: vid, parent: 1, host_paths: [src] });
let node;
for (let i = 0; i < 200 && !node; i++) {
  await sleep(150);
  const l = await invoke('fs_list', { vault: vid, dir: 1, filters: null });
  node = l.ok?.find((n) => n.size === video.length);
}
if (!node) throw new Error('import did not finish');
const url = await invoke('media_url', { vault: vid, node: node.id });
if (!url.ok?.startsWith('http://127.0.0.1:')) throw new Error(`media_url did not stream a file over MEDIA_RAM_MAX: ${JSON.stringify(url)}`);
await invoke('media_release', { url: url.ok });
const ticket = `window.__TAURI_INTERNALS__.invoke('media_url', { vault: ${JSON.stringify(vid)}, node: ${node.id} })`;
const nv = `'nvault://' + encodeURIComponent(${JSON.stringify(vid)}) + '/${node.id}'`;
const raw = await scenario('app loopback stream without loop', ticket);
const looped = await scenario('app loopback stream + loop (MediaViewer)', ticket, { loop: true });
const scheme = await scenario('nvault:// scheme', nv);
const blob = await scenario(
  'blob from nvault:// (ranged fetches, MediaViewer up to MEDIA_RAM_MAX)',
  `(async () => { const parts = []; for (let o = 0; o < ${video.length}; o += 8 << 20) { const r = await fetch(${nv}, { headers: { Range: 'bytes=' + o + '-' + (Math.min(${video.length}, o + (8 << 20)) - 1) } }); parts.push(await r.arrayBuffer()); } return URL.createObjectURL(new Blob(parts, { type: 'video/webm' })); })()`,
);
out.push(raw, looped, scheme, blob);
await invoke('vault_lock', { id: vid }).catch(() => undefined);
await wd.quit().catch(() => undefined);
server.close();
console.log(JSON.stringify(out.map((o) => ({ [o.name]: o.disk ? 'disk' : o.st?.err ? 'error' : 'no disk' }))));
const fails = [];
const played = (o) => o.st && !o.st.err && o.st.readyState >= 2 && o.st.t > 1;
if (looped.disk || !played(looped)) fails.push('the looping stream buffered on disk or did not play');
if (blob.disk || !played(blob)) fails.push('the blob buffered on disk or did not play');
if (!raw.disk) console.log('note: WebKit no longer disk-buffers a plain http stream (mechanism gone?)');
if (!scheme.st?.err) console.log('note: nvault:// plays in <video> now: the loopback stream may be unnecessary');
for (const f of fails) console.log(`FAIL ${f}`);
process.exit(fails.length ? 1 : 0);
