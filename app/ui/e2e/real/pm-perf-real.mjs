import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pmProbeSource } from '../../scripts/pm-perf.mjs';
import { WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const N = Number(process.env.NV_E2E_PM_ENTRIES ?? 2000);
const RUNS = Number(process.env.NV_E2E_PM_RUNS ?? 3);
const PASSWORD = 'cavallo batteria graffetta corretta';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const LIB = `
  const norm = (s) => (s || '').replace(/\\s+/g, ' ').trim();
  const btn = (name) => [...document.querySelectorAll('button')].find((b) => norm(b.getAttribute('aria-label') || b.textContent) === name);
  const byLabel = (text) => { const l = [...document.querySelectorAll('label')].find((l) => norm(l.textContent) === text); return l ? document.getElementById(l.htmlFor) : null; };
  const screen = () => document.querySelector('[data-screen]')?.dataset.screen;
`;
let wd;
const q = (body, ...args) => wd.exec(`${LIB}\n${body}`, ...args);
async function waitFor(desc, body, timeout = 30000) {
  const t0 = Date.now();
  let last;
  while (Date.now() - t0 < timeout) {
    try {
      last = await q(body);
      if (last) return last;
    } catch (e) {
      last = e.message;
    }
    await sleep(150);
  }
  throw new Error(`timeout waiting for ${desc} (last: ${JSON.stringify(last)})`);
}
const clickButton = async (name) => wd.click(await waitFor(`button ${name}`, `return btn(${JSON.stringify(name)})`));
const run = (body) => wd.execAsync(`${LIB}\nconst done = arguments[arguments.length - 1];\n${pmProbeSource}\n(async () => { ${body} })().then(done, (e) => done({ error: String(e) }));`);

function csv(n) {
  const sites = ['posta', 'banca', 'forum', 'negozio', 'cloud', 'giochi', 'viaggi', 'lavoro', 'scuola', 'salute', 'news', 'musica'];
  const folders = ['', 'Personale', 'Personale/Banche', 'Lavoro', 'Lavoro/Clienti', 'Casa', 'Svago', ''];
  const rows = ['name,url,username,password,folder,totp'];
  for (let i = 0; i < n; i++) {
    const s = sites[i % sites.length];
    const k = String(i + 1).padStart(4, '0');
    rows.push(`${s[0].toUpperCase()}${s.slice(1)} ${k},https://${s}${i % 50}.example.com/login,utente${i % 37}@example.it,pw-${s}-${k},${folders[i % folders.length]},${i % 9 === 0 ? 'JBSWY3DPEHPK3PXP' : ''}`);
  }
  return rows.join('\n') + '\n';
}

const median = (v) => [...v].sort((a, b) => a - b)[Math.floor(v.length / 2)];

async function main() {
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await waitFor('app ready', 'return document.querySelector("[data-ready=true]")');
  await clickButton('Nuovo volume');
  const name = await waitFor('name', 'return byLabel("Nome")');
  await wd.clear(name);
  await wd.sendKeys(name, 'Perf');
  await clickButton('Avanti');
  await wd.sendKeys(await waitFor('pw', 'return byLabel("Password")'), PASSWORD);
  await wd.sendKeys(await waitFor('confirm', 'return byLabel("Ripeti la password")'), PASSWORD);
  await waitFor('strong', 'return btn("Crea volume") && !btn("Crea volume").disabled');
  await clickButton('Crea volume');
  await waitFor('created', 'return document.querySelector("[data-step=\\"3\\"]")', 60000);
  await clickButton('Apri il volume');
  await waitFor('vault', 'return screen() === "vault"');

  const t0 = Date.now();
  const imp = await run(`
    const inv = window.__TAURI_INTERNALS__.invoke;
    const [v] = await inv('vault_list_open');
    await inv('pm_import', { vault: v.id, kind: 'csv', path: null, data: ${JSON.stringify(csv(N))}, master_password: null, keyfile: null });
    for (let i = 0; i < 600; i++) {
      const n = (await inv('pm_list', { vault: v.id })).length;
      if (n >= ${N}) return n;
      await new Promise((r) => setTimeout(r, 500));
    }
    return -1;`);
  if (imp !== N) throw new Error(`import: ${JSON.stringify(imp)}`);
  console.log(`imported ${N} entries in ${Date.now() - t0} ms`);

  const runs = [];
  for (let r = 0; r < RUNS; r++) {
    await wd.click(await waitFor('Password nav', 'return screen() !== "passwords" && document.querySelector("[data-testid=nav-passwords]")'));
    await waitFor('passwords', 'return screen() === "passwords" && document.querySelectorAll("[data-testid=pm-entry]").length > 0');
    const first = await run(`
      document.querySelector('[data-testid=nav-files]').click();
      for (let i = 0; i < 100 && document.querySelector('[data-testid=pm-list]'); i++) await new Promise((r) => setTimeout(r, 50));
      await new Promise((r) => setTimeout(r, 500));
      const t = performance.now();
      document.querySelector('[data-testid=nav-passwords]').click();
      await new Promise((resolve) => {
        const check = () => document.querySelectorAll('[data-testid=pm-entry]').length > 0 ? requestAnimationFrame(() => setTimeout(resolve, 0)) : requestAnimationFrame(check);
        check();
      });
      return performance.now() - t;`);
    await sleep(800);
    const m = await run(`
      const rows = document.querySelectorAll('[data-testid=pm-entry]').length;
      const idle = await window.__pmProbe.idle(2000);
      const scroll = await window.__pmProbe.scroll(3000);
      await new Promise((r) => setTimeout(r, 300));
      const fling = await window.__pmProbe.scroll(3000, 2400);
      const search = await window.__pmProbe.search('0015');
      const select = await window.__pmProbe.select(10);
      return { rows, idle, scroll, fling, search, select };`);
    if (m?.error) throw new Error(m.error);
    runs.push({ first_render_ms: +first.toFixed(1), ...m });
    console.log(`run ${r + 1}: first ${first.toFixed(0)} ms, rows ${m.rows}, idle worst ${m.idle.worst_ms} (>16.7: ${m.idle.over16}), scroll ${m.scroll.avg_fps} fps (worst ${m.scroll.worst_ms}), fling ${m.fling.avg_fps} fps (worst ${m.fling.worst_ms}, >16.7: ${m.fling.over16}), search max ${m.search.max_list_ms} ms, select max ${m.select.max_ms} ms`);
    await wd.click(await waitFor('Files nav', 'return document.querySelector("[data-testid=nav-files]")'));
    await waitFor('vault', 'return screen() === "vault"');
  }
  const summary = {
    entries: N,
    runs: runs.length,
    first_render_ms: median(runs.map((r) => r.first_render_ms)),
    rows_in_dom: median(runs.map((r) => r.rows)),
    idle_worst_ms: median(runs.map((r) => r.idle.worst_ms)),
    idle_over16: median(runs.map((r) => r.idle.over16)),
    scroll_fps: median(runs.map((r) => r.scroll.avg_fps)),
    scroll_worst_ms: median(runs.map((r) => r.scroll.worst_ms)),
    fling_fps: median(runs.map((r) => r.fling.avg_fps)),
    fling_worst_ms: median(runs.map((r) => r.fling.worst_ms)),
    fling_over16: median(runs.map((r) => r.fling.over16)),
    search_max_ms: median(runs.map((r) => r.search.max_list_ms)),
    search_echo_max_ms: median(runs.map((r) => Math.max(...r.search.keystroke_echo_ms))),
    search_worst_frame_ms: median(runs.map((r) => r.search.frames.worst_ms)),
    select_max_ms: median(runs.map((r) => r.select.max_ms)),
  };
  console.log(JSON.stringify(summary, null, 2));
  writeFileSync(join(OUT, 'pm-perf-real.json'), JSON.stringify({ summary, runs }, null, 2));
  await wd.quit();
  void WORK;
}

main().catch(async (e) => {
  console.error('fatal:', e);
  try {
    await wd?.quit();
  } catch {
  }
  process.exit(2);
});
