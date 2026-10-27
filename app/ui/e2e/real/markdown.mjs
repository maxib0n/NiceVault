import { copyFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { KEY, WebDriver } from './webdriver.mjs';

const APP = process.env.NV_E2E_APP;
const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const DRIVER = process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444';
const SAMPLES = join(dirname(fileURLToPath(import.meta.url)), '../../public/mock/v');
const PASSWORD = 'cavallo batteria graffetta corretta';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let wd;
let failed = 0;

const invoke = (cmd, args) =>
  wd.cmd('POST', '/execute/async', {
    script: 'const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then((r) => done({ ok: r }), (e) => done({ err: e }));',
    args: [cmd, args],
  });

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
    await sleep(100);
  }
  throw new Error(`timeout: ${desc} (last ${JSON.stringify(last)})`);
}

function check(ok, msg) {
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${msg}`);
}

const shot = async (name) => writeFileSync(join(OUT, `markdown-${name}.png`), await wd.screenshot());

const WATCH = `
  if (!window.__nvMdWatch) {
    window.__nvMdWatch = true;
    window.__nvCsp = [];
    window.__nvAlerts = 0;
    document.addEventListener('securitypolicyviolation', (e) => window.__nvCsp.push(e.violatedDirective + ' ' + (e.blockedURI || '') + ' ' + (e.sample || '')));
    window.alert = () => window.__nvAlerts++;
  }
`;

async function open(name) {
  await wd.exec(`const o = [...document.querySelectorAll('[role=listbox] [role=option]')].find((o) => o.querySelector('[data-testid=item-name]')?.textContent.trim() === ${JSON.stringify(name)}); o.click();`);
  await wd.keys(KEY.Space);
  await waitFor(`quick look ${name}`, `return document.querySelector('[data-testid=viewer-markdown][data-mode=rendered] article')`);
}
async function close() {
  await wd.keys(KEY.Escape);
  await waitFor('quick look closed', 'return !document.querySelector("[data-testid=viewer-markdown]")');
}

async function main() {
  wd = new WebDriver(DRIVER);
  await wd.start({ 'tauri:options': { application: APP } });
  await waitFor('app ready', 'return document.querySelector("[data-ready=true]")');
  const host = join(WORK, 'host');
  copyFileSync(join(SAMPLES, 'taccuino.md'), join(host, 'Taccuino di casa.md'));
  copyFileSync(join(SAMPLES, 'sospetto.md'), join(host, 'Nota sospetta.md'));
  copyFileSync(join(SAMPLES, 'lago.webp'), join(host, 'Lago.webp'));
  const vol = join(WORK, 'stick', 'Markdown.nvault');
  const info = await invoke('vault_create', { path: vol, password: PASSWORD, kdf: 'fast' });
  if (!info.ok) throw new Error(JSON.stringify(info));
  const id = info.ok.id;
  await invoke('fs_import', { vault: id, parent: 1, host_paths: ['Taccuino di casa.md', 'Nota sospetta.md', 'Lago.webp'].map((n) => join(host, n)) });
  await sleep(1500);
  await invoke('vault_lock', { vault: id });
  await wd.exec('location.reload()');
  await sleep(300);
  await waitFor('board', 'return document.querySelector("[data-ready=true]") && document.querySelector(".passport .book")');
  await wd.click(await wd.find('.passport .book'));
  const pw = await waitFor('password field', 'return document.querySelector("input[type=password]")');
  await wd.sendKeys(pw, PASSWORD + KEY.Enter);
  await waitFor('vault', 'return document.querySelector("[data-screen=vault]") && document.querySelectorAll("[role=option]").length >= 3', 20000);
  await sleep(1200);
  await wd.exec(WATCH);

  await open('Taccuino di casa.md');
  const md = '[data-testid=viewer-markdown]';
  const st = await waitFor('math + diagram', `const m = document.querySelector('${md}'); return m && m.dataset.math !== 'loading' && m.dataset.mermaid !== 'loading' && m.dataset.pictures === '1' && { math: m.dataset.math, mermaid: m.dataset.mermaid }`, 30000);
  check(st.math === 'ready', `KaTeX rendered (${st.math})`);
  const why = st.mermaid === 'ready' ? '' : await wd.exec(`const m = document.querySelector('${md} .md-body'); return m.dataset.error || m.querySelector('.md-mermaid')?.dataset.error`);
  check(st.mermaid === 'ready', `Mermaid rendered under the CSP (${st.mermaid}${why ? ': ' + why : ''})`);
  const facts = await wd.exec(`const m = document.querySelector('${md}');
    const svg = m.querySelector('.md-mermaid svg');
    const img = m.querySelector('img.md-pic');
    return { katex: m.querySelectorAll('.katex').length, svgText: svg ? svg.textContent.includes('Copia cifrata') : false,
      img: img && { src: img.src.slice(0, 24), ok: img.complete && img.naturalWidth > 0 }, csp: window.__nvCsp, alerts: window.__nvAlerts }`);
  check(facts.katex >= 2, `KaTeX formulas: ${facts.katex}`);
  check(facts.svgText, 'the diagram has its labels (SVG text)');
  check(!!facts.img?.ok && /^(nvault:|http:\/\/nvault\.)/.test(facts.img.src), `the volume's own picture shows (${JSON.stringify(facts.img)})`);
  await shot('rich-top');
  await wd.exec(`document.querySelector('${md} .md-mermaid').scrollIntoView({ block: 'center' })`);
  await sleep(400);
  await shot('rich-diagram');
  await close();

  await open('Nota sospetta.md');
  await waitFor('hostile settled', `const m = document.querySelector('${md}'); return m && m.dataset.mermaid !== 'loading' && m.dataset.math !== 'loading'`, 30000);
  await sleep(800);
  const bad = await wd.exec(`const m = document.querySelector('${md}');
    return { tags: [...m.querySelectorAll('script,iframe,img,form,input,object,embed,foreignObject')].map((e) => e.tagName),
      handlers: [...m.querySelectorAll('*')].flatMap((e) => e.getAttributeNames().filter((n) => n.startsWith('on'))),
      hrefs: [...m.querySelectorAll('a[href]')].map((a) => a.getAttribute('href')).filter((h) => !/^(https?:\\/\\/|mailto:|#)/.test(h)),
      alerts: window.__nvAlerts, url: location.href }`);
  check(bad.tags.length === 0, `no loading/running elements (${bad.tags})`);
  check(bad.handlers.length === 0, `no event handlers (${bad.handlers})`);
  check(bad.hrefs.length === 0, `only safe links (${bad.hrefs})`);
  check(bad.alerts === 0, 'no script ran');
  await shot('hostile');
  await close();
  const csp = await wd.exec('return window.__nvCsp');
  check(csp.length === 0, `no CSP violation (${JSON.stringify(csp)})`);
  console.log(failed ? `\n${failed} check(s) FAILED` : '\nall markdown checks passed');
}

main()
  .catch((e) => {
    console.error(e);
    failed++;
  })
  .finally(async () => {
    try {
      await wd?.cmd('DELETE', '');
    } catch {
    }
    process.exit(failed ? 1 : 0);
  });
