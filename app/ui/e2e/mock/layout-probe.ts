import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';

export interface LayoutFinding {
  kind: 'overlap' | 'clip' | 'offscreen' | 'squeeze';
  a: string;
  b?: string;
  px: number;
}

function probeLayout() {
  const SKIP = ".breadcrumb[data-overflow='true'] button.crumb, .pm .mask, .detail .mask, svg, canvas, [data-decor], .microline, .ghost, .perforated, .fibres, .mrz-ghost, .sr-only, .visually-hidden, [hidden], dialog:not([open])";
  const name = (el) => {
    const t = (el.textContent || el.getAttribute('aria-label') || el.placeholder || '').trim().replace(/\s+/g, ' ').slice(0, 28);
    return `${el.tagName.toLowerCase()}${el.classList.length ? '.' + [...el.classList].filter((c) => !c.startsWith('s-')).slice(0, 2).join('.') : ''} "${t}"`;
  };
  const visible = (el) => {
    for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
      const cs = getComputedStyle(e);
      if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) < 0.05) return false;
    }
    return true;
  };
  const boxes = [];
  const squeezed = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!n.nodeValue.trim()) continue;
    const el = n.parentElement;
    if (!el || el.closest(SKIP) || !visible(el)) continue;
    const eb = el.getBoundingClientRect();
    if (eb.width <= 1 || eb.height <= 1) continue;
    const range = document.createRange();
    range.selectNodeContents(n);
    const rects = [...range.getClientRects()].filter((r) => r.width > 1 && r.height > 1);
    for (const r of rects) boxes.push({ el, r });
    const fs = parseFloat(getComputedStyle(el).fontSize);
    if (rects.length >= 4 && Math.max(...rects.map((r) => r.width)) < fs * 2.5) squeezed.push(el);
  }
  for (const el of document.querySelectorAll('input:not([type=hidden]):not([type=checkbox]):not([type=radio]):not([type=range]), textarea, select')) {
    if (el.closest(SKIP) || !visible(el)) continue;
    const r = el.getBoundingClientRect();
    if (r.width > 1 && r.height > 1) boxes.push({ el, r, field: true });
  }
  for (const b of boxes) {
    let { left, top, right, bottom } = b.r;
    for (let a = b.el; a && a !== document.body; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.overflowX === 'visible' && cs.overflowY === 'visible') continue;
      const c = a.getBoundingClientRect();
      if (cs.overflowX !== 'visible') (left = Math.max(left, c.left)), (right = Math.min(right, c.right));
      if (cs.overflowY !== 'visible') (top = Math.max(top, c.top)), (bottom = Math.min(bottom, c.bottom));
    }
    b.v = { left, top, right, bottom, height: bottom - top };
  }
  const out = [];
  const seen = new Set();
  const add = (f) => {
    const k = f.kind + f.a + (f.b || '');
    if (!seen.has(k)) {
      seen.add(k);
      out.push(f);
    }
  };
  const layer = (el) => {
    for (let e = el; e && e !== document.body; e = e.parentElement) if (e.matches('dialog[open], [popover]') || /fixed|sticky/.test(getComputedStyle(e).position)) return e;
    return document.body;
  };
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const A = boxes[i], B = boxes[j];
      if (A.el === B.el || A.el.contains(B.el) || B.el.contains(A.el) || layer(A.el) !== layer(B.el)) continue;
      if ((A.field && A.el.parentElement.contains(B.el)) || (B.field && B.el.parentElement.contains(A.el))) continue;
      const x = Math.min(A.v.right, B.v.right) - Math.max(A.v.left, B.v.left);
      const y = Math.min(A.v.bottom, B.v.bottom) - Math.max(A.v.top, B.v.top);
      if (x > 2 && y > Math.min(4, Math.min(A.v.height, B.v.height) * 0.3)) add({ kind: 'overlap', a: name(A.el), b: name(B.el), px: Math.round(Math.min(x, y)) });
    }
  }
  for (const el of squeezed) add({ kind: 'squeeze', a: name(el), px: 0 });
  const vw = innerWidth, vh = innerHeight;
  for (const { el, r: raw, v } of boxes) {
    const r = raw;
    let cut = 0;
    for (let a = el; a && a !== document.body; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.textOverflow === 'ellipsis' || cs.webkitLineClamp !== 'none' && cs.webkitLineClamp) break;
      const ox = cs.overflowX, oy = cs.overflowY;
      if (/auto|scroll/.test(ox + oy)) break;
      if (/hidden|clip/.test(ox) || /hidden|clip/.test(oy)) {
        const b = a.getBoundingClientRect();
        if (/hidden|clip/.test(ox)) cut = Math.max(cut, b.left - r.left, r.right - b.right);
        if (/hidden|clip/.test(oy)) cut = Math.max(cut, b.top - r.top, r.bottom - b.bottom);
        if (cut > 2) break;
      }
    }
    if (cut > 2) add({ kind: 'clip', a: name(el), px: Math.round(cut) });
    else {
      const scrolls = (axis) => {
        for (let a = el.parentElement; a; a = a.parentElement) if (/auto|scroll/.test(getComputedStyle(a)[axis])) return true;
        return false;
      };
      const px = Math.max(v.right - vw, -v.left);
      if (px > 1 && !scrolls('overflowX')) add({ kind: 'offscreen', a: name(el), px: Math.round(px) });
      else if (v.top > vh && !scrolls('overflowY')) add({ kind: 'offscreen', a: name(el), px: Math.round(v.top - vh) });
    }
  }
  return out;
}

export async function probe(page: Page): Promise<LayoutFinding[]> {
  return page.evaluate(`(() => { ${probeSource()}; return probeLayout(); })()`) as Promise<LayoutFinding[]>;
}

function probeSource(): string {
  return readFileSync(new URL(import.meta.url), 'utf8').match(/\/\/ probe-begin\n([\s\S]*?)\/\/ probe-end/)![1];
}
