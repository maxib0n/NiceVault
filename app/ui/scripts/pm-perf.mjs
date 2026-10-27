import { chromium, webkit } from '@playwright/test';
import { writeFileSync } from 'node:fs';

export const pmProbeSource = String.raw`
window.__pmProbe = {
  frames(ms) {
    return new Promise((resolve) => {
      const out = [];
      let last = performance.now();
      const end = last + ms;
      const tick = (t) => {
        out.push(t - last);
        last = t;
        if (t < end) requestAnimationFrame(tick);
        else resolve(out);
      };
      requestAnimationFrame(tick);
    });
  },
  /** frames with nothing happening: the platform's own jitter (the floor for the numbers below) */
  async idle(ms = 2000) {
    return this.summary(await this.frames(ms));
  },
  async longTasks(fn) {
    const lt = [];
    let obs = null;
    try {
      obs = new PerformanceObserver((l) => l.getEntries().forEach((e) => lt.push(e.duration)));
      obs.observe({ type: 'longtask', buffered: false });
    } catch {}
    const r = await fn();
    await new Promise((r2) => setTimeout(r2, 50));
    obs?.disconnect();
    return { r, lt };
  },
  summary(f) {
    const s = [...f].slice(1);
    const total = s.reduce((a, b) => a + b, 0);
    return {
      frames: s.length,
      avg_fps: +(s.length / (total / 1000)).toFixed(1),
      worst_ms: +Math.max(0, ...s).toFixed(1),
      over16: s.filter((x) => x > 16.7 + 1).length,
      p95_ms: +(s.sort((a, b) => a - b)[Math.floor(s.length * 0.95)] ?? 0).toFixed(1),
    };
  },
  /** pxPerSec = 0: stress (the whole list end to end every 1.5 s); else a steady fling */
  async scroll(ms = 3000, pxPerSec = 0) {
    const list = document.querySelector('[data-testid=pm-list]');
    const sc = list.querySelector('[data-scroller]') ?? list;
    const max = sc.scrollHeight - sc.clientHeight;
    const t0 = performance.now();
    let stop = false;
    const step = () => {
      if (stop) return;
      const t = performance.now() - t0;
      if (pxPerSec) sc.scrollTop = Math.min(max, (t / 1000) * pxPerSec);
      else {
        const k = (t / 1500) % 2;
        sc.scrollTop = (k < 1 ? k : 2 - k) * max;
      }
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
    const { r, lt } = await this.longTasks(() => this.frames(ms));
    stop = true;
    sc.scrollTop = 0;
    return { ...this.summary(r), long_tasks: lt.length, scroll_px: max };
  },
  /** resolves at the first frame after the list's DOM changed (or 1 s) */
  nextListPaint(since) {
    const list = document.querySelector('[data-testid=pm-list]');
    return new Promise((resolve) => {
      let done = false;
      const fin = () => {
        if (done) return;
        done = true;
        mo.disconnect();
        requestAnimationFrame(() => setTimeout(() => resolve(performance.now() - since), 0));
      };
      const mo = new MutationObserver(fin);
      mo.observe(list, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['data-title', 'data-q', 'data-count'] });
      setTimeout(fin, 1000);
    });
  },
  async search(word) {
    const input = document.querySelector('[data-testid=pm-search]');
    input.focus();
    input.value = '';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise((r) => setTimeout(r, 300));
    const lat = [];
    const echo = [];
    const framesP = this.frames(word.length * 220 + 200);
    for (const ch of word) {
      const t = performance.now();
      const paint = this.nextListPaint(t);
      input.value += ch;
      input.dispatchEvent(new InputEvent('input', { bubbles: true, data: ch, inputType: 'insertText' }));
      // the typed character on screen: the next frame
      await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));
      echo.push(performance.now() - t);
      lat.push(await paint);
      await new Promise((r) => setTimeout(r, 200 - Math.min(200, performance.now() - t)));
    }
    const f = await framesP;
    input.value = '';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    const round = (v) => v.map((x) => +x.toFixed(1));
    return { keystroke_to_list_ms: round(lat), keystroke_echo_ms: round(echo), max_list_ms: +Math.max(...lat).toFixed(1), frames: this.summary(f) };
  },
  async select(n = 10) {
    const rows = [...document.querySelectorAll('[data-testid=pm-entry]')].slice(1, n + 1);
    const out = [];
    for (const row of rows) {
      const title = row.dataset.title;
      const t = performance.now();
      row.click();
      await new Promise((resolve) => {
        const check = () => {
          const d = document.querySelector('[data-testid=pm-detail]');
          if (d && d.textContent.includes(title)) requestAnimationFrame(() => setTimeout(resolve, 0));
          else requestAnimationFrame(check);
        };
        check();
      });
      out.push(+(performance.now() - t).toFixed(1));
    }
    return { click_to_detail_ms: out, max_ms: Math.max(...out) };
  },
};
`;

async function main() {
  const args = process.argv.slice(2);
  const base = args.find((a) => !a.startsWith('--')) ?? 'http://127.0.0.1:5295';
  const opt = (k, d) => args.find((a) => a.startsWith(`--${k}=`))?.split('=')[1] ?? d;
  const n = Number(opt('entries', '2000'));
  const which = opt('browser', 'chromium');
  const trace = opt('trace', null);
  const browser = await (which === 'webkit' ? webkit : chromium).launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, locale: 'it-IT' });
  await page.addInitScript(pmProbeSource);
  await page.goto(`${base}/?screen=vault&pm=${n}`);
  await page.locator('[data-ready="true"]').waitFor();
  if (opt('icons', 'on') === 'off') await page.evaluate(() => (window.__nv.mock.siteIcons = false));
  if (trace && which === 'chromium') await browser.startTracing(page, { path: trace, screenshots: false });
  const first = await page.evaluate(
    () =>
      new Promise((resolve) => {
        const t = performance.now();
        document.querySelector('[data-testid=nav-passwords]').click();
        const check = () => {
          if (document.querySelectorAll('[data-testid=pm-entry]').length > 0) requestAnimationFrame(() => setTimeout(() => resolve(performance.now() - t), 0));
          else requestAnimationFrame(check);
        };
        check();
      }),
  );
  await page.waitForTimeout(500);
  const rows = await page.locator('[data-testid=pm-entry]').count();
  const idle = await page.evaluate(() => window.__pmProbe.idle(2000));
  const scroll = await page.evaluate(() => window.__pmProbe.scroll(3000));
  await page.waitForTimeout(300);
  const fling = await page.evaluate(() => window.__pmProbe.scroll(3000, 2400));
  const search = await page.evaluate(() => window.__pmProbe.search('0015'));
  const select = await page.evaluate(() => window.__pmProbe.select(10));
  if (trace && which === 'chromium') await browser.stopTracing();
  const out = { browser: which, entries: n, first_render_ms: +first.toFixed(1), rows_in_dom: rows, idle, scroll, fling, search, select };
  console.log(JSON.stringify(out, null, 2));
  if (opt('out', null)) writeFileSync(opt('out'), JSON.stringify(out, null, 2));
  await browser.close();
}

if (import.meta.url === `file://${process.argv[1]}`) await main();
