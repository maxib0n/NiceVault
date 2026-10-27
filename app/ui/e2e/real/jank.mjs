import { execFileSync, spawn } from 'node:child_process';
import { loadavg } from 'node:os';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { KEY, WebDriver } from './webdriver.mjs';

const CHROMIUM = process.argv.includes('--chromium');
const HERE = new URL('.', import.meta.url).pathname;
const UI = join(HERE, '..', '..');
const OUT = process.env.NV_E2E_OUT ?? join(HERE, 'out');
const LABEL = process.env.NV_JANK_LABEL ?? (CHROMIUM ? 'chromium' : 'real');
const TOGGLES = (process.env.NV_JANK_TOGGLES ?? '').split(',').filter(Boolean);
const ONLY = (process.env.NV_JANK_ONLY ?? '').split(',').filter(Boolean);
const TRACE = CHROMIUM && process.env.NV_JANK_TRACE !== '0';
const PORT = Number(process.env.NV_JANK_PORT ?? 5296);
const PASSWORD = CHROMIUM ? 'demo' : 'cavallo batteria graffetta corretta';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(OUT, { recursive: true });

const CSS_TOGGLES = {
  noblend: '*,*::before,*::after{mix-blend-mode:normal!important;background-blend-mode:normal!important}',
  nofilter: '*,*::before,*::after{filter:none!important;backdrop-filter:none!important}',
  noshadow: '*,*::before,*::after{box-shadow:none!important;text-shadow:none!important}',
  notex: '.sheet::before,.ground::before,.cloth::before{display:none!important}.foil{background-image:none!important;color:currentColor!important;-webkit-background-clip:border-box!important;background-clip:border-box!important}',
  nocanvas: 'canvas{display:none!important}',
  notransition: '*,*::before,*::after{transition:none!important;animation-duration:0s!important;animation-delay:0s!important}',
  cv: '[data-testid=pm-entry]{content-visibility:auto;contain-intrinsic-size:auto 50px}.file-item{content-visibility:auto;contain-intrinsic-size:auto 190px}[data-layout=list] .file-item{contain-intrinsic-size:auto 40px}',
};

const PROBE = (toggles, autostart) => `(() => {
  if (window.__jank) return 'again';
  const css = ${JSON.stringify(toggles.map((t) => CSS_TOGGLES[t]).filter(Boolean).join('\n'))};
  if (css) {
    const put = () => { const s = document.createElement('style'); s.id = 'jank-toggles'; s.textContent = css; (document.head || document.documentElement).appendChild(s); };
    if (document.documentElement) put(); else document.addEventListener('DOMContentLoaded', put);
  }
  if (${JSON.stringify(toggles.includes('noreveal'))}) {
    const mm = window.matchMedia.bind(window);
    const fake = (q) => ({ matches: true, media: q, onchange: null, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => false });
    window.matchMedia = (q) => (/prefers-reduced-motion:\\s*reduce/.test(q) ? fake(q) : mm(q));
  }
  if (${JSON.stringify(toggles.includes('fmtcache'))}) {
    // models a fix: one Intl formatter per locale + options instead of one per call (lib/format.ts)
    const nf = new Map(), df = new Map(), NF = Intl.NumberFormat, DF = Intl.DateTimeFormat;
    const key = (l, o) => String(l) + JSON.stringify(o || {});
    Number.prototype.toLocaleString = function (l, o) { const k = key(l, o); let f = nf.get(k); if (!f) nf.set(k, (f = new NF(l, o))); return f.format(this); };
    const C = function (l, o) { const k = key(l, o); let f = df.get(k); if (!f) df.set(k, (f = new DF(l, o))); return f; };
    C.prototype = DF.prototype; C.supportedLocalesOf = DF.supportedLocalesOf;
    Intl.DateTimeFormat = C;
  }
  // apicensus: time and count the DOM/canvas calls the decode and the views make (a forced layout or
  // style recalc shows up as time inside the call that forced it); adds overhead, use for attribution only
  const API = new Map();
  if (${JSON.stringify(toggles.includes('apicensus'))}) {
    const wrap = (obj, name, label) => {
      const f = obj[name];
      if (typeof f !== 'function') return;
      obj[name] = function (...a) { if (!window.__jank || !window.__jank.on) return f.apply(this, a); const t = performance.now(); try { return f.apply(this, a); } finally { const e = API.get(label) || [0, 0]; e[0]++; e[1] += performance.now() - t; API.set(label, e); } };
    };
    wrap(Element.prototype, 'getBoundingClientRect', 'Element.getBoundingClientRect');
    wrap(Range.prototype, 'getBoundingClientRect', 'Range.getBoundingClientRect');
    wrap(Element.prototype, 'setAttribute', 'Element.setAttribute');
    wrap(Element.prototype, 'removeAttribute', 'Element.removeAttribute');
    wrap(window, 'getComputedStyle', 'getComputedStyle');
    wrap(CanvasRenderingContext2D.prototype, 'fillText', 'ctx.fillText');
    wrap(CanvasRenderingContext2D.prototype, 'measureText', 'ctx.measureText');
    wrap(CanvasRenderingContext2D.prototype, 'drawImage', 'ctx.drawImage');
    wrap(CanvasRenderingContext2D.prototype, 'getImageData', 'ctx.getImageData');
    wrap(CanvasRenderingContext2D.prototype, 'clearRect', 'ctx.clearRect');
    wrap(HTMLCanvasElement.prototype, 'getContext', 'canvas.getContext');
    wrap(Node.prototype, 'appendChild', 'Node.appendChild');
    wrap(Node.prototype, 'insertBefore', 'Node.insertBefore');
    wrap(TreeWalker.prototype, 'nextNode', 'TreeWalker.nextNode');
    wrap(CSSStyleDeclaration.prototype, 'setProperty', 'style.setProperty');
    wrap(HTMLImageElement.prototype, 'decode', 'img.decode');
    wrap(Element.prototype, 'scrollIntoView', 'Element.scrollIntoView');
    wrap(HTMLElement.prototype, 'focus', 'HTMLElement.focus');
    wrap(CanvasRenderingContext2D.prototype, 'stroke', 'ctx.stroke');
    wrap(CanvasRenderingContext2D.prototype, 'fill', 'ctx.fill');
    wrap(HTMLDialogElement.prototype, 'showModal', 'dialog.showModal');
    wrap(Element.prototype, 'animate', 'Element.animate');
    for (const [proto, prop] of [[HTMLElement.prototype, 'offsetWidth'], [HTMLElement.prototype, 'offsetHeight'], [Element.prototype, 'clientWidth'], [Element.prototype, 'scrollHeight']]) {
      const d = Object.getOwnPropertyDescriptor(proto, prop);
      if (!d || !d.get) continue;
      const g = d.get, label = 'get ' + prop;
      Object.defineProperty(proto, prop, { ...d, get() { if (!window.__jank || !window.__jank.on) return g.call(this); const t = performance.now(); try { return g.call(this); } finally { const e = API.get(label) || [0, 0]; e[0]++; e[1] += performance.now() - t; API.set(label, e); } } });
    }
  }
  const J = (window.__jank = { census: false, cb: new Map(), samples: [], on: false, name: null, t0: 0, frames: [], js: new Map(), marks: [], long: [], loaf: [] });
  const orig = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => orig((t) => {
    const s = performance.now();
    try { cb(t); } finally { if (J.on) { J.js.set(t, (J.js.get(t) || 0) + performance.now() - s); J.cb.set(t, (J.cb.get(t) || 0) + 1); } }
  });
  // decode census (J.census on): every 4th frame, the glyph canvases alive and the elements the decode
  // holds back, split into on- and off-screen (a held element off screen is work nobody sees)
  const census = () => {
    const held = document.querySelectorAll('[data-ar-t],[data-ar-m]');
    let off = 0;
    for (const el of held) { const r = el.getBoundingClientRect(); if (r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth) off++; }
    J.samples.push([performance.now() - J.t0, document.querySelectorAll('.ar-overlay canvas').length, held.length, off, document.querySelectorAll('.ar-overlay').length]);
  };
  let last = 0;
  let nth = 0;
  // phases (always on, cheap): where a long frame's time went, in engines without long-task entries
  // (WebKit). Each rendering update is timed from its rAF to a message posted from it (delivered once the
  // update, style + layout + paint recording on the main thread, has finished): "render". A 4 ms timer
  // heartbeat finds the main thread blocked outside rendering (event handlers, promise continuations,
  // component mounts): "tasks". What is neither is the compositor / GPU (the main thread was free).
  const ch = new MessageChannel();
  let rs = 0;
  ch.port1.onmessage = () => { if (J.on && rs) J.ph.push([rs, performance.now() - rs]); rs = 0; };
  let hb = 0;
  const beat = () => { if (!J.on) { hb = 0; return; } const now = performance.now(); if (hb && now - hb > 9) J.gaps.push([hb, now - hb]); hb = now; setTimeout(beat, 0); };
  const loop = (t) => { if (!J.on) { last = 0; return; } rs = performance.now(); ch.port2.postMessage(0); if (last) J.frames.push([t, t - last]); last = t; if (J.census && nth++ % 4 === 0) census(); orig(loop); };
  const types = (window.PerformanceObserver && PerformanceObserver.supportedEntryTypes) || [];
  J.supported = types.slice();
  try { if (types.includes('longtask')) new PerformanceObserver((l) => { if (J.on) for (const e of l.getEntries()) J.long.push([e.startTime, e.duration]); }).observe({ type: 'longtask' }); } catch {}
  try { if (types.includes('long-animation-frame')) new PerformanceObserver((l) => { if (J.on) for (const e of l.getEntries()) J.loaf.push({ t: e.startTime, d: e.duration, block: e.blockingDuration, render: e.renderStart ? e.startTime + e.duration - e.renderStart : 0, style: e.styleAndLayoutStart ? e.startTime + e.duration - e.styleAndLayoutStart : 0, scripts: e.scripts.map((s) => ({ d: s.duration, inv: s.invoker, src: (s.sourceURL || '').split('/').pop() + ':' + s.sourceFunctionName, layout: s.forcedStyleAndLayoutDuration })) }); }).observe({ type: 'long-animation-frame' }); } catch {}
  J.start = (name) => { J.name = name; J.t0 = performance.now(); API.clear(); J.frames = []; J.js = new Map(); J.cb = new Map(); J.samples = []; J.marks = []; J.long = []; J.loaf = []; J.ph = []; J.gaps = []; J.on = true; last = 0; orig(loop); if (!hb) beat(); return J.t0; };
  J.mark = (m) => { J.marks.push([performance.now(), m]); };
  J.stop = () => {
    J.on = false;
    const t1 = performance.now();
    const rev = (window.__nvReveal || []).filter((e) => e.t >= J.t0 - 1 && e.t <= t1).map((e) => [e.t, e.kind + ':' + e.what + ':' + e.key]);
    return { name: J.name, t0: J.t0, t1, dpr: devicePixelRatio, w: innerWidth, h: innerHeight, frames: J.frames.map(([t, d]) => [Math.round((t - J.t0) * 10) / 10, Math.round(d * 10) / 10, Math.round((J.js.get(t) || 0) * 10) / 10]), marks: J.marks.map(([t, m]) => [Math.round(t - J.t0), m]).concat(rev.map(([t, m]) => [Math.round(t - J.t0), m])), long: J.long.map(([t, d]) => [Math.round(t - J.t0), Math.round(d)]), loaf: J.loaf.map((l) => ({ ...l, t: Math.round(l.t - J.t0) })), ph: J.ph.map(([t, d]) => [Math.round((t - J.t0) * 10) / 10, Math.round(d * 10) / 10]), gaps: J.gaps.map(([t, d]) => [Math.round((t - J.t0) * 10) / 10, Math.round(d * 10) / 10]), api: [...API.entries()].sort((a, b) => b[1][1] - a[1][1]).map(([k, [n, ms]]) => [k, n, Math.round(ms * 10) / 10]), cbMax: Math.max(0, ...J.cb.values()), cbMean: J.cb.size ? [...J.cb.values()].reduce((a, b) => a + b, 0) / J.cb.size : 0, samples: J.samples.map((x) => x.map((v) => Math.round(v))), dom: document.getElementsByTagName('*').length };
  };
  if (${JSON.stringify(autostart)}) J.start(${JSON.stringify(autostart)});
  window.__nvInvoke = (c, a) => (window.__TAURI_INTERNALS__ ? window.__TAURI_INTERNALS__.invoke(c, a) : window.__nv.mock.invoke(c, a));
  return 'ok';
})()`;

