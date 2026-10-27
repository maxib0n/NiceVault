import { afterEach, describe, expect, it } from 'vitest';
import { i18n, intlTag, systemLocale } from './i18n.svelte';
import { errorText } from './errors';
import { formatAgo, formatBytes, formatDate, itemCount } from './format';
import { humanDuration, scoreLabel } from './strength';
import { lockReasonText } from './stores/app.svelte';
import { m } from '../paraglide/messages.js';
import itCat from '../../messages/it.json';
import enCat from '../../messages/en.json';

afterEach(() => {
  i18n.choice = 'it';
});

describe('i18n', () => {
  it('maps the OS languages to a UI language', () => {
    expect(systemLocale(['it-IT', 'en-US'])).toBe('it');
    expect(systemLocale(['de-DE', 'it'])).toBe('it');
    expect(systemLocale(['en-GB'])).toBe('en');
    expect(systemLocale(['fr-FR'])).toBe('en');
    expect(intlTag('en', ['en-US'])).toBe('en-US');
    expect(intlTag('en', ['it-IT'])).toBe('en-GB');
    expect(intlTag('it', [])).toBe('it-IT');
  });

  it('switches every message, number and plural at once', () => {
    i18n.choice = 'it';
    expect(m.welcome_new()).toBe('Nuovo volume');
    expect(itemCount(1)).toBe('1 elemento');
    expect(itemCount(12345)).toBe('12.345 elementi');
    expect(formatBytes(1536)).toBe('1,5 KB');
    expect(humanDuration(3 * 3600)).toBe('3 ore');
    expect(scoreLabel(4)).toBe('Ottima');
    expect(lockReasonText('removed')).toBe('Unità scollegata');
    i18n.choice = 'en';
    expect(m.welcome_new()).toBe('New volume');
    expect(itemCount(1)).toBe('1 item');
    expect(itemCount(1234)).toBe('1,234 items');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(humanDuration(1)).toBe('1 second');
    expect(humanDuration(1e12)).toBe('centuries');
    expect(scoreLabel(0)).toBe('Very weak');
    expect(lockReasonText('removed')).toBe('Drive disconnected');
    expect(lockReasonText('user')).toBeNull();
  });

  it('dates and relative times follow the language', () => {
    const now = new Date(2026, 9, 24, 15, 0);
    i18n.choice = 'en';
    expect(formatDate(new Date(2026, 9, 24, 9, 5), now)).toMatch(/^today /);
    expect(formatAgo(now.getTime() - 2 * 3600_000, now.getTime())).toBe('2 hours ago');
    expect(formatAgo(now.getTime() - 86_400_000, now.getTime())).toBe('yesterday');
    i18n.choice = 'it';
    expect(formatAgo(now.getTime() - 60_000, now.getTime())).toBe('1 minuto fa');
  });

  it('translates backend error codes; unknown codes keep the backend message', () => {
    i18n.choice = 'en';
    expect(errorText({ code: 'wrong_password', message: 'whatever' })).toBe('Wrong password.');
    expect(errorText({ code: 'duress_conflict', message: 'x' })).toBe('It must differ from every other password of the volume.');
    expect(errorText({ code: 'brand_new', message: 'Backend says so.' })).toBe('Backend says so.');
    i18n.choice = 'it';
    expect(errorText({ code: 'io', message: 'disk full' })).toBe('Errore di lettura o scrittura sul disco.');
  });

  it('both catalogs have the same keys and the same parameters', () => {
    const keys = (o: object) => Object.keys(o).filter((k) => k !== '$schema').sort();
    expect(keys(itCat)).toEqual(keys(enCat));
    const params = (v: unknown) => [...JSON.stringify(v).matchAll(/\{(\w+)\}/g)].map((x) => x[1]).sort().join();
    for (const k of keys(itCat)) expect(params((enCat as Record<string, unknown>)[k]), k).toBe(params((itCat as Record<string, unknown>)[k]));
    for (const k of keys(enCat)) expect(JSON.stringify((enCat as Record<string, unknown>)[k])).not.toMatch(/""|\b(della|questo|volume è)\b/);
  });
});

describe('dates read in the UI language (formatWhen)', () => {
  afterEach(() => {
    i18n.choice = 'it';
  });
  const now = new Date(2026, 9, 24, 20, 30).getTime();
  const at = (d: number, h: number, min = 0) => new Date(2026, 9, 24 - d, h, min).getTime();
  it('Italian: relative where it reads better, then the date', async () => {
    const { formatWhen } = await import('./format');
    i18n.choice = 'it';
    expect(formatWhen(now - 20_000, now)).toBe('adesso');
    expect(formatWhen(now - 5 * 60_000, now)).toBe('5 minuti fa');
    expect(formatWhen(at(0, 8, 23), now)).toBe('oggi 08:23');
    expect(formatWhen(at(1, 9, 10), now)).toBe('ieri 09:10');
    expect(formatWhen(at(3, 12), now)).toBe('3 giorni fa');
    expect(formatWhen(at(30, 12), now)).toBe('24 set 2026');
    expect(m.pm_created({ date: formatWhen(at(0, 18, 23), now) })).toBe('Creata oggi 18:23');
  });
  it('English: the same moments', async () => {
    const { formatWhen } = await import('./format');
    i18n.choice = 'en';
    expect(formatWhen(now - 20_000, now)).toBe('just now');
    expect(formatWhen(at(1, 9, 10), now)).toMatch(/^yesterday 09:10/);
    expect(formatWhen(at(30, 12), now)).toMatch(/24 Sept? 2026|Sep 24, 2026/);
  });
});
