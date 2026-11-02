import { expect, type Page, type TestInfo } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

export function rng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export interface Watch {
  errors: string[];
}

export async function watch(page: Page): Promise<Watch> {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(`console: ${m.text()}`);
  });
  page.context().on('page', (p) => void p.close().catch(() => undefined));
  await page.addInitScript(() => {
    const w = window as unknown as { __scimmiaLong: number[] };
    w.__scimmiaLong = [];
    addEventListener('unhandledrejection', (e) => console.error(`unhandled: ${String((e.reason as Error)?.message ?? e.reason)}`));
    try {
      new PerformanceObserver((list) => {
        for (const e of list.getEntries()) if (e.duration > 200) w.__scimmiaLong.push(Math.round(e.duration));
      }).observe({ type: 'long-animation-frame', buffered: false });
    } catch {
    }
  });
  return { errors };
}

export const longFrames = (page: Page) =>
  page.evaluate(() => (window as unknown as { __scimmiaLong?: number[] }).__scimmiaLong?.splice(0) ?? []);

export interface Combo {
  query: string;
  phone?: boolean;
}

export const STARTS = [
  'screen=welcome',
  'screen=vault',
  'screen=vault&view=list&jobs',
  'screen=passwords',
  'screen=details',
  'screen=vault&dir=Foto',
  'screen=welcome&recents=long',
  'screen=passwords&pm=2000',
];

export async function boot(page: Page, query: string): Promise<Watch> {
  const w = await watch(page);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto(`/?${query}`);
  await expect(page.locator('[data-ready="true"]')).toHaveCount(1, { timeout: 20_000 });
  return w;
}

const AVOID = /^(esci|quit)\b|esci da nicevault|quit nicevault|distruggi|destroy|^150 ?%$/i;

const ODD_TEXT = [
  'demo',
  'demo',
  'Foto',
  'a',
  '',
  '   ',
  'ﺍﻟﺴﻼﻡ عليكم',
  '👩‍👩‍👧‍👦🏳️‍🌈',
  'x'.repeat(300),
  '../../etc/passwd',
  '<img src=x onerror=alert(1)>',
  'Ünïcödé — “quoted” \'single\'',
  'CON',
  'nome.con.molti.punti.txt',
  '\t',
];

const KEYS = [
  'Escape', 'Escape', 'Enter', 'Tab', 'Shift+Tab', 'ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight', 'Home', 'End',
  'PageDown', 'PageUp', 'Space', 'Delete', 'Backspace', 'F2', 'Control+a', 'Control+c', 'Control+x', 'Control+v',
  'Control+z', 'Control+Shift+Z', 'Control+f', 'Control+,', 'Control+n', 'Control+Shift+N', 'Alt+ArrowLeft', 'Alt+ArrowUp',
  'Alt+ArrowRight', 'Control+l', 'Control+Shift+L', 'Control+=', 'Control+-', 'Control+0', 'Control+i', 'Control+k', 'F1',
  'Control+g', 'Shift+ArrowDown', 'Control+ArrowDown', 'Control+Enter', 'Alt+Enter',
];

const CANDIDATES = [
  'button', 'a[href]', '[role=button]', '[role=option]', '[role=menuitem]', '[role=menuitemradio]', '[role=tab]',
  '[role=radio]', '[role=switch]', '[role=checkbox]', '[role=treeitem]', '[role=link]', 'summary', 'label',
  'input:not([type=hidden])', 'textarea', 'select', '[tabindex="0"]',
].join(',');

interface Target {
  x: number;
  y: number;
  desc: string;
  field: boolean;
  w: number;
  h: number;
}