function phases(r, a, b) {
  const ph = (r.ph ?? []).filter(([t]) => t >= a - 1 && t < b - 1);
  const render = ph.reduce((s, [, d]) => s + d, 0);
  let blocked = 0;
  for (const [t, d] of r.gaps ?? []) blocked += Math.max(0, Math.min(t + d, b) - Math.max(t, a));
  return { render: Math.round(render), tasks: Math.max(0, Math.round(blocked - render)) };
}
function stats(r) {
  const d = r.frames.map((f) => f[1]).filter((x) => x > 0);
  const js = r.frames.map((f) => f[2]);
  const sorted = [...d].sort((a, b) => a - b);
  const q = (p) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] : 0);
  const over = (ms) => (d.length ? Math.round((1000 * d.filter((x) => x > ms).length) / d.length) / 10 : 0);
  const dur = r.frames.length ? r.frames.at(-1)[0] - r.frames[0][0] + d[0] : 0;
  const hist = { '<=17.5': 0, '17.5-25': 0, '25-33.4': 0, '33.4-50': 0, '50-100': 0, '>100': 0 };
  for (const x of d) hist[x <= 17.5 ? '<=17.5' : x <= 25 ? '17.5-25' : x <= 33.4 ? '25-33.4' : x <= 50 ? '33.4-50' : x <= 100 ? '50-100' : '>100']++;
  const worst = r.frames
    .map((f) => f)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5)
    .map(([t, dt, j]) => ({ at: Math.round(t), ms: dt, rafJs: j, ...phases(r, t - dt, t), during: [...new Set(r.marks.filter(([mt]) => mt <= t && mt >= t - dt - 400).map(([, m]) => m.replace(/:[0-9a-f]{8,}.*$/, ':…')))].slice(-3) }));
  const missed = d.reduce((a, x) => a + Math.max(0, Math.round(x / 16.67) - 1), 0);
  return {
    frames: d.length,
    drop: d.length ? Math.round((1000 * missed) / (d.length + missed)) / 10 : 0,
    over25: over(25),
    ms: Math.round(dur),
    fps: dur ? Math.round((d.length * 10000) / dur) / 10 : 0,
    p50: q(0.5),
    p95: q(0.95),
    p99: q(0.99),
    max: sorted.at(-1) ?? 0,
    over16: over(17.5),
    over33: over(33.4),
    jsMean: js.length ? Math.round((10 * js.reduce((a, b) => a + b, 0)) / js.length) / 10 : 0,
    jsMax: Math.max(0, ...js),
    longTasks: r.long.length,
    longMs: r.long.reduce((a, [, x]) => a + x, 0),
    hist,
    worst,
    dom: r.dom,
    rafCallbacksMax: r.cbMax,
    canvasesMax: Math.max(0, ...(r.samples ?? []).map((x) => x[1])),
    overlaysMax: Math.max(0, ...(r.samples ?? []).map((x) => x[4])),
    heldMax: Math.max(0, ...(r.samples ?? []).map((x) => x[2])),
    heldOffscreenMax: Math.max(0, ...(r.samples ?? []).map((x) => x[3])),
  };
}

