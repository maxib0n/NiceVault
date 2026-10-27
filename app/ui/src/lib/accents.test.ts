import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { ACCENTS, DEFAULT_ACCENT, applyAccent, isAccent } from './accents';
import { MockBackend } from './mock';

const read = (f: string) => readFileSync(resolve(__dirname, '../styles', f), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
type Vars = Record<string, string>;
type Theme = 'light' | 'dark' | 'forced';

function rules(css: string): [string, Vars][] {
  return [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => [
    m[1].replace(/@media[^{]*$/, '').trim(),
    Object.fromEntries([...m[2].matchAll(/--([\w-]+):\s*([^;]+);/g)].map((x) => [x[1], x[2].trim().replace(/\s+/g, ' ')])),
  ]);
}
const themeOf = (sel: string): Theme => (sel.includes(":not([data-theme='light'])") ? 'dark' : sel.includes("data-theme='dark'") ? 'forced' : 'light');

const tokens = rules(read('tokens.css'));
const merge = (pred: (s: string) => boolean) => Object.assign({}, ...tokens.filter(([s]) => pred(s)).map(([, v]) => v)) as Vars;
const LIGHT_BASE = merge((s) => s === ':root');
const BASE: Record<Theme, Vars> = {
  light: LIGHT_BASE,
  dark: { ...LIGHT_BASE, ...merge((s) => s === ":root:not([data-theme='light'])") },
  forced: { ...LIGHT_BASE, ...merge((s) => s === ":root[data-theme='dark']") },
};

const sheet = { accent: {}, surface: {} } as Record<'accent' | 'surface', Record<Theme, Record<string, Vars>>>;
for (const k of ['accent', 'surface'] as const) sheet[k] = { light: {}, dark: {}, forced: {} };
for (const [sel, vars] of rules(read('accents.css'))) {
  const kind = 'accent' in vars ? 'accent' : 'surface';
  const id = sel.match(/data-accent='(\w+)'/)?.[1] ?? DEFAULT_ACCENT;
  const t = themeOf(sel);
  sheet[kind][t][id] = { ...sheet[kind][t][id], ...vars };
}

function palette(theme: Theme, id: string): Vars {
  const p = { ...BASE[theme], ...sheet.accent.light[id], ...sheet.surface.light[id] };
  if (theme !== 'light') Object.assign(p, sheet.accent[theme][id], sheet.surface[theme][id]);
  const get = (v: string, depth = 0): string => {
    const m = v.match(/^var\(--([\w-]+)\)$/);
    return m && depth < 8 ? get(p[m[1]] ?? v, depth + 1) : v;
  };
  return Object.fromEntries(Object.entries(p).map(([k, v]) => [k, get(v)]));
}

function lum(hex: string): number {
  expect(hex, 'a plain #rrggbb').toMatch(/^#[0-9a-f]{6}$/i);
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}
const ratio = (a: string, b: string) => {
  const [x, y] = [lum(a), lum(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
};
function oklab(hex: string): [number, number, number] {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
}
const deltaE = (a: string, b: string) => Math.hypot(...oklab(a).map((v, i) => v - oklab(b)[i]));

const ACCENT_KEYS = ['accent', 'accent-deep', 'accent-ink', 'accent-line', 'accent-soft', 'foil', 'foil-hi', 'foil-lo', 'foil-mid'];
const DARK_SURFACES = ['ground', 'ground-hi', 'ground-lo', 'paper', 'paper-hi', 'paper-lo', 'paper-2', 'paper-sunk', 'paper-edge', 'paper-rule', 'paper-rule-2',
  'ink', 'ink-2', 'ink-3', 'ink-4', 'field', 'field-focus', 'field-edge', 'key-hi', 'key-lo', 'key-press', 'key-dead', 'stage', 'print-develop',
  'env-paper', 'env-paper-hi', 'env-paper-lo', 'env-paper-2', 'file-paper', 'file-fold', 'file-ink', 'lining-base', 'lining-base-hi', 'lining-base-lo'];
const LIGHT_SURFACES = ['ground', 'ground-hi', 'ground-lo', 'paper', 'paper-hi', 'paper-lo', 'paper-2', 'paper-sunk', 'paper-rule', 'field', 'key-hi', 'key-lo'];
const others = ACCENTS.map((a) => a.id).filter((id) => id !== DEFAULT_ACCENT);

describe('ink presets', () => {
  it('every preset defines its accent and foil for light, dark by OS and forced dark; forced = by OS', () => {
    for (const { id } of ACCENTS) {
      for (const t of ['light', 'dark', 'forced'] as const) expect(Object.keys(sheet.accent[t][id] ?? {}).sort(), `${t}/${id}`).toEqual(ACCENT_KEYS);
      expect(sheet.accent.forced[id]).toEqual(sheet.accent.dark[id]);
    }
    expect(Object.keys(sheet.accent.light).sort()).toEqual(ACCENTS.map((a) => a.id).sort());
  });

  it('every ink but velluto is a whole palette: its own board, card, levels, hairlines and inks', () => {
    expect(sheet.surface.light[DEFAULT_ACCENT]).toBeUndefined();
    expect(sheet.surface.dark[DEFAULT_ACCENT]).toBeUndefined();
    const velluto = palette('dark', DEFAULT_ACCENT);
    for (const id of others) {
      const dark = sheet.surface.dark[id];
      expect(Object.keys(dark).sort(), id).toEqual([...DARK_SURFACES].sort());
      expect(sheet.surface.forced[id], `${id} forced = by OS`).toEqual(dark);
      for (const k of Object.keys(sheet.surface.light[id])) expect(dark, `${id} dark ${k}`).toHaveProperty(k);
      for (const k of LIGHT_SURFACES) expect(sheet.surface.light[id], `${id} light ${k}`).toHaveProperty(k);
      for (const k of DARK_SURFACES) if (k !== 'ink' && !k.startsWith('ink-')) expect(dark[k], `${id} ${k} is not anthracite`).not.toBe(velluto[k]);
      expect(lum(dark.ground), `${id} board`).toBeLessThan(0.03);
      expect(lum(dark.paper)).toBeLessThan(lum(dark.ground));
    }
  });

  it('the family keeps apart: every dark board is its own colour', () => {
    const boards = ACCENTS.map((a) => palette('dark', a.id).ground);
    for (let i = 0; i < boards.length; i++)
      for (let j = i + 1; j < boards.length; j++) expect(deltaE(boards[i], boards[j]), `${ACCENTS[i].id} vs ${ACCENTS[j].id}`).toBeGreaterThan(0.012);
  });

  for (const theme of ['light', 'dark'] as const) {
    for (const { id } of ACCENTS) {
      it(`${id} (${theme}) meets WCAG`, () => {
        const p = palette(theme, id);
        const card = [p.paper, p['paper-hi'], p['paper-2']];
        const board = [p.ground, p['ground-hi'], p['ground-lo']];
        const all = [...card, p['paper-sunk'], ...board, ...(theme === 'dark' ? [p['lining-base']] : [])];
        for (const c of card) {
          expect(ratio(p.ink, c), `body ink on card ${c}`).toBeGreaterThanOrEqual(7);
          expect(ratio(p['ink-2'], c), `ink-2 on card ${c}`).toBeGreaterThanOrEqual(7);
          expect(ratio(p['ink-4'], c), `quiet ink-4 on card ${c}`).toBeGreaterThanOrEqual(3);
        }
        for (const c of all) {
          expect(ratio(p['ink-3'], c), `hint ink-3 on ${c}`).toBeGreaterThanOrEqual(4.5);
          expect(ratio(p.accent, c), `accent text on ${c}`).toBeGreaterThanOrEqual(4.5);
          expect(ratio(p.danger, c), `danger on ${c}`).toBeGreaterThanOrEqual(4.5);
        }
        expect(ratio(p.danger, p.field), 'danger in a field').toBeGreaterThanOrEqual(4.5);
        expect(ratio(p['field-edge'], p.field), 'field edge on field').toBeGreaterThanOrEqual(3);
        expect(ratio(p['field-edge'], p.paper), 'field edge on card').toBeGreaterThanOrEqual(3);
        for (const c of [p.paper, p.field, p['paper-sunk']]) expect(ratio(p.accent, c), `focus ring on ${c}`).toBeGreaterThanOrEqual(3);
        expect(ratio(p['accent-ink'], p.accent), 'on-accent ink').toBeGreaterThanOrEqual(4.5);
        expect(ratio(p.accent, p['accent-soft']), 'accent on soft').toBeGreaterThanOrEqual(4.5);
        expect(ratio(p.ink, p['accent-soft']), 'body ink on soft').toBeGreaterThanOrEqual(4.5);
        expect(ratio(p['accent-line'], p.paper), 'line on card (UI)').toBeGreaterThanOrEqual(3);
        const keyInk = theme === 'dark' ? '#fff6f2' : p['accent-ink'];
        expect(ratio(keyInk, p['accent-deep']), 'key ink on deep').toBeGreaterThanOrEqual(4.5);
        if (theme === 'dark') expect(ratio(p.foil, p['lining-base']), 'foil on the lining').toBeGreaterThanOrEqual(4.5);
        expect(ratio(p.foil, p['accent-deep']), 'foil on the dyed cover').toBeGreaterThanOrEqual(3);
        const velluto = palette(theme, DEFAULT_ACCENT).accent;
        expect(deltaE(p.danger, velluto), 'danger apart from velluto').toBeGreaterThanOrEqual(0.05);
      });
    }
  }

  it('has no electric blue', () => {
    for (const t of ['light', 'dark'] as const)
      for (const v of Object.values(sheet.accent[t])) {
        const [r, g, b] = [1, 3, 5].map((i) => parseInt(v.accent.slice(i, i + 2), 16));
        expect(b > 200 && r < 60 && g < 110, v.accent).toBe(false);
      }
  });
});

describe('accent setting', () => {
  it('applyAccent sets data-accent, unknown ids fall back to the default', () => {
    const root = { dataset: {} as DOMStringMap };
    applyAccent('ottanio', root);
    expect(root.dataset.accent).toBe('ottanio');
    applyAccent('#0047ff', root);
    expect(root.dataset.accent).toBe(DEFAULT_ACCENT);
    expect(isAccent('velluto')).toBe(true);
    expect(isAccent('bordeaux')).toBe(false);
  });

  it('mock settings: default velluto, set, unknown ignored', async () => {
    const m = new MockBackend({ speed: 0 });
    expect(((await m.invoke('settings_get')) as { accent: string }).accent).toBe('velluto');
    expect(((await m.invoke('settings_set', { settings: { accent: 'seppia' } })) as { accent: string }).accent).toBe('seppia');
    const s = (await m.invoke('settings_set', { settings: { accent: 'nope', auto_lock_minutes: 5 } })) as { accent: string; auto_lock_minutes: number };
    expect(s).toMatchObject({ accent: 'seppia', auto_lock_minutes: 5, theme: 'dark' });
  });
});
