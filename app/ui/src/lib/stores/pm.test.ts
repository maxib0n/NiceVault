import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../api';
import { MOCK_PASSWORD, MockBackend, STICK } from '../mock';
import { generated, iconHost, parseTime, parseUrls } from '../pmMock';
import { folderTree, haystack, PmStore, searchIn } from './pm.svelte';

const DOC = `${STICK}/Documenti.nvault`;
const titles = (s: PmStore) => s.visible.map((e) => e.title);

describe('PmStore (mock backend)', () => {
  let m: MockBackend;
  let s: PmStore;

  beforeEach(async () => {
    api.useMock((m = new MockBackend({ speed: 0 })));
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    s = new PmStore(v.id);
    await s.load();
  });

  it('searches in memory, applying the query on the next frame', async () => {
    s.setQuery('example.it mario');
    expect(s.query).toBe('example.it mario');
    await vi.waitFor(() => expect(s.applied).toBe('example.it mario'));
    expect(titles(s)).toEqual(['Posta personale', 'SPID']);
    expect((await api.pmSearch(s.vault, 'example.it mario')).map((e) => e.title)).toEqual(titles(s));
    s.setQuery('');
    expect(s.applied).toBe('');
    expect(s.visible).toHaveLength(6);
  });

  it('counts every filter in one pass', () => {
    expect(s.countIn({ kind: 'all' })).toBe(6);
    expect(s.countIn({ kind: 'favorites' })).toBe(2);
    expect(s.countIn({ kind: 'totp' })).toBe(3);
    expect(s.countIn({ kind: 'folder', folder: 'Personale' })).toBe(3);
    expect(s.countIn({ kind: 'folder', folder: 'Personale/Banche' })).toBe(1);
    expect(s.folders).toEqual(['Casa', 'Lavoro', 'Personale', 'Personale/Banche']);
  });

  it('picks several entries and moves them to a folder; rename and delete folders', async () => {
    const id = (t: string) => s.all.find((e) => e.title === t)!.id;
    await s.select(id('Film in streaming'));
    await s.select(id('SPID'), { shift: true });
    expect(s.picked.length).toBeGreaterThan(1);
    await s.select(id('Posta personale'), { toggle: true });
    const picked = [...s.picked];
    expect(picked).not.toContain(id('Posta personale'));
    expect(await s.move(picked, 'Nuova/Sotto')).toBe(true);
    for (const p of picked) expect(s.all.find((e) => e.id === p)!.folder).toBe('Nuova/Sotto');
    expect(s.folders).toContain('Nuova');

    s.filter = { kind: 'folder', folder: 'Nuova/Sotto' };
    expect(await s.renameFolder('Nuova', 'Rinominata')).toBeNull();
    expect(s.filter).toEqual({ kind: 'folder', folder: 'Rinominata/Sotto' });
    expect(s.countIn({ kind: 'folder', folder: 'Rinominata/Sotto' })).toBe(picked.length);
    expect(await s.renameFolder('Inesistente', 'X')).toMatch(/.+/);

    await s.deleteFolder('Rinominata');
    expect(s.filter).toEqual({ kind: 'all' });
    expect(s.all.find((e) => e.id === picked[0])!.folder).toBe('Sotto');
    await s.move([picked[0]], '');
    expect(s.all.find((e) => e.id === picked[0])!.folder).toBe('');
  });

  it('a move does not change the modification time', async () => {
    const e = s.all.find((x) => x.title === 'SPID')!;
    await s.move([e.id], 'Altrove');
    expect(s.all.find((x) => x.id === e.id)!.updated).toBe(e.updated);
  });

  it('repairs raw import fields (AUTOFILLURLS, CREATETIME, MODIFYTIME) once', async () => {
    const d = await api.pmCreate(s.vault, {
      title: 'login.microsoftonline.com',
      urls: ['https://login.microsoftonline.com/'],
      fields: [
        { name: 'AUTOFILLURLS', value: '[{"url":"https://login.microsoftonline.com/","mode":0},{"url":"https://b.example/","mode":0}]' },
        { name: 'CREATETIME', value: '1774863307' },
        { name: 'MODIFYTIME', value: '1790593026' },
        { name: 'Domanda', value: 'x' },
      ],
    });
    await s.repair();
    const after = await api.pmGet(s.vault, d.id);
    expect(after.urls).toEqual(['https://login.microsoftonline.com/', 'https://b.example/']);
    expect([after.created, after.updated]).toEqual([1_774_863_307_000, 1_790_593_026_000]);
    expect(after.fields.map((f) => f.name)).toEqual(['Domanda']);
    expect(await api.pmRepair(s.vault)).toBe(0);
  });

  it('site icons: refreshed on request, off with the setting', async () => {
    await api.appSettingsSet({ site_icons: false });
    expect(await api.pmIconsRefresh(s.vault)).toBe(0);
    await api.appSettingsSet({ site_icons: true });
    const hosts = await api.pmIconsRefresh(s.vault);
    expect(hosts).toBeGreaterThan(0);
    await s.startIcons();
    await vi.waitFor(async () => expect((await api.pmList(s.vault)).filter((e) => e.icon).length).toBeGreaterThan(0));
    const withIcon = (await api.pmList(s.vault)).find((e) => e.icon)!;
    expect(withIcon.icon!.split('/')[0]).toBe(iconHost(withIcon.urls));
    expect(api.pmIconUrl(s.vault, withIcon.icon!)).toMatch(/^data:image\//);
    expect((await api.pmList(s.vault)).find((e) => e.title === 'Wi-Fi di casa')!.icon).toBeNull();
    s.dispose();
  });

  it('handles thousands of entries', async () => {
    api.useMock((m = new MockBackend({ speed: 0, pmEntries: 2000 })));
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const big = new PmStore(v.id);
    await big.load();
    expect(big.all).toHaveLength(2006);
    const t0 = performance.now();
    big.applied = 'banca 00';
    const n = big.visible.length;
    expect(performance.now() - t0).toBeLessThan(50);
    expect(n).toBeGreaterThan(0);
    expect(big.positions.get(big.visible[n - 1].id)).toBe(n - 1);
    void m;
  });
});

describe('pure helpers', () => {
  it('search terms must all match the haystack', () => {
    const e = generated(0, Date.now());
    const s = { id: 'x', title: e.input.title!, username: e.input.username!, urls: e.input.urls!, folder: '', tags: [], favorite: false, has_password: true, has_totp: false, created: 0, updated: 0, icon: null };
    expect(searchIn([s], [haystack(s)], 'posta 0001')).toHaveLength(1);
    expect(searchIn([s], [haystack(s)], 'posta zzz')).toHaveLength(0);
    expect(folderTree([{ ...s, folder: 'A/B/C' }])).toEqual(['A', 'A/B', 'A/B/C']);
  });

  it('mirrors the backend import mapping', () => {
    expect(parseTime('1774863307')).toBe(1_774_863_307_000);
    expect(parseTime('1774863307123')).toBe(1_774_863_307_123);
    expect(parseTime('2024-02-29T12:00:00Z')).toBe(1_709_208_000_000);
    expect(parseTime('42')).toBeNull();
    expect(parseUrls('[{"url":"https://a.example","mode":0}]')).toEqual(['https://a.example']);
    expect(parseUrls('non un url')).toBeNull();
    expect(iconHost(['http://192.168.1.1', 'https://www.Example.com/x'])).toBe('www.example.com');
    expect(iconHost(['https://nas.local'])).toBeNull();
  });

  it('imports CSV with AUTOFILLURLS / CREATETIME / MODIFYTIME columns mapped', async () => {
    api.useMock(new MockBackend({ speed: 0 }));
    const v = await api.vaultOpen(DOC, MOCK_PASSWORD);
    const csv = 'name,url,username,password,AUTOFILLURLS,CREATETIME,MODIFYTIME\nMS,https://login.example.com/,u,p,"[{""url"":""https://b.example/"",""mode"":0}]",1774863307,1790593026\n';
    const done = new Promise<void>((r) => void api.onJobDone(() => r()));
    await api.pmImport(v.id, 'csv', { data: csv });
    await done;
    const e = (await api.pmList(v.id)).find((x) => x.title === 'MS')!;
    expect(e.urls).toEqual(['https://login.example.com/', 'https://b.example/']);
    expect([e.created, e.updated]).toEqual([1_774_863_307_000, 1_790_593_026_000]);
    expect((await api.pmGet(v.id, e.id)).fields).toEqual([]);
  });
});