function pickTarget(page: Page, r: number, onlyFields = false): Promise<Target | null> {
  return page.evaluate(
    ({ sel, r, onlyFields, avoid }) => {
      const re = new RegExp(avoid, 'i');
      const out: { el: Element; x: number; y: number; w: number; h: number }[] = [];
      for (const el of document.querySelectorAll(sel)) {
        if ((el as HTMLButtonElement).disabled || el.closest('[inert], [aria-hidden="true"]')) continue;
        if (onlyFields && !el.matches('input, textarea, [contenteditable="true"]')) continue;
        const rect = el.getBoundingClientRect();
        if (rect.width < 2 || rect.height < 2) continue;
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue;
        const hit = document.elementFromPoint(x, y);
        if (!hit || !(hit === el || el.contains(hit) || (el instanceof HTMLLabelElement && el.control === hit))) continue;
        const label = (el.getAttribute('aria-label') ?? (el as HTMLElement).innerText ?? '').trim();
        if (re.test(label)) continue;
        out.push({ el, x, y, w: rect.width, h: rect.height });
      }
      if (!out.length) return null;
      const t = out[Math.floor(r * out.length)];
      const el = t.el as HTMLElement;
      const name = (el.getAttribute('aria-label') ?? el.innerText ?? el.getAttribute('placeholder') ?? '').trim().replace(/\s+/g, ' ').slice(0, 40);
      const role = el.getAttribute('role') ?? el.tagName.toLowerCase();
      const tid = el.dataset.testid ? `#${el.dataset.testid}` : '';
      return { x: t.x, y: t.y, w: t.w, h: t.h, field: el.matches('input, textarea, select, [contenteditable="true"]'), desc: `${role}${tid} «${name}»` };
    },
    { sel: CANDIDATES, r, onlyFields, avoid: AVOID.source },
  );
}

interface Probe {
  ready: boolean;
  screen: string;
  hscroll: string | null;
  covered: string[];
  modalFocusEscaped: string | null;
  openLayers: string[];
  busy: string[];
  reachable: number;
  outside: string[];
}