let D;
const results = [];
let currentTrace = null;

async function realDriver() {
  const wd = new WebDriver(process.env.NV_E2E_DRIVER ?? 'http://127.0.0.1:4444');
  await wd.start({ 'tauri:options': { application: process.env.NV_E2E_APP } });
  const pointer = (actions) =>
    wd.cmd('POST', '/actions', { actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions }] }).then(() => wd.cmd('DELETE', '/actions'));
  const el = async (css) => ({ 'element-6066-11e4-a52e-4f735466cecf': await wd.find(css) });
  let wheelAt = null;
  return {
    exec: (body, ...args) => wd.exec(body, ...args),
    run: async (body, ...args) => {
      const r = await wd.execAsync(`const done = arguments[arguments.length - 1]; const args = Array.from(arguments).slice(0, -1); (async (...args) => { ${body} })(...args).then((v) => done({ ok: v }), (e) => done({ err: String(e && (e.message || e.code) || e) }));`, ...args);
      if (r && 'err' in r) throw new Error(r.err);
      return r?.ok;
    },
    hover: async (css) => pointer([{ type: 'pointerMove', origin: await el(css), x: 0, y: 0, duration: 120 }]),
    moveTo: (x, y) => pointer([{ type: 'pointerMove', origin: 'viewport', x, y, duration: 120 }]),
    click: async (css) => wd.click(await wd.find(css)),
    dblElement: async (ref) => pointer([{ type: 'pointerMove', origin: ref, x: 0, y: 0 }, { type: 'pointerDown', button: 0 }, { type: 'pointerUp', button: 0 }, { type: 'pointerDown', button: 0 }, { type: 'pointerUp', button: 0 }]),
    rightElement: async (ref) => pointer([{ type: 'pointerMove', origin: ref, x: 0, y: 0 }, { type: 'pointerDown', button: 2 }, { type: 'pointerUp', button: 2 }]),
    typeInto: async (css, text, delay) => {
      const e = await wd.find(css);
      for (const ch of text) {
        await wd.sendKeys(e, ch);
        await sleep(delay);
      }
    },
    key: (k) => wd.keys(KEY[k] ?? k),
    wheel: async (css, dy) => {
      if (wheelAt?.css !== css) wheelAt = { css, origin: await el(css) };
      const origin = wheelAt.origin;
      await wd.cmd('POST', '/actions', { actions: [{ type: 'wheel', id: 'wheel', actions: [{ type: 'scroll', origin, x: 0, y: 0, deltaX: 0, deltaY: dy, duration: 0 }] }] });
      await wd.cmd('DELETE', '/actions');
    },
    reload: async () => {
      await wd.exec('location.reload()');
      for (let i = 0; i < 400; i++) {
        try {
          const r = await wd.exec(`return document.readyState !== 'loading' || !!document.documentElement ? ${PROBE(TOGGLES, 'boot')} : null`);
          if (r === 'ok') return;
          if (r === 'again') await sleep(5);
        } catch {
          await sleep(5);
        }
      }
      throw new Error('probe not injected after reload');
    },
    quit: () => wd.quit(),
    wd,
  };
}

async function chromiumDriver() {
  const { chromium } = await import('playwright');
  const up = async () => fetch(`http://127.0.0.1:${PORT}/`).then((r) => r.ok, () => false);
  if (await up()) throw new Error(`port ${PORT} is busy (someone else's server?): set NV_JANK_PORT`);
  const dist = join(process.env.TMPDIR ?? '/tmp', `nv-jank-dist-${PORT}`);
  execFileSync('pnpm', ['exec', 'vite', 'build', '--minify', 'false', '--outDir', dist, '--emptyOutDir'], { cwd: UI, stdio: 'ignore', env: { ...process.env, NV_KEEP_MOCK: '1' } });
  const vite = spawn('pnpm', ['exec', 'vite', 'preview', '--outDir', dist, '--host', '127.0.0.1', '--port', String(PORT), '--strictPort'], { cwd: UI, stdio: 'ignore', detached: true });
  for (let i = 0; i < 200 && !(await up()); i++) await sleep(100);
  const browser = await chromium.launch({ args: ['--disable-renderer-backgrounding', '--disable-background-timer-throttling'] });
  const [vw, vh] = (process.env.NV_JANK_WINDOW ?? '1200x780').split('x').map(Number);
  const ctx = await browser.newContext({ viewport: { width: vw, height: vh }, deviceScaleFactor: Number(process.env.NV_JANK_DPR ?? 1), locale: 'it-IT' });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  await cdp.send('Profiler.enable');
  await cdp.send('Profiler.setSamplingInterval', { interval: 200 });
  await page.addInitScript({ content: PROBE(TOGGLES, 'boot') });
  const loc = (css) => page.locator(css).first();
  const handle = async (ref) => ref;
  D = {
    page,
    exec: (body, ...args) => page.evaluate(([b, a]) => new Function(b)(...a), [body, args]),
    run: (body, ...args) => page.evaluate(([b, a]) => new Function('...args', `return (async (...args) => { ${b} })(...args)`)(...a), [body, args]),
    hover: (css) => loc(css).hover(),
    moveTo: (x, y) => page.mouse.move(x, y, { steps: 6 }),
    click: (css) => loc(css).click(),
    dblElement: async (ref) => (await handle(ref)).dblclick(),
    rightElement: async (ref) => (await handle(ref)).click({ button: 'right' }),
    typeInto: (css, text, delay) => loc(css).pressSequentially(text, { delay }),
    key: (k) => page.keyboard.press(k),
    wheel: async (css, dy) => {
      if (D.wheelAt?.css !== css) {
        const b = await loc(css).boundingBox();
        D.wheelAt = { css, x: b.x + b.width / 2, y: b.y + Math.min(b.height / 2, 200) };
        await page.mouse.move(D.wheelAt.x, D.wheelAt.y);
      }
      await page.mouse.wheel(0, dy);
    },
    startTrace: async (name) => {
      if (!TRACE) return;
      await browser.startTracing(page, {
        categories: ['devtools.timeline', 'disabled-by-default-devtools.timeline', 'disabled-by-default-devtools.timeline.frame', 'blink', 'cc', 'gpu', 'viz', 'v8', 'disabled-by-default-v8.gc'],
      });
      await cdp.send('Profiler.start');
      currentTrace = name;
    },
    stopTrace: async () => {
      if (!TRACE || !currentTrace) return null;
      const { profile } = await cdp.send('Profiler.stop');
      const buf = await browser.stopTracing();
      currentTrace = null;
      return { trace: analyseTrace(JSON.parse(buf.toString())), profile: analyseProfile(profile) };
    },
    quit: async () => {
      await browser.close();
      process.kill(-vite.pid, 'SIGTERM');
      rmSync(dist, { recursive: true, force: true });
    },
  };
  await page.goto(`http://127.0.0.1:${PORT}/`);
  return D;
}

