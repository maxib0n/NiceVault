import { beforeEach, describe, expect, it } from 'vitest';
import * as api from './api';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';
import { _totp } from './pmMock';
import type { PmImportDone } from './passwords';

const DOC = `${STICK}/Documenti.nvault`;

let m: MockBackend;
let vault: string;

async function importDone(start: () => Promise<string>): Promise<PmImportDone> {
  let resolve!: (d: PmImportDone) => void;
  const got = new Promise<PmImportDone>((r) => (resolve = r));
  let job = '';
  const early: PmImportDone[] = [];
  const off = await api.onJobDone((d) => {
    const pd = d as unknown as PmImportDone;
    if (job && pd.job === job) resolve(pd);
    else early.push(pd);
  });
  job = await start();
  const e = early.find((d) => d.job === job);
  if (e) resolve(e);
  const d = await got;
  off();
  return d;
}

describe('password manager (mock backend)', () => {
  beforeEach(async () => {
    m = new MockBackend({ speed: 0 });
    api.useMock(m);
    vault = (await api.vaultOpen(DOC, MOCK_PASSWORD)).id;
  });

  it('seeds sample entries without secrets in list / get', async () => {
    const list = await api.pmList(vault);
    expect(list.length).toBeGreaterThanOrEqual(5);
    expect(list.map((e) => e.title)).toEqual([...list.map((e) => e.title)].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase())));
    const withTotp = list.filter((e) => e.has_totp);
    expect(withTotp.length).toBeGreaterThanOrEqual(2);
    const bank = list.find((e) => e.title === 'Banca del Borgo')!;
    const d = await api.pmGet(vault, bank.id);
    expect(d.totp).toMatchObject({ algorithm: 'SHA256', digits: 8, period: 60 });
    expect(d.fields.find((f) => f.hidden)?.value).toBeNull();
    const json = JSON.stringify(list) + JSON.stringify(d);
    expect(json).not.toContain('x7#Lq9');
    expect(json).not.toContain('GEZDGNBV');
    expect(json).not.toContain('48213');
  });

  it('creates, searches, reveals, updates with history and deletes', async () => {
    const e = await api.pmCreate(vault, {
      title: 'Forum',
      username: 'mario',
      password: 'pw-1',
      urls: ['https://forum.example.org', ' '],
      folder: ' Svago / Forum ',
      fields: [{ name: 'PIN', value: '1111', hidden: true }],
    });
    expect(e.folder).toBe('Svago/Forum');
    expect(e.urls).toEqual(['https://forum.example.org']);
    expect(await api.pmReveal(vault, e.id, 'password')).toBe('pw-1');
    expect(await api.pmReveal(vault, e.id, 'field:0')).toBe('1111');
    expect((await api.pmSearch(vault, 'FORUM svago')).map((x) => x.id)).toEqual([e.id]);
    expect(await api.pmSearch(vault, 'pw-1')).toEqual([]);

    await api.pmUpdate(vault, e.id, { password: 'pw-2', fields: [{ name: 'PIN', hidden: true }] });
    const h = await api.pmHistory(vault, e.id);
    expect(h).toHaveLength(1);
    expect(await api.pmReveal(vault, e.id, 'history:0')).toBe('pw-1');
    expect(await api.pmReveal(vault, e.id, 'field:0')).toBe('1111');

    await expect(api.pmCreate(vault, { title: '  ' })).rejects.toMatchObject({ code: 'invalid_entry' });
    await expect(api.pmCreate(vault, { title: 'x', totp: 'otpauth://hotp/x?secret=AA' })).rejects.toMatchObject({ code: 'invalid_totp' });
    await expect(api.pmReveal(vault, e.id, 'totp')).rejects.toMatchObject({ code: 'not_found' });
    await expect(api.pmReveal(vault, e.id, 'bogus' as never)).rejects.toMatchObject({ code: 'invalid_field' });

    await api.pmDelete(vault, e.id);
    await expect(api.pmGet(vault, e.id)).rejects.toMatchObject({ code: 'not_found' });
    await expect(api.pmDelete(vault, e.id)).rejects.toMatchObject({ code: 'not_found' });
  });

  it('computes RFC 6238 codes', async () => {
    const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ'; // "12345678901234567890"
    const t = _totp.parseTotp(`otpauth://totp/x?secret=${seed}&digits=8`);
    expect((await _totp.totpCode(t, 59_000)).code).toBe('94287082');
    expect((await _totp.totpCode(t, 1_111_111_109_000)).code).toBe('07081804');
    expect((await _totp.totpCode(t, 20_000_000_000_000)).code).toBe('65353130');
    const c = await _totp.totpCode(t, 61_000);
    expect(c.remaining).toBe(29);

    const list = await api.pmList(vault);
    const mail = list.find((e) => e.title === 'Posta personale')!;
    const code = await api.pmTotp(vault, mail.id);
    expect(code.code).toMatch(/^\d{6}$/);
    expect(code.period).toBe(30);
    const chk = await api.pmTotpCheck('JBSWY3DPEHPK3PXP');
    expect(chk).toMatchObject({ algorithm: 'SHA1', digits: 6, period: 30 });
    await expect(api.pmTotpCheck('https://x')).rejects.toMatchObject({ code: 'invalid_totp' });
  });

  it('copies and clears the clipboard only if it still holds the value', async () => {
    const e = await api.pmCreate(vault, { title: 'Clip', password: 'copy-me' });
    const r = await api.pmCopy(vault, e.id, 'password', 1);
    expect(r.clear_after).toBe(1);
    expect(m.pm.clipboard).toBe('copy-me');
    await new Promise((res) => setTimeout(res, 10));
    expect(m.pm.clipboard).toBe('');
    await api.pmCopy(vault, e.id, 'password', 1);
    m.pm.clipboard = 'something else';
    await new Promise((res) => setTimeout(res, 10));
    expect(m.pm.clipboard).toBe('something else');
    expect((await api.pmCopy(vault, e.id, 'password', 0)).clear_after).toBe(0);
  });

  it('generates passwords and passphrases with entropy', async () => {
    const g = await api.pmGenerate({ kind: 'random', length: 16, symbols: false, exclude_ambiguous: true });
    expect(g.value).toHaveLength(16);
    expect(g.value).toMatch(/^[A-Za-z0-9]+$/);
    expect(g.value).not.toMatch(/[0O1lI]/);
    expect(g.entropy_bits).toBeGreaterThan(85);
    expect(g.entropy_bits).toBeLessThan(16 * Math.log2(57));
    const p = await api.pmGenerate({ kind: 'passphrase', words: 4, language: 'it', separator: ' ', number: true });
    expect(p.value.split(' ')).toHaveLength(4);
    expect(p.entropy_bits).toBeCloseTo(4 * Math.log2(7776) + Math.log2(10) + 2, 6);
    await expect(api.pmGenerate({ kind: 'random', length: 2 })).rejects.toMatchObject({ code: 'invalid_options' });
    await expect(api.pmGenerate({ kind: 'random', lower: false, upper: false, digits: false, symbols: false })).rejects.toMatchObject({
      code: 'invalid_options',
    });
  });

  it('imports CSV / Bitwarden data and KeePass with its master password', async () => {
    const before = (await api.pmList(vault)).length;
    const csv = await importDone(() => api.pmImport(vault, 'csv', { data: 'name,url,username,password\n,https://www.nameless.example,me,pw\n' }));
    expect(csv).toMatchObject({ error: null, imported: 1 });
    expect((await api.pmSearch(vault, 'nameless.example'))[0].title).toBe('nameless.example');

    const bw = JSON.stringify({ encrypted: false, folders: [{ id: 'f', name: 'Lavoro' }], items: [{ type: 1, name: 'Bw', folderId: 'f', login: { username: 'u', password: 'p', totp: 'JBSWY3DPEHPK3PXP' } }] });
    expect(await importDone(() => api.pmImport(vault, 'bitwarden', { data: bw }))).toMatchObject({ error: null, imported: 1 });
    expect((await importDone(() => api.pmImport(vault, 'bitwarden', { data: '{"encrypted":true}' }))).error?.code).toBe('unsupported');

    expect((await importDone(() => api.pmImport(vault, 'keepass', { data: 'AAAA', master_password: 'no' }))).error?.code).toBe('wrong_password');
    const kp = await importDone(() => api.pmImport(vault, 'keepass', { path: '/home/utente/db.kdbx', master_password: MOCK_PASSWORD }));
    expect(kp.error).toBeNull();
    expect(kp.imported).toBe(3);
    const after = await api.pmList(vault);
    expect(after.length).toBe(before + 5);
    expect(after.some((e) => e.folder === 'Casa/Rete')).toBe(true);
    await expect(api.pmImport(vault, 'csv', {})).rejects.toMatchObject({ code: 'invalid_import' });
  });

  it('refuses everything once the volume is locked; new volumes start empty', async () => {
    await api.vaultLock(vault);
    await expect(api.pmList(vault)).rejects.toMatchObject({ code: 'locked' });
    const fresh = await api.vaultCreate(`${STICK}/Nuovo.nvault`, 'pw');
    expect(await api.pmList(fresh.id)).toEqual([]);
  });
});