function probe(page: Page): Promise<Probe> {
  return page.evaluate(() => {
    const root = document.querySelector('[data-ready]');
    const vis = (el: Element) => {
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      return r.width > 1 && r.height > 1 && s.visibility !== 'hidden' && s.display !== 'none' && Number(s.opacity) > 0.05;
    };
    const desc = (el: Element) => {
      const h = el as HTMLElement;
      const name = (h.getAttribute('aria-label') ?? h.innerText ?? '').trim().replace(/\s+/g, ' ').slice(0, 30);
      return `${h.tagName.toLowerCase()}${h.dataset?.testid ? '#' + h.dataset.testid : ''}${h.className && typeof h.className === 'string' ? '.' + h.className.trim().split(/\s+/).slice(0, 2).join('.') : ''}${name ? ` «${name}»` : ''}`;
    };
    const clip = (el: Element) => {
      let r = el.getBoundingClientRect();
      let { left, top, right, bottom } = r;
      for (let p = el.parentElement; p; p = p.parentElement) {
        const s = getComputedStyle(p);
        if (s.overflowX !== 'visible' || s.overflowY !== 'visible' || s.contain.includes('paint')) {
          r = p.getBoundingClientRect();
          left = Math.max(left, r.left);
          top = Math.max(top, r.top);
          right = Math.min(right, r.right);
          bottom = Math.min(bottom, r.bottom);
        }
        if (s.position === 'fixed') break;
      }
      return { left: Math.max(left, 0), top: Math.max(top, 0), right: Math.min(right, innerWidth), bottom: Math.min(bottom, innerHeight) };
    };
    const LAYER = 'dialog[open], [role=dialog], [role=alertdialog], [role=menu], [role=tooltip], [role=listbox][popover], [popover], [role=status], [role=alert], [aria-live], [data-testid=drop-overlay], [role=region]';
    const layerOf = (el: Element) => el.closest(LAYER);
    const scrollsClear = (el: Element, hit: Element) => {
      for (let p = el.parentElement; p; p = p.parentElement) {
        if (!/auto|scroll/.test(getComputedStyle(p).overflowY) || p.scrollHeight <= p.clientHeight + 1) continue;
        return !p.contains(hit) && (p.scrollTop > 0 || p.scrollTop + p.clientHeight < p.scrollHeight - 1);
      }
      return false;
    };
    const floats = (hit: Element, el: Element) => {
      for (let p: Element | null = hit; p && p !== document.body; p = p.parentElement) {
        if (p.contains(el)) return false;
        const pos = getComputedStyle(p).position;
        if (pos === 'fixed' || pos === 'sticky') return true;
      }
      return false;
    };

    const doc = document.scrollingElement ?? document.documentElement;
    const hscroll = doc.scrollWidth > innerWidth + 1 ? `page scrolls sideways: ${doc.scrollWidth} > ${innerWidth}` : null;

    const covered: string[] = [];
    for (const el of document.querySelectorAll('button, [role=option], [role=menuitem], [role=tab], [role=radio], a[href], h1, h2, h3, label')) {
      if (el.closest('[inert], [aria-hidden="true"]') || !vis(el)) continue;
      const text = (el as HTMLElement).innerText?.trim();
      if (!text) continue;
      if (getComputedStyle(el).pointerEvents === 'none') continue;
      const c = clip(el);
      if (c.right - c.left < 4 || c.bottom - c.top < 4) continue;
      const x = (c.left + c.right) / 2;
      const y = (c.top + c.bottom) / 2;
      const hit = document.elementFromPoint(x, y);
      if (!hit || hit === el || el.contains(hit) || hit.contains(el)) continue;
      if (getComputedStyle(hit).pointerEvents === 'none') continue;
      const lh = layerOf(hit);
      if (lh && lh !== layerOf(el)) continue;
      if (floats(hit, el)) continue;
      if (scrollsClear(el, hit)) continue;
      if (el instanceof HTMLLabelElement && (el.control === hit || hit.closest('label') === el)) continue;
      covered.push(`${desc(el)} under ${desc(hit)}`);
    }

    let modalFocusEscaped: string | null = null;
    const modal = [...document.querySelectorAll('dialog[open]')].find((d) => d.matches(':modal'));
    const a = document.activeElement;
    if (modal && a && a !== document.body && !modal.contains(a)) modalFocusEscaped = `${desc(a)} outside ${desc(modal)}`;

    const openLayers = [
      ...[...document.querySelectorAll('dialog[open], [role=menu], [role=dialog]:not(dialog), [popover]')].filter((e) => {
        if (e.matches('[popover]') && !e.matches(':popover-open')) return false;
        return vis(e);
      }),
    ].map(desc);

    const busy = [...document.querySelectorAll('[aria-busy="true"], [role=progressbar]:not([aria-valuenow])')].filter(vis).map(desc);

    let reachable = 0;
    for (const el of document.querySelectorAll('button, a[href], input, [role=option], [role=menuitem], [tabindex="0"]')) {
      if ((el as HTMLButtonElement).disabled || el.closest('[inert]') || !vis(el)) continue;
      const r = el.getBoundingClientRect();
      if (r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight) reachable++;
    }

    const outside: string[] = [];
    for (const el of document.querySelectorAll('button, [role=tab], input, [role=menuitem]')) {
      if (el.closest('[inert], [aria-hidden="true"]') || !vis(el)) continue;
      const r = el.getBoundingClientRect();
      if (r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1 && r.left >= -1 && r.top >= -1) continue;
      let clipped = false;
      for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
        const s = getComputedStyle(p);
        if (s.overflowX !== 'visible' || s.overflowY !== 'visible' || s.position === 'fixed') {
          clipped = true;
          break;
        }
      }
      if (!clipped) outside.push(`${desc(el)} at ${Math.round(r.left)},${Math.round(r.top)}`);
    }

    return {
      ready: !!root && root.getAttribute('data-ready') === 'true',
      screen: root?.getAttribute('data-screen') ?? '',
      hscroll,
      covered: covered.slice(0, 6),
      modalFocusEscaped,
      openLayers,
      busy,
      reachable,
      outside: outside.slice(0, 6),
    };
  });
}

export interface MonkeyOptions {
  seed: number;
  steps: number;
  phone?: boolean;
  maxFrame?: number;
  busyMs?: number;
  sizes?: { width: number; height: number }[];
  shots?: string;
}

