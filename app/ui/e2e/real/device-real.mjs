import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WebDriver } from './webdriver.mjs';

const WORK = process.env.NV_E2E_WORK;
const OUT = process.env.NV_E2E_OUT;
const STOP = process.env.NV_DEVICE_STOP ?? 'done';
const WANT = process.env.NV_DEVICE_DISK ?? '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const wd = new WebDriver(process.env.NV_E2E_DRIVER);
const times = [];
let n = 0;

const trace = () => {
  try {
    return readFileSync(join(WORK, 'tauri-driver.log'), 'utf8');
  } catch {
    return '';
  }
};
async function shot(name) {
  writeFileSync(join(OUT, `dev-${String(++n).padStart(2, '0')}-${name}.png`), await wd.screenshot());
}
async function until(desc, script, timeout = 120000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeout) {
    const v = await wd.exec(script).catch(() => null);
    if (v) return v;
    await sleep(50);
  }
  throw new Error(`timeout: ${desc}`);
}
async function timed(name, click, ready) {
  const t0 = Date.now();
  await wd.exec(click);
  const r = await until(name, ready);
  times.push({ step: name, ms: Date.now() - t0 });
  console.log(`${name}: ${Date.now() - t0} ms`);
  return r;
}

try {
  if (!/\[device\] backend: (dry-run|fake)/.test(trace()) && !process.env.NICEVAULT_DEVICE_DRY_RUN && !process.env.NICEVAULT_FAKE_DISKS) throw new Error('not a dry run / fake backend: refusing');
  await wd.start({ 'tauri:options': { application: process.env.NV_E2E_APP } });
  await until('app ready', 'return !!document.querySelector("[data-ready=true]")');
  await timed('open wizard', 'document.querySelector("[data-testid=open-device]").click()', 'return !!document.querySelector("[data-testid=device-wizard]")');
  const disks = await until('drives listed', 'const d=[...document.querySelectorAll("[data-testid=device-disk]")]; return d.length ? d.map((e) => e.dataset.diskId + " | " + e.textContent.replace(/\\s+/g," ").trim()) : null', 30000);
  console.log('drives:', disks);
  const pick = disks.findIndex((d) => d.includes(WANT));
  if (pick < 0) throw new Error(`no drive matching "${WANT}"`);
  await wd.exec(`document.querySelectorAll("[data-testid=device-disk]")[${pick}].click()`);
  await shot('disk');
  await timed('Avanti -> options', 'document.querySelector("[data-testid=device-next]").click()', 'return document.querySelector("[data-testid=device-wizard]").dataset.stage === "options" || document.querySelector("[data-testid=device-form-error]")?.textContent');
  await shot('options');
  if (STOP !== 'options') {
    await timed('Avanti -> confirm', 'document.querySelector("[data-testid=device-next]").click()', 'return document.querySelector("[data-testid=device-wizard]").dataset.stage === "confirm" || document.querySelector("[data-testid=device-form-error]")?.textContent');
    const warn = await wd.exec('return document.querySelector("[data-testid=device-volumes]")?.textContent.replace(/\\s+/g," ").trim() ?? null');
    console.log('volume warning:', warn);
    await shot('confirm');
  }
  if (STOP === 'type' || STOP === 'done') {
    await wd.exec('document.querySelector("[data-testid=device-continue]").click()');
    const name = await until('type field', 'const l=document.querySelector("label[for=dev-typed]"); return l && document.querySelector("[data-testid=device-wizard]").dataset.confirmName');
    const field = await wd.find('[data-testid=device-typed]');
    await wd.sendKeys(field, name);
    await wd.exec('const c=document.querySelector("[data-testid=device-volumes-ack]"); if (c && !c.checked) c.click()');
    await shot('type');
  }
  if (STOP === 'done') {
    const safe = trace().includes('[device] backend: dry-run') || (!!process.env.NICEVAULT_FAKE_DISKS && trace().includes('[device] backend: fake'));
    if (!safe) throw new Error('the trace does not say dry-run / fake: not pressing Cancella e crea');
    await timed('Cancella e crea -> progress', 'document.querySelector("[data-testid=device-erase]").click()', 'const s=document.querySelector("[data-testid=device-wizard]").dataset.stage; return s === "progress" || s === "done" || s === "error"');
    await shot('progress');
    const end = await until('done', 'const s=document.querySelector("[data-testid=device-wizard]").dataset.stage; return (s === "done" || s === "error") && s', 300000);
    times.push({ step: 'job', end });
    await shot(end);
  }
  console.log(JSON.stringify(times));
  console.log('--- app trace');
  console.log(trace().split('\n').filter((l) => l.includes('[device]')).join('\n'));
} catch (e) {
  console.log('FAIL', e.message);
  console.log('state:', JSON.stringify(await wd.exec('const w=document.querySelector("[data-testid=device-wizard]"); return w && { stage: w.dataset.stage, body: w.querySelector(".body")?.innerText.slice(0, 600), err: document.querySelector("[data-testid=device-form-error]")?.textContent, scrollTop: w.scrollTop, scrollH: w.scrollHeight, clientH: w.clientHeight }').catch((x) => x.message)));
  await shot('fail').catch(() => {});
  console.log(trace().split('\n').filter((l) => l.includes('[device]')).join('\n'));
  process.exitCode = 1;
} finally {
  await wd.quit().catch(() => {});
}
