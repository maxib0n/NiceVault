import { readFileSync, writeFileSync } from 'node:fs';
import { circle, loops, petals, rays, ring } from '../src/ornament/lathe.ts';

const W = 200;
const cx = W / 2;
const cy = W / 2;
const R = W / 2 - 1;
const STEP = 1.1;

class Rec {
  d = '';
  x = 0;
  y = 0;
  sx = 0;
  sy = 0;
  pending = null;
  moveTo(x, y) {
    this.flush();
    this.d += `M${f(x)} ${f(y)}`;
    this.x = this.sx = r1(x);
    this.y = this.sy = r1(y);
  }
  lineTo(x, y) {
    if (Math.hypot(x - this.x, y - this.y) < STEP) {
      this.pending = [x, y];
      return;
    }
    this.pending = null;
    this.rel(x, y);
  }
  rel(x, y) {
    const nx = r1(x);
    const ny = r1(y);
    this.d += `l${f(nx - this.x)} ${f(ny - this.y)}`.replace(/ -/g, '-');
    this.x = nx;
    this.y = ny;
  }
  flush() {
    if (this.pending) this.rel(...this.pending);
    this.pending = null;
  }
  arc(x, y, r) {
    this.flush();
    this.d += `A${f(r)} ${f(r)} 0 1 1 ${f(x - r)} ${f(y)}A${f(r)} ${f(r)} 0 1 1 ${f(x + r)} ${f(y)}`;
    this.x = r1(x + r);
    this.y = r1(y);
  }
  closePath() {
    this.flush();
    this.d += 'z';
    this.x = this.sx;
    this.y = this.sy;
  }
}
const r1 = (v) => Math.round(v * 10) / 10;
const f = (v) => String(r1(v)).replace(/^(-?)0\./, '$1.');

const p = new Rec();
const fine = new Rec();
circle(p, cx, cy, R * 0.996);
circle(fine, cx, cy, R * 0.981);
ring(fine, cx, cy, R, { rm: 0.951, lines: 3, k: 84, a: 0.016 });
circle(fine, cx, cy, R * 0.921);
rays(fine, cx, cy, R, 0.876, 0.914, Math.round(R * 1.55));
circle(p, cx, cy, R * 0.872);
circle(fine, cx, cy, R * 0.598);
circle(fine, cx, cy, R * 0.586);
circle(p, cx, cy, R * 0.388);
ring(fine, cx, cy, R, { rm: 0.374, lines: 2, k: 64, a: 0.0055 });
circle(fine, cx, cy, R * 0.355);
const Rm = R * 0.27;
const mark = new Rec();
circle(mark, cx, cy, Rm * 0.99);
circle(mark, cx, cy, Rm * 0.3);
petals(mark, cx, cy, Rm, { r0: 0.36, r1: 0.94, lines: 3, k: 8, lean: 0.8 });
const a = new Rec();
loops(a, cx, cy, R, { rc: 0.733, a: 0.072, k: 30, lines: 3, squash: 1.12 });
const b = new Rec();
petals(b, cx, cy, R, { r0: 0.405, r1: 0.572, lines: 5, k: 15, lean: 1.2 });
for (const r of [p, fine, mark, a, b]) r.flush();

