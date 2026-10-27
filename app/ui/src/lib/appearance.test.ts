import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import { lockedAccent, lockedView } from './emblem';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';
import { MOCK_EMBLEM_WEBP } from './mockEmblem';
import { parseSwitches } from './switches';
import type { RecentEntry, Settings } from './types';

const DOC = `${STICK}/Documenti.nvault`;
const ARC = `${STICK}/Archivio fiscale.nvault`;
const DIA = `${STICK}/Diario.nvault`;
const PNG_B64 = 'iVBORw0KGgpJTUFHRUJZVEVTX3o3';

const byName = (r: RecentEntry[], n: string) => r.find((x) => x.name === n)!;

describe('appearance (mock backend)', () => {
  beforeEach(() => {
    api.useMock(new MockBackend({ speed: 0 }));
    api.bindActiveVault(() => null);
  });

  it('seeds visible and encrypted volumes; locked screens read accent + emblem from the header', async () => {
    const r = await api.recentList();
    expect([byName(r, 'Documenti').accent, byName(r, 'Archivio fiscale').accent, byName(r, 'Diario').accent]).toEqual(['velluto', 'prussia', null]);
    expect(lockedAccent(byName(r, 'Diario'))).toBeNull();
    expect(lockedView(byName(r, 'Diario'))).toEqual({ kind: 'hidden' });
    expect(lockedAccent(byName(r, 'Archivio fiscale'))).toBe('prussia');
    const info = await api.vaultOpen(DIA, MOCK_PASSWORD);
    expect([info.visibility, info.show_when_locked, info.settings?.accent, info.emblem]).toEqual(['encrypted', false, 'bottiglia', { kind: 'icon', id: 'book-open' }]);
    expect(await api.vaultAppearanceGet(info.id)).toEqual({ accent: 'bottiglia', emblem: { kind: 'icon', id: 'book-open' }, emblem_frame: 'guilloche', visibility: 'encrypted', tampered: false });
  });

  it('?appearance= forces every seeded volume', async () => {
    const vis = (await new MockBackend({ speed: 0, appearance: 'visible' }).invoke('recent_list')) as RecentEntry[];
    expect(vis.every((x) => x.show_when_locked)).toBe(true);
    expect(byName(vis, 'Diario').accent).toBe('bottiglia');
    const enc = (await new MockBackend({ speed: 0, appearance: 'encrypted' }).invoke('recent_list')) as RecentEntry[];
    expect(enc.map((x) => [x.show_when_locked, x.emblem ?? null, x.accent ?? null])).toEqual(enc.map(() => [false, null, null]));
    expect(parseSwitches('?appearance=encrypted').appearance).toBe('encrypted');
    expect(parseSwitches('?appearance=boh').appearance).toBeNull();
  });

  it('visibility toggle and the header copy of an image', async () => {
    const info = await api.vaultOpen(DOC, MOCK_PASSWORD);
    let a = await api.vaultAppearanceSet(info.id, { visibility: 'encrypted' });
    expect(a.visibility).toBe('encrypted');
    let r = byName(await api.recentList(), 'Documenti');
    expect([r.show_when_locked, r.emblem, r.accent]).toEqual([false, null, null]);
    a = await api.vaultAppearanceSet(info.id, { visibility: 'visible', accent: 'ottanio' });
    expect([a.visibility, a.accent]).toEqual(['visible', 'ottanio']);
    r = byName(await api.recentList(), 'Documenti');
    expect([r.accent, r.emblem]).toEqual(['ottanio', { kind: 'image', data_base64: MOCK_EMBLEM_WEBP }]);
    expect((await api.settingsGet(info.id)).accent).toBe('ottanio');

    await api.vaultEmblemSet(info.id, { kind: 'image', data_base64: MOCK_EMBLEM_WEBP }, true, PNG_B64);
    expect(byName(await api.recentList(), 'Documenti').emblem).toEqual({ kind: 'image', data_base64: PNG_B64 });
    expect((await api.vaultEmblemGet(info.id)).emblem).toEqual({ kind: 'image', data_base64: MOCK_EMBLEM_WEBP });
    expect((await api.vaultAppearanceGet(info.id)).visibility).toBe('visible');
    await api.vaultEmblemSet(info.id, { kind: 'image', data_base64: MOCK_EMBLEM_WEBP }, false);
    expect((await api.vaultAppearanceGet(info.id)).visibility).toBe('encrypted');
    const big = btoa('\x89PNG\r\n\x1a\n' + 'x'.repeat(25 * 1024));
    await expect(api.vaultEmblemSet(info.id, { kind: 'icon', id: 'house' }, true, big)).rejects.toMatchObject({ code: 'invalid_emblem' });
    await expect(api.vaultAppearanceSet(info.id, { visibility: 'boh' as never })).rejects.toMatchObject({ code: 'invalid_appearance' });
  });

  it('video_arrows: seek by default, step when chosen, junk ignored', async () => {
    expect((await api.appSettingsGet()).video_arrows).toBe('seek');
    expect((await api.appSettingsSet({ video_arrows: 'step' })).video_arrows).toBe('step');
    expect((await api.appSettingsSet({ video_arrows: 'nonsense' as never })).video_arrows).toBe('step');
    expect((await api.appSettingsGet()).video_arrows).toBe('step');
    expect((await api.appSettingsSet({ video_arrows: 'seek' })).video_arrows).toBe('seek');
  });

  it('settings are per volume; no vault = the active one (bindActiveVault), none open = session defaults', async () => {
    const d = await api.settingsGet();
    expect(d).toEqual({ auto_lock_minutes: 15, accent: 'velluto', theme: 'dark', lock_on_sleep: true, lock_on_screen_lock: true, lock_scope: 'volume', grid_size: 'm', list_density: 'normal', browser_enabled: true });
    expect((await api.settingsSet({ accent: 'seppia' })).accent).toBe('seppia');
    await api.appSettingsSet({ app_accent: 'ottanio', app_theme: 'light' });
    const like = await api.vaultCreate(`${STICK}/Come app.nvault`, 'pw-lunga-123');
    expect([like.settings?.accent, like.settings?.theme]).toEqual(['ottanio', 'light']);
    await api.vaultLock(like.id);
    await api.appSettingsSet({ new_like_app: false });
    const created = await api.vaultCreate(`${STICK}/Nuovo.nvault`, 'pw-lunga-123');
    expect(created.settings?.accent).toBe('seppia');

    const doc = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const arc = await api.vaultOpen(ARC, MOCK_PASSWORD);
    expect((await api.settingsGet()).accent).toBe('prussia');
    let active = doc.id;
    api.bindActiveVault(() => active);
    const seen: [string | null, Partial<Settings>][] = [];
    const off = api.onSettingsChanged((v, s) => seen.push([v, s]));
    expect(await api.settingsSet({ theme: 'light', auto_lock_minutes: 5 })).toMatchObject({ auto_lock_minutes: 5, accent: 'velluto', theme: 'light' });
    expect((await api.settingsGet(arc.id)).theme).toBe('dark');
    active = arc.id;
    expect((await api.settingsGet()).accent).toBe('prussia');
    expect((await api.settingsSet({ theme: 'neon' as never })).theme).toBe('dark');
    off();
    expect(seen.map(([v, s]) => [v, s.theme])).toEqual([[doc.id, 'light'], [arc.id, 'dark']]);

    await api.vaultLock(doc.id);
    await expect(api.settingsGet(doc.id)).rejects.toMatchObject({ code: 'locked' });
    const again = await api.vaultOpen(DOC, MOCK_PASSWORD);
    expect(again.settings).toMatchObject({ auto_lock_minutes: 5, accent: 'velluto', theme: 'light' });
  });
});