const GROUP = [
  [/^CpuProfiler|^Profiler|^TracingStartedInBrowser/, 'profiler (measurement artefact)'],
  [/GC|^BlinkGC|garbage/i, 'gc'],
  [/^(FunctionCall|EvaluateScript|TimerFire|FireAnimationFrame|EventDispatch|RunMicrotasks|v8\.|V8\.|FireIdleCallback|ParseHTML|ResourceReceivedData|XHR|HandlePostMessage|ProfileCall|SerializedScriptValue|ScheduledAction|EventHandler|ScriptedAnimationController)/, 'script'],
  [/^(UpdateLayoutTree|RecalculateStyles|ScheduleStyleRecalculation|StyleRecalc|Document::recalcStyle|Document::updateStyle|StyleEngine|ParseAuthorStyleSheet|Document::UpdateStyleAndLayoutTree)/, 'style'],
  [/^(Layout$|LocalFrameView::performLayout|Document::rebuildLayoutTree|InlineNode::|LayoutShift|LayoutBlock|LayoutNG|ResizeObserver|IntersectionObserver|UpdateLayerTree)/, 'layout'],
  [/^LocalFrameView::RunStyleAndLayoutLifecyclePhases/, 'style+layout (lifecycle self)'],
  [/^(Canvas|CanvasRenderingContext2D|Canvas2DResourceProvider|OffscreenCanvas|CanvasResource)/, 'canvas'],
  [/^(Paint$|PrePaint|Blink\.PrePaint|Blink\.Paint|PaintArtifactCompositor|LayerTreeHost::DoUpdateLayers|CullRectUpdate|Layerize|UpdateLayer|CompositeLayers|Commit|ProxyMain::BeginMainFrame::commit|PaintImage|PaintController|GraphicsLayer|LocalFrameView::RunPaintLifecyclePhase|PaintLayer)/, 'paint/commit'],
  [/^(Decode Image|ImageDecodeTask|Decode LazyPixelRef|Draw LazyPixelRef|ImageFrameGenerator|ImageDecoder|ImageResource)/, 'image decode'],
  [/^LayerTreeHost::WaitForCommitCompletion/, 'waiting for the compositor'],
  [/^(HitTest|ScrollLayer|ScrollUpdate|EventHandler::HitTest|LocalFrame::HitTest|InputHandlerProxy)/, 'hit test / scroll'],
  [/^(WidgetBase::WillBeginMainFrame|ProxyMain::BeginMainFrame|WebFrameWidgetImpl::BeginMainFrame|PageAnimator)/, 'begin frame'],
  [/^(RunTask|ThreadControllerImpl::RunTask|ThreadPool_RunTask|SequenceManager|ThreadController)/, 'task overhead'],
];
function analyseTrace(json) {
  const evs = json.traceEvents ?? json;
  const names = new Map();
  for (const e of evs) if (e.ph === 'M' && e.name === 'thread_name') names.set(`${e.pid}:${e.tid}`, e.args.name);
  const count = new Map();
  for (const e of evs) if (names.get(`${e.pid}:${e.tid}`) === 'CrRendererMain' && e.ph === 'X') count.set(`${e.pid}:${e.tid}`, (count.get(`${e.pid}:${e.tid}`) ?? 0) + 1);
  const main = [...count.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  const mainEvs = evs.filter((e) => `${e.pid}:${e.tid}` === main && e.ph === 'X' && e.dur > 0).sort((a, b) => a.ts - b.ts || b.dur - a.dur);
  const self = new Map();
  const stack = [];
  for (const e of mainEvs) {
    while (stack.length && stack.at(-1).ts + stack.at(-1).dur <= e.ts) stack.pop();
    const parent = stack.at(-1);
    if (parent) self.set(parent.name, (self.get(parent.name) ?? 0) - Math.min(e.dur, parent.ts + parent.dur - e.ts));
    self.set(e.name, (self.get(e.name) ?? 0) + e.dur);
    stack.push(e);
  }
  const groups = {};
  const byName = [];
  for (const [n, us] of self) {
    const g = GROUP.find(([re]) => re.test(n))?.[1] ?? 'other';
    groups[g] = Math.round(((groups[g] ?? 0) + us / 1000) * 10) / 10;
    byName.push([n, Math.round(us / 100) / 10]);
  }
  const threads = {};
  for (const e of evs) {
    const tn = names.get(`${e.pid}:${e.tid}`);
    if (!tn || e.ph !== 'X' || !e.dur || !/Compositor|Raster|Viz|GPU|CrGpuMain|VizCompositor/.test(tn)) continue;
    if (!/RunTask|ThreadControllerImpl::RunTask|ThreadPool_RunTask/.test(e.name)) continue;
    threads[tn] = Math.round(((threads[tn] ?? 0) + e.dur / 1000) * 10) / 10;
  }
  return { main: groups, top: byName.sort((a, b) => b[1] - a[1]).slice(0, 14), threads };
}
function analyseProfile(p) {
  const self = new Map();
  const byId = new Map(p.nodes.map((n) => [n.id, n]));
  const dt = p.timeDeltas;
  for (let i = 0; i < p.samples.length; i++) {
    const n = byId.get(p.samples[i]);
    const cf = n.callFrame;
    const file = (cf.url || '').replace(/^.*\/src\//, 'src/').replace(/\?.*$/, '').replace(/^.*node_modules\/\.vite\/deps\//, 'deps/');
    const key = `${cf.functionName || '(anon)'} ${file ? `${file}:${cf.lineNumber + 1}` : ''}`.trim();
    self.set(key, (self.get(key) ?? 0) + (dt[i] ?? 0));
  }
  return [...self.entries()].filter(([k]) => k !== '(idle)' && k !== '(program)').sort((a, b) => b[1] - a[1]).slice(0, 18).map(([k, us]) => [k, Math.round(us / 100) / 10]);
}

const BENCH = `
  const dpr = args[0];
  const W = innerWidth, H = innerHeight;
  const raf = () => new Promise((r) => requestAnimationFrame(r));
  const med = (a) => a.slice().sort((x, y) => x - y)[a.length >> 1];
  const host = document.createElement('div');
  Object.assign(host.style, { position: 'fixed', left: '0', top: '0', width: W + 'px', height: H + 'px', pointerEvents: 'none', opacity: '0.01', zIndex: '9999' });
  document.body.appendChild(host);
  const mk = (w, h) => { const c = document.createElement('canvas'); c.width = Math.round(w * dpr); c.height = Math.round(h * dpr); c.style.width = w + 'px'; c.style.height = h + 'px'; return c; };
  const main = mk(W, H); host.appendChild(main);
  const ctx = main.getContext('2d');
  const ALPHA = ' .:-=+<#ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
  const N = 3000, fs = Math.round(13 * dpr);
  const pts = Array.from({ length: N }, (_, i) => [(i * 37) % (W * dpr), ((i * 53) % (H * dpr)), ALPHA[1 + (i * 7) % (ALPHA.length - 1)]]);
  const font = fs + 'px "JetBrains Mono Variable", monospace';
  const time = (fn) => { const out = []; for (let k = 0; k < 15; k++) { const t = performance.now(); fn(k); ctx.getImageData(0, 0, 1, 1); out.push(performance.now() - t); } return Math.round(med(out) * 100) / 100; };
  const r = {};
  r.fillTextPerGlyphColour = time((k) => { ctx.clearRect(0, 0, main.width, main.height); ctx.textAlign = 'center'; ctx.textBaseline = 'middle'; for (let i = 0; i < N; i++) { if (i % 12 === 0) ctx.font = font; ctx.fillStyle = 'rgba(' + ((i * 3 + k) & 255) + ',90,60,' + (0.5 + (i % 5) / 10) + ')'; ctx.fillText(pts[i][2], pts[i][0], pts[i][1]); } });
  r.fillTextOneColour = time(() => { ctx.clearRect(0, 0, main.width, main.height); ctx.font = font; ctx.fillStyle = '#b08040'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'; for (let i = 0; i < N; i++) ctx.fillText(pts[i][2], pts[i][0], pts[i][1]); });
  // atlas: every glyph rasterised once (white), tinted once with source-in, then blitted
  const cw = Math.ceil(fs * 0.7), chh = Math.ceil(fs * 1.3);
  const atlas = document.createElement('canvas'); atlas.width = cw * ALPHA.length; atlas.height = chh;
  const tAtlas0 = performance.now();
  const a = atlas.getContext('2d'); a.font = font; a.textAlign = 'center'; a.textBaseline = 'middle'; a.fillStyle = '#b08040';
  for (let j = 0; j < ALPHA.length; j++) a.fillText(ALPHA[j], j * cw + cw / 2, chh / 2);
  r.atlasBuildOnce = Math.round((performance.now() - tAtlas0) * 100) / 100;
  const idx = new Map([...ALPHA].map((c, j) => [c, j]));
  r.atlasDrawImage = time(() => { ctx.clearRect(0, 0, main.width, main.height); for (let i = 0; i < N; i++) { const j = idx.get(pts[i][2]); ctx.drawImage(atlas, j * cw, 0, cw, chh, pts[i][0] - cw / 2, pts[i][1] - chh / 2, cw, chh); } });
  r.atlasDrawImageAlpha = time(() => { ctx.clearRect(0, 0, main.width, main.height); for (let i = 0; i < N; i++) { const j = idx.get(pts[i][2]); ctx.globalAlpha = 0.5 + (i % 5) / 10; ctx.drawImage(atlas, j * cw, 0, cw, chh, pts[i][0] - cw / 2, pts[i][1] - chh / 2, cw, chh); } ctx.globalAlpha = 1; });
  // the same 3,000 glyphs as 300 strings of 10 (ascii-reveal: one string per text node), plain,
  // with a letter spacing set (and reset) per string, with a save/restore per string
  const strs = Array.from({ length: 300 }, (_, i) => Array.from({ length: 10 }, (_, j) => ALPHA[1 + ((i * 10 + j) * 7) % (ALPHA.length - 1)]).join(''));
  const S = (fn) => time(() => { ctx.clearRect(0, 0, main.width, main.height); ctx.font = font; ctx.fillStyle = '#b08040'; ctx.textAlign = 'left'; ctx.textBaseline = 'middle'; for (let i = 0; i < 300; i++) fn(i); });
  r.strings300 = S((i) => ctx.fillText(strs[i], pts[i][0], pts[i][1]));
  r.strings300Spaced = 'letterSpacing' in ctx ? S((i) => { ctx.letterSpacing = (i % 7) * 0.3 + 'px'; ctx.fillText(strs[i], pts[i][0], pts[i][1]); ctx.letterSpacing = '0px'; }) : null;
  r.strings300Stretched = S((i) => { ctx.setTransform(0.85 + (i % 7) * 0.05, 0, 0, 1, pts[i][0], 0); ctx.fillText(strs[i], 0, pts[i][1]); ctx.setTransform(1, 0, 0, 1, 0, 0); });
  r.strings300Translated = S((i) => { ctx.setTransform(1, 0, 0, 1, pts[i][0], 0); ctx.fillText(strs[i], 0, pts[i][1]); ctx.setTransform(1, 0, 0, 1, 0, 0); });
  r.glyphs3000Centered = S((i) => { ctx.textAlign = 'center'; for (let j = 0; j < 10; j++) ctx.fillText(strs[i][j], pts[i][0] + j * 8, pts[i][1]); });
  // as the engine sees them: 400 nodes, each its own pitch
  const str4 = Array.from({ length: 400 }, (_, i) => strs[i % 300]);
  const S4 = (fn) => time(() => { ctx.clearRect(0, 0, main.width, main.height); ctx.font = font; ctx.fillStyle = '#b08040'; ctx.textAlign = 'left'; ctx.textBaseline = 'middle'; for (let i = 0; i < 400; i++) fn(i); });
  const px = (i) => pts[i % N][0], py = (i) => pts[i % N][1];
  r.nodes400SpacedDistinct = 'letterSpacing' in ctx ? S4((i) => { ctx.letterSpacing = (-1.5 + i * 0.0071).toFixed(3) + 'px'; ctx.fillText(str4[i], px(i), py(i)); ctx.letterSpacing = '0px'; }) : null;
  const q = Array.from({ length: 400 }, (_, i) => Math.round((-1.5 + i * 0.0071) * 10) / 10).sort((a, b) => a - b);
  r.nodes400SpacedQuantisedSorted = 'letterSpacing' in ctx ? S4((i) => { const v = q[i] + 'px'; if (i === 0 || q[i] !== q[i - 1]) ctx.letterSpacing = v; ctx.fillText(str4[i], px(i), py(i)); }) : null;
  if ('letterSpacing' in ctx) ctx.letterSpacing = '0px';
  r.nodes400StretchDistinct = S4((i) => { ctx.setTransform(0.8 + i * 0.001, 0, 0, 1, px(i), 0); ctx.fillText(str4[i], 0, py(i)); ctx.setTransform(1, 0, 0, 1, 0, 0); });
  r.nodes400Glyphs = S4((i) => { ctx.textAlign = 'center'; for (let j = 0; j < 10; j++) ctx.fillText(str4[i][j], px(i) + j * 8, py(i)); });
  r.strings300SaveRestore = S((i) => { ctx.save(); ctx.font = font; ctx.fillStyle = '#b08040'; ctx.fillText(strs[i], pts[i][0], pts[i][1]); ctx.restore(); });
  r.measureText100 = time(() => { for (let i = 0; i < 100; i++) { ctx.font = (12 + (i % 3)) + 'px serif'; ctx.measureText('Hg' + i); } });
  host.removeChild(main);
  // many small canvases vs one: creation + first draw (sync), then the next two frames
  const frameAfter = async () => { await raf(); const t = performance.now(); await raf(); return Math.round((performance.now() - t) * 10) / 10; };
  const many = async (n, w, h) => {
    const t = performance.now();
    const cs = [];
    for (let i = 0; i < n; i++) { const c = mk(w, h); c.style.position = 'absolute'; c.style.left = (i * 29) % (W - w) + 'px'; c.style.top = (i * 17) % (H - h) + 'px'; host.appendChild(c); const x = c.getContext('2d'); x.fillStyle = '#b08040'; x.fillRect(0, 0, 8, 8); cs.push(c); }
    const sync = performance.now() - t;
    const f1 = await (async () => { const a = performance.now(); await raf(); return performance.now() - a; })();
    const f2 = await frameAfter();
    for (const c of cs) c.remove();
    await raf(); await raf();
    return { sync: Math.round(sync * 10) / 10, firstFrame: Math.round(f1 * 10) / 10, nextFrame: f2 };
  };
  await raf(); await raf();
  r.canvases40small = await many(40, 164, 124);
  r.canvases100small = await many(100, 164, 124);
  r.canvas1window = await many(1, W - 1, H - 1);
  host.remove();
  return r;
`;

async function waitFor(desc, body, timeout = 15000) {
  const t0 = Date.now();
  let last;
  while (Date.now() - t0 < timeout) {
    try {
      last = await D.exec(body);
      if (last) return last;
    } catch (e) {
      last = e.message;
    }
    await sleep(50);
  }
  throw new Error(`timeout: ${desc} (last ${JSON.stringify(last)})`);
}
const invoke = (cmd, args) => D.run('return await window.__nvInvoke(args[0], args[1]);', cmd, args);
const want = (name) => !ONLY.length || ONLY.includes(name);

async function measure(name, fn) {
  if (!want(name)) return fn(() => {});
  await D.exec(`window.__jank.start(${JSON.stringify(name)})`);
  await D.startTrace?.(name);
  await fn((m) => D.exec(`window.__jank.mark(${JSON.stringify(m)})`));
  const prof = await D.stopTrace?.();
  const raw = await D.exec('return window.__jank.stop()');
  report(name, raw, prof);
  const runs = await D.exec('const t = window.__jank.t0; return (window.__nvRevealRuns || []).filter((r) => r.t >= t)').catch(() => []);
  for (const r of runs || []) if (r.targets > 3) console.log(`      run ${r.root} ${r.dir > 0 ? 'reveal' : 'hide'} items ${r.items} targets ${r.targets} layout ${r.layout} ms, setup ${r.setup} ms (collect ${r.collect}: media ${r.media}, text ${r.text}; hold ${r.hold} [reads ${r.reads}, canvas ${r.canvas}], clear ${r.clear}), ready ${r.ready} ms, ${r.frames} frames, tick ${r.tickMean}/${r.tickMax} ms (text ${r.textMs}, media ${r.mediaMs}), ended ${r.why} at ${r.at}/${r.end} ms`);
  results.at(-1).runs = runs;
}
function report(name, raw, prof) {
  const s = stats(raw);
  results.push({ name, ...s, load: loadavg()[0], raw, prof, loaf: raw.loaf });
  const pad = (x, n) => String(x).padStart(n);
  console.log(`${name.padEnd(22)} ${pad(s.frames, 4)} fr ${pad(s.ms, 5)} ms ${pad(s.fps, 5)} fps  drop ${pad(s.drop, 5)}%  p50 ${pad(s.p50, 5)}  p95 ${pad(s.p95, 5)}  p99 ${pad(s.p99, 5)}  max ${pad(s.max, 6)}  >16.7 ${pad(s.over16, 5)}%  >25 ${pad(s.over25, 5)}%  >33 ${pad(s.over33, 5)}%  rafJS ${s.jsMean}/${Math.round(s.jsMax)}  LT ${s.longTasks}/${s.longMs}ms  dom ${s.dom}  load ${loadavg()[0].toFixed(1)}`);
  if (raw.api?.length) console.log(`      api: ${raw.api.slice(0, 7).map(([k, n, ms]) => `${k} ${n}x ${ms}ms`).join(' · ')}`);
  if (s.heldMax) console.log(`      decode census: glyph canvases max ${s.canvasesMax} (overlays ${s.overlaysMax}), held max ${s.heldMax}, of them off screen ${s.heldOffscreenMax}, rAF callbacks/frame max ${s.rafCallbacksMax}`);
  for (const w of s.worst.slice(0, 3)) console.log(`      worst ${w.ms} ms @${w.at} (rAF js ${w.rafJs}, render ${w.render}, tasks ${w.tasks}) ${w.during.join(' | ')}`);
  if (prof) {
    console.log(`      main: ${Object.entries(prof.trace.main).sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k} ${v}`).join(', ')}`);
    console.log(`      threads: ${Object.entries(prof.trace.threads).map(([k, v]) => `${k} ${v}`).join(', ')}`);
    console.log(`      top events: ${prof.trace.top.slice(0, 8).map(([k, v]) => `${k} ${v}`).join(', ')}`);
    console.log(`      top JS: ${prof.profile.slice(0, 8).map(([k, v]) => `${k} ${v}`).join(' · ')}`);
  }
}

const itemRef = (name) =>
  waitFor(`item ${name}`, `const o = [...document.querySelectorAll('[role=option]')].find((o) => o.querySelector('[data-testid=item-name]')?.textContent.trim() === ${JSON.stringify(name)}); if (o) { const r = o.getBoundingClientRect(); if (r.top < 0 || r.bottom > innerHeight) o.scrollIntoView({ block: 'center' }); } return o`);
async function openItem(name, opened) {
  const ref = await itemRef(name);
  await D.dblElement(CHROMIUM ? await D.page.locator(`[role=option]:has([data-testid=item-name]:text-is(${JSON.stringify(name)}))`).first() : ref);
  if (!opened) return;
  if (!(await waitFor('opened', opened, 2000).catch(() => false))) {
    console.log(`      (${name}: pointer double click not taken, dispatching dblclick)`);
    await D.exec(`[...document.querySelectorAll('[role=option]')].find((o) => o.querySelector('[data-testid=item-name]')?.textContent.trim() === ${JSON.stringify(name)}).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  }
}
async function rightItem(name) {
  const ref = await itemRef(name);
  await D.rightElement(CHROMIUM ? await D.page.locator(`[role=option]:has([data-testid=item-name]:text-is(${JSON.stringify(name)}))`).first() : ref);
}
const here = (name) => waitFor(`in ${name}`, `return document.querySelector('.breadcrumb [aria-current=page]')?.textContent.trim() === ${JSON.stringify(name)} && document.querySelector('[role=listbox]')?.getAttribute('aria-busy') !== 'true'`);
const back = () => D.exec('window.dispatchEvent(new MouseEvent("mouseup", { button: 3, bubbles: true }))');
const decodeDone = (re, timeout = 6000) =>
  waitFor(`decode ${re}`, `const t = window.__jank.t0; return (window.__nvReveal || []).some((e) => e.t >= t && (e.what === 'done' || e.what === 'reduced') && new RegExp(${JSON.stringify(re)}).test(e.key))`, timeout).catch(() => false);
async function wheelFor(css, ms, dy = 100) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    await D.wheel(css, dy);
    await sleep(CHROMIUM ? 50 : 35);
  }
}
const scrollTop = (css) => D.exec(`let e = document.querySelector(${JSON.stringify(css)}); while (e && !(e.scrollHeight > e.clientHeight + 4 && /auto|scroll/.test(getComputedStyle(e).overflowY))) e = e.parentElement; if (e) e.scrollTop = 0; return e ? e.scrollHeight : 0`);

