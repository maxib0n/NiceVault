import { describe, expect, it } from 'vitest';
import { KeyMock, KEY_PASSWORD, fingerprintOf, parsePublic, samplePublic } from './keyMock';
import { groupFingerprint } from './stores/mykey.svelte';

const wait = () => Promise.resolve();

describe('keyMock (mykey.rs parity)', () => {
  it('shapes public keys like the real ones and checks pastes', () => {
    const k = samplePublic('Anna');
    expect(k).toMatch(/^nvpk1[A-Za-z0-9_-]{1627}$/);
    expect(samplePublic('Anna')).toBe(k);
    expect(fingerprintOf(k)).toMatch(/^[0-9a-f]{32}$/);
    expect(parsePublic(`  ${k.slice(0, 700)}\n${k.slice(700)} `)).toBe(k);
    expect(() => parsePublic('nvpk1abc')).toThrow(expect.objectContaining({ code: 'invalid_recipient' }));
    expect(() => parsePublic('NVKEY001…')).toThrow(expect.objectContaining({ code: 'secret_key_file' }));
    expect(groupFingerprint('0123456789abcdef0123456789abcdef')).toBe('0123 4567 89ab cdef 0123 4567 89ab cdef');
  });

  it('create, export, import round trip and refusals', async () => {
    const a = new KeyMock({ seed: null, installed: true });
    await expect(a.invoke('my_key_create', { password: '' }, wait)).rejects.toMatchObject({ code: 'invalid_password' });
    const mine = (await a.invoke('my_key_create', { password: 'pw' }, wait)) as { public: string; persisted: boolean };
    expect(mine.persisted).toBe(true);
    await expect(a.invoke('my_key_create', { password: 'pw' }, wait)).rejects.toMatchObject({ code: 'exists' });
    await a.invoke('my_key_export', { dest: '/x/me.nvkey' }, wait);
    await expect(a.invoke('my_key_export', { dest: '/x/me.nvkey' }, wait)).rejects.toMatchObject({ code: 'exists' });
    await a.invoke('my_key_export_public', { dest: '/x/me.nvpub' }, wait);
    await expect(a.invoke('recipient_read_file', { path: '/x/me.nvkey' }, wait)).rejects.toMatchObject({ code: 'secret_key_file' });
    expect(await a.invoke('recipient_read_file', { path: '/x/me.nvpub' }, wait)).toMatchObject({ public: mine.public });
    const files = a.files;
    const b = new KeyMock({ seed: null, installed: false });
    for (const [k, v] of files) b.files.set(k, v);
    await expect(b.invoke('my_key_import', { path: '/x/me.nvkey', password: 'no' }, wait)).rejects.toMatchObject({ code: 'wrong_password' });
    await expect(b.invoke('my_key_import', { path: '/x/me.nvpub', password: 'pw' }, wait)).rejects.toMatchObject({ code: 'not_a_key' });
    expect(await b.invoke('my_key_import', { path: '/x/me.nvkey', password: 'pw' }, wait)).toMatchObject({ public: mine.public, persisted: false });
    await expect(b.invoke('my_key_import', { path: '/y/other.nvkey', password: KEY_PASSWORD }, wait)).rejects.toMatchObject({ code: 'exists' });
  });
});
