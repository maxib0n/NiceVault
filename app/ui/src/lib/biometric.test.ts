import { describe, expect, it } from 'vitest';
import * as api from './api';
import { bioLabel, bioReason, bioTitle } from './biometric';
import { i18n } from './i18n.svelte';
import { MOCK_PASSWORD, MockBackend, STICK } from './mock';

describe('biometric unlock', () => {
  it('names the phone kind and its slot, in both languages', () => {
    i18n.choice = 'it';
    expect(bioTitle({ kind: 'fingerprint' })).toBe('Sblocca con l’impronta');
    expect(bioTitle({ kind: 'face' })).toBe('Sblocca con Face ID');
    expect(bioLabel({ kind: 'fingerprint', device: 'Pixel 9 Pro' })).toBe('Pixel 9 Pro · impronta');
    expect(bioLabel({ kind: 'biometric', device: '  ' })).toBe('NiceVault · biometria');
    expect(bioReason('no_passcode')).toBe('Imposta prima un blocco schermo sul telefono.');
    i18n.choice = 'en';
    expect(bioTitle({ kind: 'biometric' })).toBe('Unlock with biometrics');
    expect(bioLabel({ kind: 'face', device: 'iPhone' })).toBe('iPhone · Face ID');
    expect(bioReason('none_enrolled')).toMatch(/^First enrol/);
    i18n.choice = 'it';
  });

  it('mock parity: on, unlock, revoked by removing the slot, the password still opens', async () => {
    const mock = new MockBackend({ speed: 0 });
    api.useMock(mock);
    const path = `${STICK}/Documenti.nvault`;
    let v = await api.vaultOpen(path, MOCK_PASSWORD);
    expect(await api.bioStatus(path)).toMatchObject({ available: true, enabled: false, stale: false });
    await expect(api.bioUnlock(path, 'Documenti')).rejects.toMatchObject({ code: 'bio_off' });
    const on = await api.bioEnable(v.id, 'Pixel · impronta');
    expect(on.enabled).toBe(true);
    const device = (await api.slotsList(v.id)).find((s) => s.kind === 'device')!;
    expect(device.label).toBe('Pixel · impronta');

    await api.vaultLock(v.id);
    v = await api.bioUnlock(path, 'Documenti');
    expect((await api.slotsList(v.id)).find((s) => s.in_use)?.kind).toBe('device');

    await api.slotRemove(v.id, device.index);
    expect((await api.bioStatus(path)).enabled).toBe(false);
    await api.bioEnable(v.id, 'Pixel · impronta');
    await api.vaultLock(v.id);
    mock.removeDeviceSlots(path);
    await expect(api.bioUnlock(path, 'Documenti')).rejects.toMatchObject({ code: 'bio_revoked' });
    expect(await api.bioStatus(path)).toMatchObject({ enabled: false, stale: true });
    v = await api.vaultOpen(path, MOCK_PASSWORD);
    expect(v.path).toBe(path);
  });
});