function hostFixtures() {
  const WORK = process.env.NV_E2E_WORK;
  const dir = join(WORK, 'jank-host');
  mkdirSync(dir, { recursive: true });
  execFileSync('python3', ['-c', `
import os, sys, numpy as np
from PIL import Image
d = sys.argv[1]
rng = np.random.default_rng(7)
def photo(w, h, seed, name):
    r = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w]
    base = np.stack([(x * (0.3 + r.random()) + y * r.random()) % 256, (y * 0.5 + 60 * np.sin(x / (40 + 90 * r.random()))) % 256, (x * y / (w * 2.0)) % 256], -1)
    noise = r.integers(0, 40, (h, w, 3))
    Image.fromarray(np.clip(base + noise, 0, 255).astype(np.uint8)).save(os.path.join(d, name), quality=88)
for i in range(6): photo(4032, 3024, i, f'camera {i+1:02d}.jpg')
for i in range(14): photo(1280, 960, 100 + i, f'foto {i+1:02d}.jpg')
for i in range(500): open(os.path.join(d, f'nota {i+1:03d}.txt'), 'w').write(f'nota {i}\\n')
import shutil
for i in range(50): shutil.copyfile(os.path.join(d, f'foto {i % 14 + 1:02d}.jpg'), os.path.join(d, f'pic {i+1:03d}.jpg'))
`, dir]);
  return dir;
}

