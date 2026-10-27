import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import { EMBLEM_ICONS, initialOf, lockedFrame, lockedView, viewOf } from './emblem';
import { frameOf } from '../ornament/frames';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';
import { MOCK_EMBLEM_WEBP } from './mockEmblem';
import { parseSwitches } from './switches';
import type { RecentEntry } from './types';

const DOC = `${STICK}/Documenti.nvault`;
const ARC = `${STICK}/Archivio fiscale.nvault`;
const PNG_B64 = 'iVBORw0KGgpJTUFHRUJZVEVTX3o3';

const byName = (r: RecentEntry[], n: string) => r.find((x) => x.name === n)!;

describe('emblem helpers', () => {
  it('initial, locked view, unknown icons', () => {
    expect(initialOf('  archivio fiscale')).toBe('A');
    expect(initialOf('2024 tasse')).toBe('2');
    expect(initialOf('...')).toBe('·');
    expect(lockedView({ emblem: { kind: 'icon', id: 'car' }, show_when_locked: true })).toEqual({ kind: 'icon', id: 'car' });
    expect(lockedView({})).toEqual({ kind: 'initial' });
    expect(lockedView({ emblem: null, show_when_locked: false })).toEqual({ kind: 'hidden' });
    expect(viewOf({ kind: 'icon', id: 'not-an-icon' })).toEqual({ kind: 'initial' });
    expect(EMBLEM_ICONS.length).toBe(24);
    expect(new Set(EMBLEM_ICONS.map((i) => i.id)).size).toBe(24);
  });
});

describe('emblem (mock backend)', () => {
  beforeEach(() => api.useMock(new MockBackend({ speed: 0 })));

  it('seeds an image, an icon and an initial, cached in the recent list', async () => {
    const r = await api.recentList();
    expect(byName(r, 'Documenti').emblem).toEqual({ kind: 'image', data_base64: MOCK_EMBLEM_WEBP });
    expect(byName(r, 'Archivio fiscale').emblem).toEqual({ kind: 'icon', id: 'receipt' });
    expect(byName(r, 'Vecchio').emblem).toEqual({ kind: 'initial' });
    expect(r.filter((x) => x.name !== 'Diario').every((x) => x.show_when_locked)).toBe(true);
    expect([byName(r, 'Diario').emblem, byName(r, 'Diario').show_when_locked]).toEqual([null, false]);
    const info = await api.vaultOpen(ARC, MOCK_PASSWORD);
    expect(info.emblem).toEqual({ kind: 'icon', id: 'receipt' });
    expect(await api.vaultEmblemGet(info.id)).toEqual({ emblem: { kind: 'icon', id: 'receipt' }, show_when_locked: true, emblem_frame: 'starburst' });
  });

  it('set: validates, updates the cache, opt-out drops it; locked volumes refuse', async () => {
    const info = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const st = await api.vaultEmblemSet(info.id, { kind: 'image', data_base64: `data:image/png;base64,${PNG_B64}` });
    expect(st).toEqual({ emblem: { kind: 'image', data_base64: PNG_B64 }, show_when_locked: true, emblem_frame: 'guilloche' });
    expect(byName(await api.recentList(), 'Documenti').emblem).toEqual(st.emblem);

    await expect(api.vaultEmblemSet(info.id, { kind: 'image', data_base64: btoa('GIF89a......') })).rejects.toMatchObject({ code: 'invalid_emblem' });
    await expect(api.vaultEmblemSet(info.id, { kind: 'icon', id: '../x' })).rejects.toMatchObject({ code: 'invalid_emblem' });
    const big = btoa('\x89PNG\r\n\x1a\n' + 'x'.repeat(64 * 1024));
    await expect(api.vaultEmblemSet(info.id, { kind: 'image', data_base64: big })).rejects.toMatchObject({ code: 'invalid_emblem' });

    await api.vaultEmblemSet(info.id, { kind: 'icon', id: 'house' }, false);
    let r = byName(await api.recentList(), 'Documenti');
    expect(r.emblem).toBeNull();
    expect(r.show_when_locked).toBe(false);
    expect(lockedView(r)).toEqual({ kind: 'hidden' });
    expect((await api.vaultEmblemGet(info.id)).emblem).toEqual({ kind: 'icon', id: 'house' });

    await api.vaultLock(info.id);
    await expect(api.vaultEmblemGet(info.id)).rejects.toMatchObject({ code: 'locked' });
    const again = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect([again.emblem, again.show_when_locked]).toEqual([{ kind: 'icon', id: 'house' }, false]);
    await api.vaultEmblemSet(again.id, { kind: 'icon', id: 'house' }, true);
    r = byName(await api.recentList(), 'Documenti');
    expect([r.emblem, r.show_when_locked]).toEqual([{ kind: 'icon', id: 'house' }, true]);
  });

  it('the frame round the emblem: stored with it, shown locked only for a visible look', async () => {
    const info = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(info.emblem_frame).toBe('guilloche');
    expect(lockedFrame(byName(await api.recentList(), 'Documenti'))).toBe('guilloche');
    const st = await api.vaultEmblemSet(info.id, { kind: 'initial' }, true, null, 'laurel');
    expect(st.emblem_frame).toBe('laurel');
    let r = byName(await api.recentList(), 'Documenti');
    expect([r.emblem_frame, lockedFrame(r)]).toEqual(['laurel', 'laurel']);
    await api.vaultAppearanceSet(info.id, { visibility: 'encrypted' });
    r = byName(await api.recentList(), 'Documenti');
    expect([r.emblem_frame ?? null, lockedFrame(r)]).toEqual([null, 'guilloche']);
    expect((await api.vaultAppearanceGet(info.id)).emblem_frame).toBe('laurel');
    await api.vaultAppearanceSet(info.id, { emblem_frame: 'future-frame' });
    expect((await api.vaultEmblemGet(info.id)).emblem_frame).toBe('future-frame');
    expect(frameOf('future-frame')).toBe('guilloche');
    await expect(api.vaultAppearanceSet(info.id, { emblem_frame: '../x' })).rejects.toMatchObject({ code: 'invalid_emblem' });
    expect(parseSwitches('?frame=meander').frame).toBe('meander');
    expect(parseSwitches('?frame=nope').frame).toBeNull();
  });

  it('a new volume starts with its initial', async () => {
    const info = await api.vaultCreate(`${STICK}/Nuovo.nvault`, 'pw-lunga-123');
    expect([info.emblem, info.show_when_locked]).toEqual([{ kind: 'initial' }, true]);
    expect(byName(await api.recentList(), 'Nuovo').emblem).toEqual({ kind: 'initial' });
  });

  it('?emblem= switch forces every seeded volume', async () => {
    for (const [mode, kind] of [['initial', 'initial'], ['icon', 'icon'], ['image', 'image']] as const) {
      const m = new MockBackend({ speed: 0, emblem: mode, appearance: 'visible' });
      const r = (await m.invoke('recent_list')) as RecentEntry[];
      expect(r.map((x) => x.emblem?.kind)).toEqual([kind, kind, kind, kind]);
    }
    const hidden = (await new MockBackend({ speed: 0, emblem: 'hidden' }).invoke('recent_list')) as RecentEntry[];
    expect(hidden.map(lockedView)).toEqual([{ kind: 'hidden' }, { kind: 'hidden' }, { kind: 'hidden' }, { kind: 'hidden' }]);
    expect(parseSwitches('?emblem=icon&accent=ottanio')).toMatchObject({ emblem: 'icon', accent: 'ottanio' });
    expect(parseSwitches('?emblem=nope&accent=blu')).toMatchObject({ emblem: null, accent: null });
  });
});
