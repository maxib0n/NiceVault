// Search helpers (mirror of nv-core search.rs: same cases as its unit tests) and the mock's
// fs_search / tags_* / fs_list parity with app/src-tauri/tests/search.rs.
import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';
import { fold, highlightParts, highlights, kindGroup, matchesFilters, naturalCmp, rank, tagColor, terms } from './search';
import type { TagsChanged } from './types';
import { ROOT } from './types';

describe('search helpers (same cases as nv-core)', () => {
  it('folds case and accents', () => {
    expect(fold('Città')).toBe('citta');
    expect(fold('CITTÀ É Ñandú')).toBe('citta e nandu');
    expect(fold('Straße Æsir Øre Łódź')).toBe('strasse aesir ore lodz');
    expect(fold('ÅNGSTRÖM')).toBe('angstrom');
  });

  it('splits terms and phrases', () => {
    expect(terms('  Mare  Estate mare ')).toEqual(['mare', 'estate']);
    expect(terms('foto "Mare d\'inverno" x')).toEqual(['foto', "mare d'inverno", 'x']);
    expect(terms('"aperto')).toEqual(['aperto']);
    expect(terms('   ')).toEqual([]);
  });

  it('groups kinds by extension', () => {
    expect(kindGroup({ name: 'Mare.JPG', kind: 'file' })).toBe('photos');
    expect(kindGroup({ name: 'clip.mkv', kind: 'file' })).toBe('videos');
    expect(kindGroup({ name: 'a.flac', kind: 'file' })).toBe('audio');
    expect(kindGroup({ name: 'Bolletta.pdf', kind: 'file' })).toBe('documents');
    expect(kindGroup({ name: 'x.tar.gz', kind: 'file' })).toBe('archives');
    expect(kindGroup({ name: 'main.rs', kind: 'file' })).toBe('code');
    expect(kindGroup({ name: '.bashrc', kind: 'file' })).toBe('other');
    expect(kindGroup({ name: 'README', kind: 'file' })).toBe('other');
    expect(kindGroup({ name: 'Foto.jpg', kind: 'dir' })).toBe('folders');
  });

  it('ranks, highlights in UTF-16 and cuts names into parts', () => {
    const t = terms('mare');
    expect(rank('mare.jpg', t)).toBe(0);
    expect(rank("mare d'inverno.jpg", t)).toBe(1);
    expect(rank('al mare.jpg', t)).toBe(2);
    expect(rank('amarena.jpg', t)).toBe(3);
    expect(highlights('Città di MARE e mare', terms('mare citta'))).toEqual([[0, 5], [9, 13], [16, 20]]);
    expect(highlights('Città', terms('a'))).toEqual([[4, 5]]);
    expect(highlights('😀 Aß.txt', terms('ass'))).toEqual([[3, 5]]);
    expect(highlightParts('Al mare.jpg', [[3, 7]])).toEqual([
      { text: 'Al ', hit: false },
      { text: 'mare', hit: true },
      { text: '.jpg', hit: false },
    ]);
  });

  it('orders digit runs as numbers', () => {
    expect(['foto 10', 'foto 2', 'foto 02', 'foto', 'foto 1a'].sort(naturalCmp)).toEqual(['foto', 'foto 1a', 'foto 2', 'foto 02', 'foto 10']);
  });

  it('applies filters', () => {
    const photo = { name: 'a.jpg', kind: 'file' as const, size: 10, mtime: 15 };
    expect(matchesFilters(photo, [], { kinds: ['photos'], min_size: 10 })).toBe(true);
    expect(matchesFilters(photo, [], { kinds: ['photos'], min_size: 11 })).toBe(false);
    expect(matchesFilters({ ...photo, kind: 'dir', name: 'A' }, [], { min_size: 0 })).toBe(false);
    expect(matchesFilters(photo, [], { modified_after: 10, modified_before: 20 })).toBe(true);
    expect(matchesFilters(photo, [], { modified_before: 15 })).toBe(false);
    expect(matchesFilters(photo, [2, 1, 3], { tags: [1, 2] })).toBe(true);
    expect(matchesFilters(photo, [1], { tags: [1, 2] })).toBe(false);
    expect(matchesFilters(photo, [2], { tags: [1, 2], tags_any: true })).toBe(true);
    expect(tagColor('prussia')).toBe('prussia');
    expect(tagColor('magenta-of-the-future')).toBe('neutral');
  });
});