function csv2000() {
  const rows = ['name,url,username,password'];
  const words = ['Banca', 'Posta', 'Forum', 'Negozio', 'Cloud', 'Lavoro', 'Scuola', 'Comune', 'Treni', 'Energia'];
  for (let i = 1; i <= 2000; i++) rows.push(`${words[i % 10]} ${String(i).padStart(4, '0')},https://${words[i % 10].toLowerCase()}${i}.example.org,utente${i},pw-${(i * 7919).toString(36)}-x`);
  return rows.join('\n');
}

async function setupReal() {
  const host = hostFixtures();
  const WORK = process.env.NV_E2E_WORK;
  const vol = join(WORK, 'stick', 'Jank.nvault');
  const info = await invoke('vault_create', { path: vol, password: PASSWORD, kdf: 'fast' });
  const id = info.id;
  const q = await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Quaranta' });
  for (let i = 1; i <= 20; i++) await invoke('fs_mkdir', { vault: id, parent: q.id, name: `Busta ${String(i).padStart(2, '0')}` });
  const photos = [...Array.from({ length: 6 }, (_, i) => `camera ${String(i + 1).padStart(2, '0')}.jpg`), ...Array.from({ length: 14 }, (_, i) => `foto ${String(i + 1).padStart(2, '0')}.jpg`)];
  await invoke('fs_import', { vault: id, parent: q.id, host_paths: photos.map((p) => join(host, p)) });
  const c = await invoke('fs_mkdir', { vault: id, parent: 1, name: 'Cinquecento' });
  const notes = Array.from({ length: 450 }, (_, i) => join(host, `nota ${String(i + 1).padStart(3, '0')}.txt`));
  await invoke('fs_import', { vault: id, parent: c.id, host_paths: notes });
  await invoke('fs_import', { vault: id, parent: c.id, host_paths: Array.from({ length: 50 }, (_, i) => join(host, `pic ${String(i + 1).padStart(3, '0')}.jpg`)) });
  const scale = [];
  for (const [name, n] of [['Dieci', 10], ['Q40', 40], ['C150', 150]]) {
    const f = await invoke('fs_mkdir', { vault: id, parent: 1, name });
    const k = n / 10;
    await invoke('fs_import', { vault: id, parent: f.id, host_paths: [...Array.from({ length: n - k }, (_, i) => join(host, `nota ${String(i + 1).padStart(3, '0')}.txt`)), ...Array.from({ length: k }, (_, i) => join(host, `pic ${String(i + 1).padStart(3, '0')}.jpg`))] });
    scale.push([f.id, n]);
  }
  await invoke('pm_import', { vault: id, kind: 'csv', path: null, data: csv2000(), master_password: null, keyfile: null });
  const t0 = Date.now();
  for (;;) {
    const a = await invoke('fs_list', { vault: id, dir: q.id });
    const b = await invoke('fs_list', { vault: id, dir: c.id });
    const p = await invoke('pm_list', { vault: id });
    let okScale = true;
    for (const [fid, n] of scale) if ((await invoke('fs_list', { vault: id, dir: fid })).length < n) okScale = false;
    if (a.length >= 40 && b.length >= 500 && p.length >= 2000 && okScale) break;
    if (Date.now() - t0 > 120000) throw new Error(`setup: ${a.length} / ${b.length} / ${p.length}`);
    await sleep(500);
  }
  await invoke('vault_lock', { vault: id });
}

