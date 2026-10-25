// Live refresh of the file browser on fs-changed (changes made through the mounted drive).
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../api';
import { MOCK_PASSWORD, MockBackend, STICK } from '../mock';
import type { FsNode, VaultInfo } from '../types';
import { ROOT } from '../types';
import { FileBrowser } from './browser.svelte';

const DOC = `${STICK}/Documenti.nvault`;

describe('FileBrowser live refresh (fs-changed)', () => {
  let m: MockBackend;
  let info: VaultInfo;
  let b: FileBrowser;
  const id = (name: string) => b.entries.find((n) => n.name === name)!.id;
  const names = () => b.sorted.map((n) => n.name);

  beforeEach(async () => {
    api.useMock((m = new MockBackend({ speed: 0 })));
    info = await api.vaultOpen(DOC, MOCK_PASSWORD);
    b = new FileBrowser(info);
    await b.load();
  });

  it('an event for the current folder re-lists it without user action', async () => {
    const [nid] = m.simulateExternalChange(ROOT, { add: ['da Dolphin.txt'] });
    await vi.waitFor(() => expect(names()).toContain('da Dolphin.txt'));
    expect(b.byId.get(nid)?.kind).toBe('file');
    expect(b.changes).toBe(1);
    b.dispose();
    m.simulateExternalChange(ROOT, { add: ['dopo.txt'] });
    await new Promise((r) => setTimeout(r, 20));
    expect(names()).not.toContain('dopo.txt'); // unsubscribed
  });

  it('keeps selection, anchor and focus on items that still exist; drops the removed ones', async () => {
    const [foto, first] = [id('Foto'), b.sorted.find((n) => n.kind === 'file')!];
    b.click(foto, {});
    b.click(first.id, { toggle: true });
    b.quickLook = first.id;
    b.renaming = foto;
    m.simulateExternalChange(ROOT, { add: ['nuovo.txt'], emit: false });
    expect(await b.externalChange([ROOT])).toBe(true);
    expect(names()).toContain('nuovo.txt');
    expect([...b.sel.selected].sort()).toEqual([foto, first.id].sort());
    expect(b.sel.focus).toBe(first.id);
    expect(b.sel.anchor).toBe(first.id);
    expect(b.quickLook).toBe(first.id);
    expect(b.renaming).toBe(foto);
    expect(b.loading).toBe(false);

    m.simulateExternalChange(ROOT, { remove: [first.name], emit: false });
    expect(await b.externalChange([ROOT])).toBe(true);
    expect([...b.sel.selected]).toEqual([foto]);
    expect(b.sel.focus).toBeNull();
    expect(b.quickLook).toBeNull();
    expect(b.renaming).toBe(foto);
  });

  it('ignores folders that are not on screen', async () => {
    await b.openDir(b.byId.get(id('Foto'))!);
    const vacanze = id('Vacanze 2024');
    await b.openDir(b.byId.get(vacanze)!);
    const nested = m.simulateExternalChange(vacanze, { add: ['Sotto'], emit: false })[0];
    const spy = vi.spyOn(m, 'invoke');
    expect(await b.externalChange([nested, 9999])).toBe(false);
    expect(spy.mock.calls.filter((c) => c[0] === 'fs_list')).toHaveLength(0);
    spy.mockRestore();
  });

  it('a change inside a folder tile re-lists the folder (its peek re-reads)', async () => {
    const before = b.entries;
    m.simulateExternalChange(id('Foto'), { add: ['nuova foto.jpg'], emit: false });
    expect(await b.externalChange([id('Foto')])).toBe(true);
    expect(b.entries).not.toBe(before);
  });

  it('a folder of the path renamed or deleted outside: crumbs follow', async () => {
    const foto = b.byId.get(id('Foto')) as FsNode;
    await b.openDir(foto);
    await b.openDir(b.byId.get(id('Vacanze 2024'))!);
    // Foto renamed from the file manager: same id, new crumb label, still inside
    await api.fsRename(info.id, foto.id, ROOT, 'Fotografie');
    expect(await b.externalChange([ROOT])).toBe(true);
    expect(b.trail.map((c) => c.name)).toEqual(['Documenti', 'Fotografie', 'Vacanze 2024']);
    expect(names()).toContain('Dolomiti 01.jpg');
    // then deleted: back to the deepest folder that still exists
    m.simulateExternalChange(ROOT, { remove: ['Fotografie'], emit: false });
    expect(await b.externalChange([ROOT])).toBe(true);
    expect(b.trail.map((c) => c.name)).toEqual(['Documenti']);
    expect(b.cwd).toBe(ROOT);
    expect(names()).not.toContain('Fotografie');
  });

  it('a burst of events is serialized and coalesced into one re-list', async () => {
    const spy = vi.spyOn(m, 'invoke');
    m.simulateExternalChange(ROOT, { add: ['a.txt'], emit: false });
    const p1 = b.externalChange([ROOT]);
    m.simulateExternalChange(ROOT, { add: ['b.txt'], emit: false });
    const p2 = b.externalChange([ROOT]);
    const p3 = b.externalChange([ROOT]);
    expect(await Promise.all([p1, p2, p3])).toEqual([true, false, false]);
    expect(spy.mock.calls.filter((c) => c[0] === 'fs_list')).toHaveLength(1);
    // one arriving while a re-list is in flight runs after it
    const p4 = b.externalChange([ROOT]);
    await Promise.resolve();
    m.simulateExternalChange(ROOT, { add: ['c.txt'], emit: false });
    const p5 = b.externalChange([ROOT]);
    expect(await Promise.all([p4, p5])).toEqual([true, true]);
    expect(names()).toContain('c.txt');
    expect(names()).toEqual(expect.arrayContaining(['a.txt', 'b.txt']));
    spy.mockRestore();
  });
});
