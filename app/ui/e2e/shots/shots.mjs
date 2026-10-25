// README screenshots and GIFs: the real UI against the in-browser mock backend, English, with the
// demo content of src/lib/mockDemo.ts (?demo=1). Output goes to docs/media/ at the repo root.
//
//   pnpm shots                 everything
//   pnpm shots welcome gif     only the shots / GIFs whose name matches one of the words (regexes)
//
// Needs Chromium from Playwright (`pnpm exec playwright install chromium`) and ffmpeg on PATH.
// It starts its own Vite dev server on NV_SHOTS_PORT (default 5291), or uses BASE=http://host:port.
// Still images are WebP; GIFs are recorded with Playwright's video and converted with ffmpeg
// (palettegen/paletteuse), each kept under 4 MB.

import { chromium } from 'playwright';
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const UI = resolve(HERE, '../..');
const OUT = resolve(UI, '../../docs/media');
const WORK = mkdtempSync(join(process.env.TMPDIR ?? tmpdir(), 'nv-shots-'));
const PORT = Number(process.env.NV_SHOTS_PORT ?? 5291);
const BASE = process.env.BASE ?? `http://127.0.0.1:${PORT}`;
const only = process.argv.slice(2).map((w) => new RegExp(w, 'i'));
const wanted = (name) => only.length === 0 || only.some((r) => r.test(name));
mkdirSync(OUT, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// --- dev server ---------------------------------------------------------------------------------

let vite = null;
async function startServer() {
  if (process.env.BASE) return;
  // node + vite's own script, not `pnpm exec`: one process, so stopping it leaves nothing behind
  vite = spawn(process.execPath, [join(UI, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', String(PORT), '--strictPort'], { cwd: UI, stdio: 'ignore' });
  for (let i = 0; i < 120; i++) {
    try {
      if ((await fetch(BASE)).ok) return;
    } catch {}
    await sleep(500);
  }
  throw new Error('the Vite dev server did not come up');
}
function stopServer() {
  if (vite) vite.kill('SIGTERM');
}

// --- browser ------------------------------------------------------------------------------------

function chromePath() {
  const root = join(homedir(), '.cache/ms-playwright');
  if (!existsSync(root)) return undefined;
  for (const d of readdirSync(root).filter((x) => /^chromium-\d+$/.test(x)).sort().reverse()) {
    const p = join(root, d, 'chrome-linux64', 'chrome');
    if (existsSync(p)) return p;
  }
  return undefined;
}
async function launch() {
  try {
    return await chromium.launch();
  } catch {
    return await chromium.launch({ executablePath: chromePath() });
  }
}

const DESKTOP = { width: 1280, height: 800 };
const PHONE = { width: 390, height: 844 };


// GIFs only: the leather and felt grain is per-pixel noise that GIF cannot compress (10 MB for 15 s),
// so its strength is turned right down while recording. Stills keep the full texture.
const CALM = `*{--board-tex-alpha:.05!important;--sheet-tex-alpha:.05!important;--cloth-tex-alpha:.1!important;--tex-k:.1!important}`;

// A visible pointer for the GIFs: Playwright's video does not draw one.
const CURSOR = () => {
  const mk = () => {
    if (document.getElementById('__cur')) return;
    const c = document.createElement('div');
    c.id = '__cur';
    c.style.cssText =
      'position:fixed;z-index:2147483647;pointer-events:none;width:24px;height:24px;border-radius:50%;left:-60px;top:-60px;' +
      'transform:translate(-50%,-50%);background:rgba(255,255,255,.22);border:2px solid rgba(255,255,255,.95);' +
      'box-shadow:0 0 0 1px rgba(0,0,0,.45),0 2px 8px rgba(0,0,0,.5);transition:width .09s,height .09s,background .09s';
    document.body.appendChild(c);
    addEventListener('mousemove', (e) => { c.style.left = e.clientX + 'px'; c.style.top = e.clientY + 'px'; }, true);
    addEventListener('mousedown', () => { c.style.width = c.style.height = '15px'; c.style.background = 'rgba(255,255,255,.85)'; }, true);
    addEventListener('mouseup', () => { c.style.width = c.style.height = '24px'; c.style.background = 'rgba(255,255,255,.22)'; }, true);
  };
  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', mk);
  else mk();
};

// no ?lang=: the context's locale (en-US) picks English, as it would on a real machine
const URL_BASE = 'demo=1';
const url = (q) => `${BASE}/?${URL_BASE}&${q}`;

async function newPage(browser, { size = DESKTOP, dpr = 1, mobile = false, record = null, motion = false } = {}) {
  const ctx = await browser.newContext({
    viewport: size,
    deviceScaleFactor: dpr,
    hasTouch: mobile,
    isMobile: mobile,
    locale: 'en-US',
    timezoneId: 'Europe/Rome',
    reducedMotion: motion ? 'no-preference' : 'reduce',
    ...(record ? { recordVideo: { dir: record, size } } : {}),
  });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && !/favicon/.test(m.text()) && errors.push(m.text()));
  if (record) {
    await page.addInitScript(CURSOR);
    await page.addInitScript(`document.addEventListener('DOMContentLoaded',()=>{const s=document.createElement('style');s.textContent=${JSON.stringify(CALM)};document.head.appendChild(s)})`);
  }
  return { ctx, page, errors };
}

async function boot(page, query) {
  await page.goto(url(query));
  await page.waitForSelector('[data-ready="true"]', { timeout: 15000 });
}

async function checkNoServer(page, name) {
  const text = await page.evaluate(() => document.body.innerText);
  void text;
}

// --- stills -------------------------------------------------------------------------------------

const toWebp = (png, webp) => {
  const r = spawnSync('ffmpeg', ['-v', 'error', '-y', '-i', png, '-c:v', 'libwebp', '-quality', '90', '-compression_level', '6', webp]);
  if (r.status !== 0) throw new Error(`ffmpeg: ${r.stderr}`);
  rmSync(png);
};

async function still(browser, name, query, opt = {}) {
  if (!wanted(name)) return;
  const { ctx, page, errors } = await newPage(browser, opt);
  await boot(page, query);
  if (opt.pre) await opt.pre(page);
  await page.waitForTimeout(opt.wait ?? 700);
  await checkNoServer(page, name);
  const png = join(WORK, `${name}.png`);
  await page.screenshot({ path: png });
  toWebp(png, join(OUT, `${name}.webp`));
  await ctx.close();
  if (errors.length) throw new Error(`${name}: ${errors.join(' | ')}`);
  console.log(`ok   ${name}.webp  ${(statSync(join(OUT, `${name}.webp`)).size / 1024).toFixed(0)} KB`);
}

const bigTiles = (page) => page.locator('[data-testid=view-size] [role=radio]').nth(2).click();
const PW = 'input[autocomplete="current-password"], input[type="password"]';

async function stills(browser) {
  // desktop, dark (the default look)
  await still(browser, 'welcome-dark', 'screen=welcome');
  await still(browser, 'unlock-dark', 'screen=unlock');
  await still(browser, 'files-grid-dark', 'screen=vault&view=grid&dir=Photos/Alps 2025', { pre: bigTiles });
  await still(browser, 'files-folders-dark', 'screen=vault&view=grid');
  await still(browser, 'files-list-dark', 'screen=vault&view=list&dir=Work');
  await still(browser, 'preview-image-dark', 'screen=vault&dir=Photos/Alps 2025&preview=Alpine lake.jpg', { wait: 1500 });
  await still(browser, 'preview-video-dark', 'screen=vault&dir=Video&preview=Alps timelapse.mp4', { wait: 2500 });
  await still(browser, 'search-dark', 'screen=vault&view=grid&q=lake&filters=kinds:photos', { wait: 1200 });
  await still(browser, 'passwords-dark', 'screen=passwords&entry=Harbor Credit Union');
  await still(browser, 'details-dark', 'screen=details&fixed=1', { pre: (page) => page.locator('[data-testid=slots]').first().evaluate((el) => el.scrollIntoView({ block: 'start' })) });
  await still(browser, 'settings-dark', 'screen=settings');
  await still(browser, 'device-drive-dark', 'screen=device&device_step=disk');
  await still(browser, 'device-confirm-dark', 'screen=device&device_step=confirm', { wait: 1200 });
  await still(browser, 'device-done-dark', 'screen=device&device_step=done', { wait: 1500 });
  // desktop, light
  const L = '&theme=light&app_theme=light';
  await still(browser, 'welcome-light', `screen=welcome${L}`);
  await still(browser, 'unlock-light', `screen=unlock${L}`);
  await still(browser, 'files-grid-light', `screen=vault&view=grid&dir=Photos/Alps 2025${L}`, { pre: bigTiles });
  await still(browser, 'files-list-light', `screen=vault&view=list&dir=Work${L}`);
  await still(browser, 'passwords-light', `screen=passwords&entry=Harbor Credit Union${L}`);
  // phone
  const P = { size: PHONE, dpr: 2, mobile: true };
  await still(browser, 'phone-unlock', 'screen=unlock&platform=mobile', P);
  await still(browser, 'phone-files-grid', 'screen=vault&view=grid&platform=mobile', P);
  await still(browser, 'phone-files-list', 'screen=vault&view=list&dir=Work&platform=mobile', P);
  await still(browser, 'phone-preview', 'screen=vault&dir=Photos/Alps 2025&preview=Lake reflection.jpg&platform=mobile', { ...P, wait: 1500 });
  await still(browser, 'phone-unlock-light', `screen=unlock&platform=mobile${L}`, P);
  await still(browser, 'phone-files-grid-light', `screen=vault&view=grid&platform=mobile${L}`, P);
}

// The README header: the app icon (docs/media/icon.png, from app/src-tauri/icons) and the title, set in the app's own fonts.
async function header(browser) {
  if (!wanted('header')) return;
  const { ctx, page, errors } = await newPage(browser, { size: { width: 1600, height: 420 } });
  await page.goto(`${BASE}/e2e/shots/header.html`);
  const icon = readFileSync(join(OUT, 'icon.png')).toString('base64');
  await page.evaluate((src) => { document.getElementById('icon').src = src; }, `data:image/png;base64,${icon}`);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
  const png = join(WORK, 'header.png');
  await page.screenshot({ path: png });
  toWebp(png, join(OUT, 'header.webp'));
  await ctx.close();
  if (errors.length) throw new Error(`header: ${errors.join(' | ')}`);
  console.log(`ok   header.webp  ${(statSync(join(OUT, 'header.webp')).size / 1024).toFixed(0)} KB`);
}

// --- GIFs ---------------------------------------------------------------------------------------

const MAX_GIF = 3.8 * 1024 * 1024;

function toGif(video, gif, { from = 0, width = 960, fps = 12 } = {}) {
  // Playwright's video carries compression noise and the UI animates a lot, so a plain conversion is
  // 7+ MB: denoise first (hqdn3d), then back off size, frame rate and palette until it fits.
  const bayer = 'bayer:bayer_scale=5';
  const ladder = [
    [width, fps, 256, bayer],
    [Math.round(width * 0.92), fps, 256, bayer],
    [Math.round(width * 0.84), 10, 256, bayer],
    [Math.round(width * 0.76), 10, 256, bayer],
    [Math.round(width * 0.68), 10, 256, bayer],
    [Math.round(width * 0.76), 10, 128, 'none'],
    [Math.round(width * 0.66), 10, 96, 'none'],
  ];
  for (const [w, f, colors, dither] of ladder) {
    const pal = join(WORK, 'pal.png');
    const vf = `fps=${f},hqdn3d=6:5:10:8,scale=${w}:-1:flags=lanczos`;
    let r = spawnSync('ffmpeg', ['-v', 'error', '-y', '-ss', String(from), '-i', video, '-vf', `${vf},palettegen=max_colors=${colors}:stats_mode=diff`, pal]);
    if (r.status !== 0) throw new Error(`palettegen: ${r.stderr}`);
    r = spawnSync('ffmpeg', ['-v', 'error', '-y', '-ss', String(from), '-i', video, '-i', pal, '-lavfi', `${vf}[x];[x][1:v]paletteuse=dither=${dither}:diff_mode=rectangle`, '-loop', '0', gif]);
    if (r.status !== 0) throw new Error(`paletteuse: ${r.stderr}`);
    const size = statSync(gif).size;
    console.log(`     ${w}px ${f}fps ${colors} colours ${dither.split(':')[0]}: ${(size / 1024 / 1024).toFixed(2)} MB`);
    if (size < MAX_GIF) return;
  }
  throw new Error(`${gif} is still over 4 MB`);
}

/** Smooth pointer travel to an element's centre (the CURSOR overlay shows it). */
async function point(page, locator, { steps = 28, dx = 0, dy = 0 } = {}) {
  await locator.scrollIntoViewIfNeeded();
  const b = await locator.boundingBox();
  await page.mouse.move(b.x + b.width / 2 + dx, b.y + b.height / 2 + dy, { steps });
}
async function click(page, locator, opt) {
  await point(page, locator, opt);
  await page.waitForTimeout(120);
  await page.mouse.down();
  await page.waitForTimeout(70);
  await page.mouse.up();
}
async function dblclick(page, locator, opt) {
  await point(page, locator, opt);
  await page.waitForTimeout(120);
  const b = await locator.boundingBox();
  await page.mouse.dblclick(b.x + b.width / 2, b.y + b.height / 2);
}

/** The GIF starts here (after the page settled), not at the blank page the video opens with. */
const startClip = (page) => { page.__clipAt = Date.now(); };

async function gif(browser, name, run, opt = {}) {
  if (!wanted(name)) return;
  const dir = join(WORK, name);
  mkdirSync(dir, { recursive: true });
  const { ctx, page, errors } = await newPage(browser, { ...opt, record: dir, motion: true });
  const t0 = Date.now();
  await run(page);
  const from = Math.max(0, ((page.__clipAt ?? t0) - t0) / 1000);
  const video = await page.video().path();
  await ctx.close();
  if (process.env.NV_SHOTS_KEEP) copyFileSync(video, join(process.env.NV_SHOTS_KEEP, `${name}.webm`));
  if (errors.length) throw new Error(`${name}: ${errors.join(' | ')}`);
  const target = join(OUT, `${name}.gif`);
  toGif(video, target, { from, width: opt.gifWidth ?? 960 });
  console.log(`ok   ${name}.gif  ${(statSync(target).size / 1024 / 1024).toFixed(2)} MB  (${((Date.now() - t0) / 1000).toFixed(0)} s recorded)`);
}

const item = (page, name) => page.getByRole('option').filter({ has: page.getByTestId('item-name').and(page.getByText(name, { exact: true })) });

async function gifs(browser) {
  // 1. unlock: the shelf, a volume, the password, the files
  await gif(browser, 'unlock', async (page) => {
    await boot(page, 'screen=welcome');
    await page.waitForTimeout(1500);
    startClip(page);
    await page.waitForTimeout(500);
    await click(page, page.getByRole('button', { name: /Documents/ }).first());
    await page.waitForSelector(PW);
    await page.waitForTimeout(1300);
    await page.locator(PW).first().pressSequentially('demo', { delay: 200 });
    await page.waitForTimeout(400);
    await page.keyboard.press('Enter');
    await page.waitForSelector('[data-screen="vault"]', { timeout: 15000 });
    await page.waitForTimeout(2000);
  });

  // 2. browse, open a photo, step through, full screen
  await gif(browser, 'browse-preview', async (page) => {
    await boot(page, 'screen=vault&view=grid');
    await page.waitForTimeout(2200);
    startClip(page);
    await page.waitForTimeout(600);
    await dblclick(page, item(page, 'Photos'));
    await page.waitForTimeout(1000);
    await dblclick(page, item(page, 'Alps 2025'));
    await page.waitForTimeout(1300);
    await dblclick(page, item(page, 'Alpine lake.jpg'));
    await page.waitForTimeout(1500);
    for (let i = 0; i < 2; i++) {
      await page.keyboard.press('ArrowRight');
      await page.waitForTimeout(1100);
    }
    await click(page, page.getByTestId('ql-fullscreen'));
    await page.waitForTimeout(1800);
  });

  // 3. make a NiceVault device from a USB stick (dry run in the mock: nothing is erased anywhere)
  await gif(browser, 'create-device', async (page) => {
    await boot(page, 'screen=welcome');
    await page.waitForTimeout(1500);
    startClip(page);
    await page.waitForTimeout(400);
    await click(page, page.getByTestId('open-device'));
    await page.getByTestId('device-disk').first().waitFor();
    await page.waitForTimeout(1200);
    await click(page, page.getByTestId('device-disk').first());
    await page.waitForTimeout(500);
    await click(page, page.getByTestId('device-next'));
    await page.waitForTimeout(1700);
    await click(page, page.getByTestId('device-next'));
    await page.waitForTimeout(1900);
    await click(page, page.getByTestId('device-continue'));
    await page.waitForTimeout(900);
    await click(page, page.getByTestId('device-typed'));
    await page.getByTestId('device-typed').pressSequentially('SanDisk Ultra', { delay: 90 });
    await page.waitForTimeout(500);
    await click(page, page.getByTestId('device-erase'));
    await page.getByTestId('device-done').waitFor({ timeout: 30000 });
    await page.waitForTimeout(2000);
  });

  // 4. phone: unlock, folders, a photo, swipe between photos
  await gif(browser, 'phone-swipe', async (page) => {
    const cdp = await page.context().newCDPSession(page);
    const touch = async (type, x, y) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: type === 'touchEnd' ? [] : [{ x, y }] });
    const tap = async (loc) => {
      const b = await loc.boundingBox();
      await touch('touchStart', b.x + b.width / 2, b.y + b.height / 2);
      await sleep(70);
      await touch('touchEnd');
    };
    const swipe = async (x0, x1, y) => {
      await touch('touchStart', x0, y);
      for (let i = 1; i <= 12; i++) {
        await touch('touchMove', x0 + ((x1 - x0) * i) / 12, y);
        await sleep(16);
      }
      await touch('touchEnd');
    };
    await boot(page, 'screen=unlock&platform=mobile');
    await page.waitForSelector(PW);
    await page.waitForTimeout(1500);
    startClip(page);
    await page.waitForTimeout(500);
    await page.locator(PW).first().pressSequentially('demo', { delay: 180 });
    await page.keyboard.press('Enter');
    await page.waitForSelector('[data-screen="vault"]', { timeout: 15000 });
    await page.waitForTimeout(1500);
    await tap(item(page, 'Photos'));
    await page.waitForTimeout(1100);
    await tap(item(page, 'Alps 2025'));
    await page.waitForTimeout(1300);
    await tap(item(page, 'Alpine lake.jpg'));
    await page.waitForTimeout(1500);
    for (let i = 0; i < 2; i++) {
      await swipe(320, 70, 420);
      await page.waitForTimeout(1200);
    }
    await swipe(70, 320, 420);
    await page.waitForTimeout(1200);
  }, { size: PHONE, dpr: 1, mobile: true, gifWidth: 390 });
}

// ------------------------------------------------------------------------------------------------

let failed = false;
try {
  await startServer();
  const browser = await launch();
  try {
    await header(browser);
    await stills(browser);
    await gifs(browser);
  } finally {
    await browser.close();
  }
} catch (e) {
  failed = true;
  console.error(String(e?.stack ?? e));
} finally {
  stopServer();
  rmSync(WORK, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