const DESKTOP_SIZES = [
  { width: 1280, height: 800 },
  { width: 1024, height: 700 },
  { width: 1920, height: 1080 },
  { width: 900, height: 600 },
  { width: 1440, height: 900 },
  { width: 800, height: 560 },
];

export class Failure extends Error {}

export async function monkey(page: Page, w: Watch, info: TestInfo, opts: MonkeyOptions) {
  const r = rng(opts.seed);
  const pick = <T>(xs: readonly T[]): T => xs[Math.floor(r() * xs.length)];
  const trail: string[] = [];
  const busySince = new Map<string, number>();
  const shots = opts.shots ?? path.join(info.project.outputDir, 'scimmia');
  fs.mkdirSync(shots, { recursive: true });
  const tag = `${info.project.name}-seed${opts.seed}`;
  let step = 0;
  let coveredBefore = new Set<string>();
  const slow: { step: number; action: string; ms: number }[] = [];

  const fail = async (what: string) => {
    const file = path.join(shots, `${tag}-step${step}.png`);
    await page.screenshot({ path: file }).catch(() => undefined);
    await info.attach(`seed ${opts.seed} step ${step}`, { path: file, contentType: 'image/png' }).catch(() => undefined);
    throw new Failure(
      `${what}\n  project ${info.project.name}, seed ${opts.seed}, step ${step} (replay: SCIMMIA_SEEDS=${opts.seed} SCIMMIA_STEPS=${step} pnpm scimmia --project=${info.project.name})\n  screenshot: ${file}\n  last steps:\n    ${trail.slice(-12).join('\n    ')}`,
    );
  };

  const settle = async () => {
    await page.waitForTimeout(40);
    await page.evaluate(() => new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)))).catch(() => undefined);
  };

  const click = async (t: Target, kind: 'click' | 'double' | 'right' | 'tap') => {
    const x = t.x + (r() - 0.5) * Math.min(t.w - 2, 20) * 0.8;
    const y = t.y + (r() - 0.5) * Math.min(t.h - 2, 12) * 0.8;
    if (kind === 'tap') await page.touchscreen.tap(x, y);
    else if (kind === 'double') await page.mouse.dblclick(x, y);
    else await page.mouse.click(x, y, { button: kind === 'right' ? 'right' : 'left' });
  };

  const openSettings = async () => {
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+,');
    return page.getByTestId('settings').isVisible({ timeout: 1500 }).catch(() => false);
  };

  type Action = [weight: number, run: () => Promise<string>];
  const actions: Action[] = [
    [40, async () => {
      const t = await pickTarget(page, r());
      if (!t) return 'click: nothing to click';
      const kind = opts.phone && r() < 0.6 ? 'tap' : r() < 0.08 ? 'double' : r() < 0.08 ? 'right' : 'click';
      await click(t, kind);
      return `${kind} ${t.desc}`;
    }],
    [10, async () => {
      const t = await pickTarget(page, r(), true);
      if (!t) return 'type: no field';
      await click(t, 'click');
      const text = pick(ODD_TEXT);
      if (r() < 0.5) await page.keyboard.press('Control+a');
      await page.keyboard.insertText(text);
      const then = r() < 0.4 ? 'Enter' : r() < 0.2 ? 'Escape' : null;
      if (then) await page.keyboard.press(then);
      return `type into ${t.desc}: ${JSON.stringify(text.slice(0, 24))}${then ? ` + ${then}` : ''}`;
    }],
    [14, async () => {
      const k = pick(KEYS);
      await page.keyboard.press(k);
      return `key ${k}`;
    }],
    [4, async () => {
      for (let i = 0; i < 6; i++) await page.keyboard.press('Escape');
      await settle();
      await page.waitForTimeout(250);
      const p = await probe(page);
      const left = p.openLayers.filter((l) => !/aria-busy|progress/i.test(l));
      if (left.length) await fail(`Escape ×6 left open: ${left.join(', ')}`);
      return 'Escape ×6';
    }],
    [5, async () => {
      const t = await pickTarget(page, r());
      if (!t) return 'burst: nothing';
      const n = 3 + Math.floor(r() * 6);
      for (let i = 0; i < n; i++) await page.mouse.click(t.x, t.y, { delay: 0 });
      return `burst ×${n} ${t.desc}`;
    }],
    [5, async () => {
      const x = Math.floor(r() * (page.viewportSize()?.width ?? 800));
      const y = Math.floor(r() * (page.viewportSize()?.height ?? 600));
      const dy = Math.round((r() - 0.4) * 2400);
      await page.mouse.move(x, y);
      await page.mouse.wheel(0, dy);
      return `wheel ${dy} at ${x},${y}`;
    }],
    [5, async () => {
      const a = await pickTarget(page, r());
      const b = await pickTarget(page, r());
      if (!a || !b) return 'drag: nothing';
      await page.mouse.move(a.x, a.y);
      await page.mouse.down();
      await page.mouse.move((a.x + b.x) / 2, (a.y + b.y) / 2, { steps: 4 });
      await page.mouse.move(b.x, b.y, { steps: 4 });
      if (r() < 0.2) await page.keyboard.press('Escape');
      await page.mouse.up();
      return `drag ${a.desc} → ${b.desc}`;
    }],
    [3, async () => {
      const x = Math.floor(r() * (page.viewportSize()?.width ?? 800));
      const y = Math.floor(r() * (page.viewportSize()?.height ?? 600));
      const name = pick(['nota.txt', 'foto.jpg', 'x'.repeat(120) + '.pdf', '日本語.md', '.nascosto']);
      const steps = r() < 0.3 ? ['dragenter', 'dragover', 'dragleave'] : ['dragenter', 'dragover', 'dragover', 'drop'];
      await page.evaluate(
        ({ x, y, name, steps }) => {
          const hit = document.elementFromPoint(x, y) ?? document.body;
          const dt = new DataTransfer();
          dt.items.add(new File(['scimmia'], name, { type: '' }));
          for (const type of steps) hit.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, dataTransfer: dt }));
        },
        { x, y, name, steps },
      );
      return `file drop ${name} at ${x},${y} (${steps.at(-1)})`;
    }],
    [opts.phone ? 0 : 3, async () => {
      const s = pick(opts.sizes ?? DESKTOP_SIZES);
      await page.setViewportSize(s);
      return `resize ${s.width}×${s.height}`;
    }],
    [2, async () => {
      if (!(await openSettings())) return 'theme: settings did not open';
      const id = pick(['light', 'dark', 'system']);
      await page.locator(`[data-testid=app-theme] [data-theme-id=${id}]`).click({ timeout: 2000 }).catch(() => undefined);
      await page.keyboard.press('Escape');
      return `theme ${id}`;
    }],
    [2, async () => {
      if (!(await openSettings())) return 'lang: settings did not open';
      const id = pick(['it', 'en', 'system']);
      await page.locator(`[data-lang=${id}]`).click({ timeout: 2000 }).catch(() => undefined);
      await page.keyboard.press('Escape');
      return `language ${id}`;
    }],
    [3, async () => {
      const pw = page.locator('[data-screen=unlock] input[type=password], [data-screen=unlock] input[autocomplete=current-password]').first();
      if (await pw.isVisible().catch(() => false)) {
        await pw.fill(r() < 0.8 ? 'demo' : 'sbagliata', { timeout: 1500 }).catch(() => undefined);
        await page.keyboard.press('Enter');
        return 'unlock';
      }
      const lock = page.getByRole('button', { name: /^(Blocca|Lock)$/ }).first();
      if (await lock.isVisible().catch(() => false)) {
        await lock.click({ timeout: 1500 }).catch(() => undefined);
        return 'lock';
      }
      return 'lock: nothing to lock';
    }],
    [3, async () => {
      const opt = await pickTarget(page, r());
      if (!opt || !/^option/.test(opt.desc)) return 'view: no item';
      await page.mouse.click(opt.x, opt.y);
      const k = r() < 0.5 ? 'Space' : 'Enter';
      await page.keyboard.press(k);
      return `open ${opt.desc} with ${k}`;
    }],
    [2, async () => {
      const nav = page.getByTestId('nav-passwords').first();
      if (await nav.isVisible().catch(() => false)) {
        await nav.click({ timeout: 1500 }).catch(() => undefined);
        return 'passwords';
      }
      return 'passwords: not reachable from here';
    }],
  ];
  const total = actions.reduce((s, [wgt]) => s + wgt, 0);

  for (step = 1; step <= opts.steps; step++) {
    let roll = r() * total;
    const act = actions.find(([wgt]) => (roll -= wgt) < 0) ?? actions[0];
    let done: string;
    try {
      done = await act[1]();
    } catch (e) {
      if (e instanceof Failure) throw e;
      done = `(action threw: ${(e as Error).message.split('\n')[0]})`;
    }
    trail.push(`${step}. ${done}`);
    await settle();
    if (await page.evaluate(() => Number(document.documentElement.style.zoom || 1) > 1.3)) {
      await page.keyboard.press('Control+Shift+Minus');
      trail.push(`${step}. (interface size back under 130 %)`);
      await settle();
    }

    if (w.errors.length) await fail(`errors:\n  ${w.errors.join('\n  ')}`);
    for (const ms of await longFrames(page)) {
      slow.push({ step, action: done, ms });
      if (ms > (opts.maxFrame ?? 3000)) await fail(`a frame of ${ms} ms`);
    }
    const lag = await Promise.race([
      page.evaluate(() => new Promise<number>((ok) => {
        const t = performance.now();
        setTimeout(() => ok(performance.now() - t), 0);
      })),
      new Promise<number>((ok) => setTimeout(() => ok(Infinity), 5000)),
    ]);
    if (lag > 2000) await fail(`the main thread was blocked for ${Math.round(lag)} ms`);

    let p = await probe(page);
    if (!p.ready) {
      await page.waitForTimeout(1500);
      p = await probe(page);
      if (!p.ready) await fail('the app is not mounted (no [data-ready=true])');
    }
    if (p.hscroll) await fail(p.hscroll);
    if (p.modalFocusEscaped) await fail(`focus left the open modal: ${p.modalFocusEscaped}`);
    if (p.reachable === 0) {
      await page.waitForTimeout(4000);
      if ((await probe(page)).reachable === 0) await fail('nothing on screen can be clicked');
    }
    const nowCovered = new Set(p.covered);
    const stuck = [...nowCovered].filter((c) => coveredBefore.has(c));
    if (stuck.length) {
      await page.waitForTimeout(400);
      const again = (await probe(page)).covered.filter((c) => stuck.includes(c));
      if (again.length) await fail(`text covered by another element:\n  ${again.join('\n  ')}`);
    }
    coveredBefore = nowCovered;
    if (p.outside.length) await fail(`controls outside the window:\n  ${p.outside.join('\n  ')}`);
    const now = Date.now();
    for (const b of p.busy) if (!busySince.has(b)) busySince.set(b, now);
    for (const b of [...busySince.keys()]) {
      if (!p.busy.includes(b)) busySince.delete(b);
      else if (now - busySince.get(b)! > (opts.busyMs ?? 20_000)) await fail(`busy for over ${opts.busyMs ?? 20_000} ms: ${b}`);
    }
  }

  if (slow.length) {
    const worst = [...slow].sort((a, b) => b.ms - a.ms).slice(0, 8);
    await info.attach('frames over 200 ms', { body: worst.map((s) => `${s.ms} ms after step ${s.step}: ${s.action}`).join('\n'), contentType: 'text/plain' });
  }
  fs.writeFileSync(path.join(shots, `${tag}.json`), JSON.stringify({ project: info.project.name, seed: opts.seed, steps: opts.steps, slow }, null, 1));
  return { trail, slow };
}
