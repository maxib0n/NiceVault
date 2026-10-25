import { describe, expect, it } from 'vitest';
import { worn } from './worn';

const app = { theme: 'light' as const, accent: 'ottanio' };

describe('worn look: open volume > visible locked look > the app', () => {
  it('locked screens with no volume look wear the app look', () => {
    expect(worn(null, null, app)).toEqual({ theme: 'light', accent: 'ottanio' });
  });
  it('Sblocca of a visible look: its ink, the app theme (the header has no theme)', () => {
    expect(worn(null, { accent: 'prussia', show_when_locked: true }, app)).toEqual({ theme: 'light', accent: 'prussia' });
  });
  it('Sblocca of an encrypted look: the app look', () => {
    expect(worn(null, { accent: 'prussia', show_when_locked: false }, app)).toEqual({ theme: 'light', accent: 'ottanio' });
    expect(worn(null, { accent: null, show_when_locked: true }, app)).toEqual({ theme: 'light', accent: 'ottanio' });
  });
  it('an open volume wears its own settings, whatever the app look', () => {
    expect(worn({ theme: 'dark', accent: 'seppia' }, { accent: 'prussia', show_when_locked: true }, app)).toEqual({ theme: 'dark', accent: 'seppia' });
    expect(worn({ theme: 'system', accent: 'velluto' }, null, app)).toEqual({ theme: 'system', accent: 'velluto' });
  });
  it('unknown inks fall back: the volume to the app ink, the app to velluto', () => {
    expect(worn({ theme: 'dark', accent: 'nope' }, null, app).accent).toBe('ottanio');
    expect(worn(null, null, { theme: 'dark', accent: 'nope' }).accent).toBe('velluto');
  });
});