async function setupMock() {
  await D.run(`
    const inv = window.__nvInvoke;
    window.__nv.mock.opts.speed = 0;
    const [r] = await inv('recent_list');
    const id = (await inv('vault_open', { path: r.path, password: 'demo' })).id;
    const photo = async (w, h, seed) => {
      const c = new OffscreenCanvas(w, h); const x = c.getContext('2d');
      const g = x.createLinearGradient(0, 0, w, h); g.addColorStop(0, 'hsl(' + seed * 37 + ' 60% 40%)'); g.addColorStop(1, 'hsl(' + seed * 91 + ' 50% 70%)');
      x.fillStyle = g; x.fillRect(0, 0, w, h);
      const img = x.getImageData(0, 0, w, Math.min(h, 600)); for (let i = 0; i < img.data.length; i += 4) img.data[i] ^= (i * 2654435761 >>> 24) & 31; x.putImageData(img, 0, 0);
      return c.convertToBlob({ type: 'image/jpeg', quality: 0.88 });
    };
    const files = [];
    for (let i = 0; i < 6; i++) files.push(new File([await photo(4032, 3024, i)], 'camera ' + String(i + 1).padStart(2, '0') + '.jpg', { type: 'image/jpeg' }));
    for (let i = 0; i < 14; i++) files.push(new File([await photo(1280, 960, 100 + i)], 'foto ' + String(i + 1).padStart(2, '0') + '.jpg', { type: 'image/jpeg' }));
    const q = await inv('fs_mkdir', { vault: id, parent: 1, name: 'Quaranta' });
    for (let i = 1; i <= 20; i++) await inv('fs_mkdir', { vault: id, parent: q.id, name: 'Busta ' + String(i).padStart(2, '0') });
    await inv('fs_import', { vault: id, parent: q.id, host_paths: files });
    const c = await inv('fs_mkdir', { vault: id, parent: 1, name: 'Cinquecento' });
    const notes = Array.from({ length: 450 }, (_, i) => '/host/nota ' + String(i + 1).padStart(3, '0') + '.txt');
    await inv('fs_import', { vault: id, parent: c.id, host_paths: notes });
    const pics = []; for (let k = 0; k < 50; k++) pics.push(new File([files[6 + (k % 14)]], 'foto ' + String(k + 1).padStart(3, '0') + '.jpg', { type: 'image/jpeg' }));
    await inv('fs_import', { vault: id, parent: c.id, host_paths: pics });
    for (const [name, n] of [['Dieci', 10], ['Q40', 40], ['C150', 150]]) {
      const f = await inv('fs_mkdir', { vault: id, parent: 1, name });
      await inv('fs_import', { vault: id, parent: f.id, host_paths: [...notes.slice(0, n - n / 10), ...pics.slice(0, n / 10).map((p, i) => new File([p], 's' + n + ' ' + i + '.jpg', { type: 'image/jpeg' }))] });
    }
    await inv('pm_import', { vault: id, kind: 'csv', path: null, data: args[0], master_password: null, keyfile: null });
    for (let i = 0; i < 100; i++) {
      const a = await inv('fs_list', { vault: id, dir: q.id }), b = await inv('fs_list', { vault: id, dir: c.id }), p = await inv('pm_list', { vault: id });
      if (a.length >= 40 && b.length >= 500 && p.length >= 2000) break;
      await new Promise((r) => setTimeout(r, 200));
    }
    await inv('vault_lock', { vault: id });
    window.__nv.mock.opts.speed = 1;
    return id;
  `, csv2000());
}