describe('fs_search + tags (mock backend)', () => {
  let vault: string;
  beforeEach(async () => {
    api.useMock(new MockBackend({ speed: 0 }));
    vault = (await api.vaultOpen(`${STICK}/Documenti.nvault`, MOCK_PASSWORD)).id;
  });

  it('searches names in every folder, ranked, with paths, highlights, pages', async () => {
    const r = await api.fsSearch(vault, 'DOLOMITI');
    expect(r.hits.map((h) => h.node.name)).toEqual(['Dolomiti 01.jpg', 'Dolomiti 02.jpg', 'Dolomiti 03.jpg']);
    expect(r.hits[0].path).toBe('/Foto/Vacanze 2024');
    expect(r.hits[0].highlights).toEqual([[0, 8]]);
    expect(r.next_cursor).toBeNull();
    expect((await api.fsSearch(vault, 'ragu')).hits.map((h) => h.node.name)).toEqual(['Ragù della nonna.txt']);
    const p1 = await api.fsSearch(vault, '', { limit: 5, sort: 'name' });
    expect(p1.hits).toHaveLength(5);
    expect(p1.next_cursor).toBe('o5');
    const p2 = await api.fsSearch(vault, '', { limit: 5, sort: 'name', cursor: p1.next_cursor });
    expect(p2.hits[0].node.name).not.toBe(p1.hits[0].node.name);
    expect(p1.hits[0].node.kind).toBe('dir'); // folders first
    await expect(api.fsSearch(vault, '', { cursor: 'nope' })).rejects.toMatchObject({ code: 'invalid_query' });
  });

  it('filters by kind, size, date, tags and folder', async () => {
    const photos = await api.fsSearch(vault, '', { filters: { kinds: ['photos'], min_size: 4 * 1024 * 1024 } });
    expect(photos.hits.map((h) => h.node.name).sort()).toEqual(['Dolomiti 01.jpg', 'Dolomiti 03.jpg', 'Lago di Braies.jpg', 'Panorama enorme.tif']);
    const tags = await api.tagsList(vault);
    const vacanze = tags.find((t) => t.name === 'Vacanze')!;
    expect(vacanze).toMatchObject({ color: 'ottanio', count: 4 });
    const tagged = await api.fsSearch(vault, '', { filters: { tags: [vacanze.id] } });
    expect(tagged.hits.map((h) => h.node.name)).toEqual(['Vacanze 2024', 'Dolomiti 01.jpg', 'Lago di Braies.jpg', 'Tramonto sul lago.jpg']);
    expect(tagged.hits[1].tags).toEqual([vacanze.id]);
    const foto = (await api.fsList(vault, ROOT)).find((n) => n.name === 'Foto')!;
    const inFoto = await api.fsSearch(vault, 'lago', { filters: { within: foto.id } });
    expect(inFoto.total).toBe(2);
    const future = await api.fsSearch(vault, '', { filters: { modified_after: Date.now() * 1e6 + 1e15 } });
    expect(future.total).toBe(0);
  });

  it('manages tags with events, and fs_list carries them and filters', async () => {
    const events: TagsChanged[] = [];
    const off = await api.onTagsChanged((e) => events.push(e));
    const t = await api.tagsCreate(vault, ' Viaggi ', 'prussia');
    expect(t).toMatchObject({ name: 'Viaggi', color: 'prussia', count: 0 });
    await expect(api.tagsCreate(vault, 'VIAGGÌ', 'neutral')).rejects.toMatchObject({ code: 'exists' });
    await expect(api.tagsCreate(vault, '', 'neutral')).rejects.toMatchObject({ code: 'invalid_name' });
    await expect(api.tagsCreate(vault, 'x', 'rosso' as never)).rejects.toMatchObject({ code: 'invalid_color' });
    const root = await api.fsList(vault, ROOT);
    const leggimi = root.find((n) => n.name === 'Leggimi.txt') ?? root.find((n) => n.kind === 'file')!;
    const dir = root.find((n) => n.kind === 'dir')!;
    const after = await api.tagsAssign(vault, [t.id], [leggimi.id, dir.id, ROOT, 999999]);
    expect(after.find((x) => x.id === t.id)!.count).toBe(2);
    expect(events.at(-1)).toEqual({ vault, nodes: [dir.id, leggimi.id].sort((a, b) => a - b), dirs: [ROOT] });
    const listed = await api.fsList(vault, ROOT);
    expect(listed.find((n) => n.id === leggimi.id)!.tags).toEqual([t.id]);
    expect((await api.fsList(vault, ROOT, { tags: [t.id] })).map((n) => n.id).sort()).toEqual([dir.id, leggimi.id].sort());
    expect((await api.fsList(vault, ROOT, { kinds: ['folders'] })).every((n) => n.kind === 'dir')).toBe(true);
    expect(await api.tagsOf(vault, [leggimi.id, 999999])).toEqual({ [String(leggimi.id)]: [t.id] });
    const up = await api.tagsUpdate(vault, t.id, { name: 'Vacanze lontane', color: 'seppia' });
    expect(up).toMatchObject({ name: 'Vacanze lontane', color: 'seppia', count: 2 });
    await api.tagsUnassign(vault, [t.id], [dir.id]);
    expect(events.at(-1)!.nodes).toEqual([dir.id]);
    // deleting a node drops its tags (the volume's foreign key)
    await new Promise<void>((resolve) => {
      void api.onJobDone(() => resolve()).then(() => api.fsDelete(vault, [leggimi.id]));
    });
    expect((await api.tagsList(vault)).find((x) => x.id === t.id)!.count).toBe(0);
    await api.tagsDelete(vault, t.id);
    await expect(api.tagsDelete(vault, t.id)).rejects.toMatchObject({ code: 'not_found' });
    off();
  });
});