const svg = `<svg class="nv-splash-seal" viewBox="0 0 ${W} ${W}" fill="none" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <defs>
          <linearGradient id="nv-seal-ink" class="nv-light" gradientUnits="userSpaceOnUse" x1="10" y1="10" x2="190" y2="190"><stop offset="0" /><stop offset=".125" /><stop offset=".25" /><stop offset=".375" /><stop offset=".5" /><stop offset=".625" /><stop offset=".75" /><stop offset=".875" /><stop offset="1" /></linearGradient>
          <filter id="nv-seal-relief" x="-2%" y="-2%" width="104%" height="104%">
            <feOffset in="SourceAlpha" dx="-.45" dy="-.55" result="a" />
            <feFlood class="nv-relief-lo" />
            <feComposite in2="a" operator="in" result="lo" />
            <feOffset in="SourceAlpha" dx=".5" dy=".62" result="b" />
            <feFlood class="nv-relief-hi" />
            <feComposite in2="b" operator="in" result="hi" />
            <feMerge><feMergeNode in="lo" /><feMergeNode in="hi" /><feMergeNode in="SourceGraphic" /></feMerge>
          </filter>
        </defs>
        <g class="nv-seal-print" filter="url(#nv-seal-relief)" stroke="url(#nv-seal-ink)">
          <path class="nv-plate nv-heavy" d="${p.d}" stroke-width=".72" />
          <path class="nv-plate nv-fine" d="${fine.d}" stroke-width=".5" />
          <path class="nv-plate nv-mark" d="${mark.d}" stroke-width=".6" />
          <path class="nv-lathe-a" d="${a.d}" stroke-width=".52" />
          <path class="nv-lathe-b" d="${b.d}" stroke-width=".52" />
        </g>
      </svg>`;

const file = new URL('../index.html', import.meta.url);
const html = readFileSync(file, 'utf8');
const begin = '<!-- nv-seal-begin (scripts/gen-splash.mjs) -->';
const end = '<!-- nv-seal-end -->';
const i = html.indexOf(begin);
const j = html.indexOf(end);
if (i < 0 || j < i) throw new Error('index.html: the nv-seal markers are missing');
writeFileSync(file, html.slice(0, i + begin.length) + '\n      ' + svg + '\n      ' + html.slice(j));
console.log(`seal: ${svg.length} bytes`);

const accents = readFileSync(new URL('../src/styles/accents.css', import.meta.url), 'utf8');
function block(sel, from = 0) {
  const i = accents.indexOf(sel, from);
  if (i < 0) throw new Error(`accents.css: ${sel} not found`);
  const body = accents.slice(accents.indexOf('{', i) + 1, accents.indexOf('}', i));
  return (name) => {
    const m = body.match(new RegExp(`${name}:\\s*([^;]+);`));
    if (!m) throw new Error(`accents.css: ${name} missing under ${sel}`);
    return m[1].trim();
  };
}
const ids = [...accents.matchAll(/^\[data-accent='(\w+)'\] \{/gm)].map((m) => m[1]);
ids.shift();
const darkSurfaces = accents.indexOf('/* surfaces, dark forced');
let light = '';
let dark = '';
for (const id of ids) {
  const ink = block(`[data-accent='${id}'] {`);
  const inkD = block(`:root[data-theme='dark'][data-accent='${id}'],`);
  const g = block(`:root[data-accent='${id}'] {`);
  const gD = block(`:root[data-theme='dark'][data-accent='${id}'] {`, darkSurfaces);
  const decl = (ground, line) =>
    `  --sp-ground: ${ground('--ground')};\n  --sp-ground-hi: ${ground('--ground-hi')};\n  --sp-ground-lo: ${ground('--ground-lo')};\n  --sp-ink: ${line('--accent')};\n`;
  light += `.nv-splash[data-accent='${id}'] {\n${decl(g, ink)}}\n`;
  const d = decl(gD, inkD);
  dark += `@media (prefers-color-scheme: dark) {\n  .nv-splash:not([data-theme='light'])[data-accent='${id}'] {\n${d.replace(/^/gm, '  ')}  }\n}\n.nv-splash[data-theme='dark'][data-accent='${id}'] {\n${d}}\n`;
}
const cssFile = new URL('../public/splash.css', import.meta.url);
const css = readFileSync(cssFile, 'utf8');
const cb = '/* nv-inks-begin (scripts/gen-splash.mjs) */';
const ce = '/* nv-inks-end */';
const ci = css.indexOf(cb);
const cj = css.indexOf(ce);
if (ci < 0 || cj < ci) throw new Error('splash.css: the nv-inks markers are missing');
writeFileSync(cssFile, css.slice(0, ci + cb.length) + '\n' + light + dark + css.slice(cj));
console.log(`inks: ${ids.join(', ')}`);