async function main() {
  console.log(`jank: ${LABEL}${TOGGLES.length ? ` toggles ${TOGGLES.join(',')}` : ''} ${CHROMIUM ? 'chromium (mock)' : 'real app (WebKitGTK)'}; env ${['WEBKIT_DISABLE_COMPOSITING_MODE', 'WEBKIT_DISABLE_DMABUF_RENDERER', 'WEBKIT_SKIA_ENABLE_CPU_RENDERING', 'GDK_SCALE', 'LIBGL_ALWAYS_SOFTWARE', 'GDK_BACKEND'].filter((k) => process.env[k]).map((k) => `${k}=${process.env[k]}`).join(' ')}`);
  if (CHROMIUM) {
    D = await chromiumDriver();
    await waitFor('app ready', 'return document.querySelector("[data-ready=true]") && document.querySelector(".passport .book")');
    await decodeDone('^start:welcome$');
    await sleep(500);
    report('boot', await D.exec('return window.__jank.stop()'), null);
    await setupMock();
  } else {
    D = await realDriver();
    await waitFor('app ready', 'return document.querySelector("[data-ready=true]")', 30000);
    await D.exec(`return ${PROBE([], null)}`);
    await setupReal();
    await D.reload();
    await waitFor('app ready', 'return document.querySelector("[data-ready=true]") && document.querySelector(".passport .book")', 30000);
    await decodeDone('^start:welcome$');
    await sleep(500);
    report('boot', await D.exec('return window.__jank.stop()'), null);
  }
  const info = await D.exec('return { ua: navigator.userAgent, dpr: devicePixelRatio, w: innerWidth, h: innerHeight, entryTypes: window.__jank.supported, gl: (() => { try { const c = document.createElement("canvas").getContext("webgl"); const e = c.getExtension("WEBGL_debug_renderer_info"); return e ? c.getParameter(e.UNMASKED_RENDERER_WEBGL) : c.getParameter(c.RENDERER); } catch (e) { return "no webgl"; } })() }');
  console.log(`      ${JSON.stringify(info)}`);
  await measure('idle-welcome', () => sleep(2000));

  await measure('passport-hover', async (mark) => {
    for (let i = 0; i < 3; i++) {
      mark('hover-on');
      await D.hover('.passport .book');
      await sleep(600);
      mark('hover-off');
      await D.moveTo(20, 20);
      await sleep(600);
    }
  });

  await measure('unlock-arrive', async () => {
    await D.click('.passport .book');
    await waitFor('password field', 'return document.querySelector("input[type=password]")');
    await decodeDone('^start:unlock$');
    await sleep(300);
  });
  await measure('unlock-ceremony', async (mark) => {
    mark('enter');
    await D.typeInto('input[type=password]', PASSWORD + (CHROMIUM ? '' : KEY.Enter), 0);
    if (CHROMIUM) await D.key('Enter');
    await waitFor('vault', 'return document.querySelector("[data-screen=vault]")', 30000);
    mark('vault');
    await decodeDone('^ceremony:');
    await sleep(800);
  });
  await waitFor('Quaranta listed', 'return [...document.querySelectorAll("[data-testid=item-name]")].some((e) => e.textContent.trim() === "Quaranta")', 20000);
  await sleep(400);
  await measure('idle-vault', () => sleep(2000));
  if (process.env.NV_JANK_WINDOW && D.wd) {
    const [w, h] = process.env.NV_JANK_WINDOW.split('x').map(Number);
    await D.wd.cmd('POST', '/window/rect', { x: 0, y: 0, width: w, height: h }).catch((e) => console.log(`      (window not resized: ${e.message})`));
    await sleep(800);
    console.log(`      window ${JSON.stringify(await D.exec('return { w: innerWidth, h: innerHeight }'))}`);
  }
  if (process.env.NV_JANK_SIZE) {
    await D.exec(`document.querySelector('[data-testid=view-size] [data-size=${process.env.NV_JANK_SIZE}]').closest('button').click()`);
    await sleep(1100);
  }

  await D.exec('window.__jank.census = true');
  await measure('folder-40', async (mark) => {
    mark('open');
    await openItem('Quaranta');
    await here('Quaranta');
    await decodeDone('^[0-9a-f]{8,}:\\d+$|^mock|:\\d+$');
    await sleep(2500);
  });
  await D.exec('window.__jank.census = false');

  await measure('quicklook', async (mark) => {
    for (const n of ['camera 01.jpg', 'foto 01.jpg', 'camera 02.jpg']) {
      mark(`open ${n}`);
      await openItem(n, 'return document.querySelector("dialog.quick-look[open]")');
      await waitFor('quick look', 'return document.querySelector("dialog.quick-look[open]")');
      await sleep(1200);
      mark('close');
      await D.key('Escape');
      await waitFor('quick look closed', 'return !document.querySelector("dialog.quick-look[open]")');
      await sleep(500);
    }
  });
  await measure('context-menu', async (mark) => {
    for (const n of ['foto 02.jpg', 'Busta 01', 'foto 03.jpg']) {
      mark(`menu ${n}`);
      await rightItem(n);
      await waitFor('menu', 'return document.querySelector("[role=menu]")');
      await sleep(500);
      await D.key('Escape');
      await sleep(400);
    }
  });
  await sleep(1100);
  await back();
  await waitFor('back at the root', 'return [...document.querySelectorAll("[data-testid=item-name]")].some((e) => e.textContent.trim() === "Cinquecento")');
  await sleep(400);

  await D.exec('window.__jank.census = true');
  for (const [folder, n] of [['Dieci', 10], ['Q40', 40], ['C150', 150], ['Cinquecento', 500]]) {
    await measure(n === 500 ? 'folder-500-open' : `decode-${n}`, async (mark) => {
      mark('open');
      await openItem(folder);
      await here(folder);
      await decodeDone(':\\d+$');
      await sleep(1500);
    });
    if (n !== 500) {
      await back();
      await waitFor('back at the root', 'return [...document.querySelectorAll("[data-testid=item-name]")].some((e) => e.textContent.trim() === "Cinquecento")');
      await sleep(1100);
    }
  }
  await D.exec('window.__jank.census = false');
  await measure('scroll-500-grid', async () => {
    await wheelFor('[role=listbox]', 4000);
    await sleep(300);
  });
  await scrollTop('[role=listbox] .items');
  await D.click('.view-toggle button[aria-pressed=false]');
  await sleep(800);
  await measure('scroll-500-list', async () => {
    await wheelFor('[role=listbox]', 4000);
    await sleep(300);
  });
  await D.click('.view-toggle button[aria-pressed=false]');
  await sleep(300);

  await measure('pm-open', async () => {
    await D.click('[data-testid=nav-passwords]');
    await waitFor('pm list', 'return Number(document.querySelector("[data-testid=pm-list]")?.dataset.count) >= 2000 && document.querySelector("[data-testid=pm-entry]")', 20000);
    await sleep(800);
  });
  await measure('pm-scroll', async () => {
    await wheelFor('[data-testid=pm-list]', 3000);
    await sleep(300);
  });
  await measure('pm-search', async (mark) => {
    mark('type');
    await D.typeInto('[data-testid=pm-search]', 'banca 01', 140);
    await sleep(600);
    mark('clear');
    for (let i = 0; i < 8; i++) {
      await D.key('Backspace');
      await sleep(140);
    }
    await sleep(600);
  });

  await measure('settings', async (mark) => {
    mark('open');
    await D.click('[data-testid=open-settings]');
    await waitFor('settings', 'return document.querySelector("[data-testid=settings]")');
    await sleep(1200);
    mark('back');
    await D.click('[data-testid=settings-back]');
    await sleep(800);
  });

  let bench = null;
  if (want('bench')) {
    bench = await D.run(BENCH, await D.exec('return devicePixelRatio'));
    console.log(`bench (ms, median of 15): ${JSON.stringify(bench)}`);
  }

  const file = join(OUT, `jank-${LABEL}.json`);
  writeFileSync(file, JSON.stringify({ label: LABEL, toggles: TOGGLES, chromium: CHROMIUM, info, bench, env: Object.fromEntries(Object.entries(process.env).filter(([k]) => /^(WEBKIT|GDK|LIBGL|GSK|NV_JANK|WAYLAND_DISPLAY|DISPLAY)/.test(k))), results }, null, 1));
  console.log(`wrote ${file}`);
  if (!CHROMIUM && process.env.NV_JANK_GPUINFO !== '0') {
    try {
      await D.wd.cmd('POST', '/url', { url: 'webkit://gpu' });
      await sleep(800);
      const gpu = await D.exec('return document.body.innerText');
      writeFileSync(join(OUT, `gpu-${LABEL}.txt`), gpu);
      console.log(`      webkit://gpu: ${gpu.split('\n').filter((l) => /accelerat|Renderer|DMA|Buffer|API|Skia|Version|backend|GL_RENDERER|Composit|Display|Type/i.test(l)).slice(0, 24).join(' | ')}`);
    } catch (e) {
      console.log(`      webkit://gpu unavailable: ${e.message}`);
    }
  }
  await D.quit();
  process.exit(0);
}

main().catch(async (e) => {
  console.log('FAIL', e.stack || e.message);
  try {
    await D?.quit();
  } catch {
  }
  process.exit(1);
});

