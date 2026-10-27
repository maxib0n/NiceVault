import { createServer } from 'node:http';
import { mkdtempSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const EXT = join(HERE, '..', 'dist', 'chrome');
const { chromium } = createRequire(join(HERE, '../../app/ui/package.json'))('playwright');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const PAGE = `<!doctype html><meta charset=utf-8><title>login</title>
<form style="margin:80px"><input name=user autocomplete=username style="width:320px;height:32px"><br><br>
<input type=password name=pw style="width:320px;height:32px"></form>`;
const server = createServer((_, res) => res.writeHead(200, { 'content-type': 'text/html' }).end(PAGE));
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/`;

const home = mkdtempSync(join(process.env.TMPDIR ?? tmpdir(), 'nv-clickjack-'));
const fails = [];
const check = (ok, what) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) fails.push(what);
};
let ctx;
try {
  ctx = await chromium.launchPersistentContext(join(home, 'profile'), {
    headless: false,
    env: { ...process.env, HOME: home, XDG_CONFIG_HOME: join(home, '.config') },
    viewport: { width: 900, height: 600 },
    args: [`--disable-extensions-except=${EXT}`, `--load-extension=${EXT}`, '--no-first-run', '--no-default-browser-check'],
  });
  const page = ctx.pages()[0] ?? (await ctx.newPage());
  await page.goto(url);
  const pw = page.locator('input[type=password]');
  await pw.click();
  const box = await pw.boundingBox();
  await sleep(1500);
  await page.mouse.click(box.x + box.width - 6 - 11, box.y + box.height / 2);
  let menu = null;
  for (let i = 0; i < 50 && !menu; i++) {
    await sleep(100);
    menu = page.frames().find((f) => f.url().startsWith('chrome-extension://') && f.url().includes('menu.html'));
  }
  check(Boolean(menu), 'the inline menu opened');
  if (menu) {
    const visible = async () => menu.evaluate(() => document.documentElement.dataset.visible ?? 'unknown');
    const waitFor = async (want, ms = 3000) => {
      const t0 = Date.now();
      let v;
      while (Date.now() - t0 < ms) {
        v = await visible();
        if (v === want) return v;
        await sleep(100);
      }
      return v;
    };
    check((await waitFor('1')) === '1', 'uncovered: the browser judges the menu visible (clicks count)');
    await page.evaluate(() => {
      const d = document.createElement('div');
      d.id = 'cover';
      d.style.cssText = 'position:fixed;inset:0;z-index:2147483647;pointer-events:none;background:rgba(255,255,255,.02)';
      document.documentElement.append(d);
    });
    check((await waitFor('0')) === '0', 'a transparent cover with pointer-events:none: not visible (clicks ignored)');
    await page.evaluate(() => document.getElementById('cover').remove());
    check((await waitFor('1')) === '1', 'cover removed: visible again');
    await page.evaluate(() => {
      const d = document.createElement('div');
      d.id = 'pop';
      d.popover = 'manual';
      d.style.cssText = 'inset:0;width:100vw;height:100vh;margin:0;border:0;pointer-events:none;background:rgba(255,255,255,.02)';
      document.body.append(d);
      d.showPopover();
    });
    check((await waitFor('0')) === '0', 'a top-layer popover over it: not visible');
    await page.evaluate(() => document.getElementById('pop').remove());
    await waitFor('1');
    await page.evaluate(() => document.querySelector('nicevault-ui').setAttribute('style', 'position:fixed;transform:scale(.05)'));
    await sleep(300);
    const still = page.frames().some((f) => f.url().includes('menu.html') && !f.isDetached());
    check(!still, 'the page rewrote the host style: the menu closed');
    const restored = await page.evaluate(() => document.querySelector('nicevault-ui').getAttribute('style'));
    check(restored.includes('all: initial'), 'and the host style came back');
  }
} finally {
  await ctx?.close().catch(() => {});
  server.close();
  rmSync(home, { recursive: true, force: true });
}
console.log(fails.length ? `${fails.length} failed` : 'all passed');
process.exit(fails.length ? 1 : 0);
