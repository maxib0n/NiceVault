import { describe, expect, it } from 'vitest';
import { checkDigit } from './lathe';
import { mrzInside, mrzLines, placeOf } from './document';

describe('the document MRZ', () => {
  const v = { name: 'Documenti', path: '/run/media/utente/CHIAVETTA/Documenti.nvault' };

  it('ICAO 9303 check digits (7-3-1 over the MRZ alphabet)', () => {
    expect(checkDigit('L898902C3')).toBe('6');
    expect(checkDigit('740812')).toBe('2');
    expect(checkDigit('<<<<<<')).toBe('0');
  });

  it('locked: only what the file and its header say, every field checked', () => {
    const [l1, l2] = mrzLines({ ...v, size_bytes: 2.61 * 1024 ** 3, last_opened: Date.UTC(2026, 9, 24, 12), volume_tag: '9Q6W9DD18', key_slots: 3, kdf: { m_kib: 524_288, t: 3, p: 4 } });
    expect(l1).toBe('NV<CHIAVETTA<<DOCUMENTI'.padEnd(44, '<'));
    expect(l2).toHaveLength(44);
    expect(l2.slice(0, 10)).toBe('9Q6W9DD18' + checkDigit('9Q6W9DD18'));
    expect(l2.slice(10, 20)).toBe('000002673' + checkDigit('000002673'));
    expect(l2.slice(20, 27)).toBe('261024' + checkDigit('261024'));
    expect(l2.slice(27, 30)).toBe('K03');
    expect(l2.slice(30, 43)).toBe('512M3T4P<<<<<');
    expect(l2[43]).toBe(checkDigit(l2.slice(0, 43)));
  });

  it('what is not known prints as filler, never as a made-up value', () => {
    const [, l2] = mrzLines({ ...v });
    expect(l2).toBe('<<<<<<<<<0<<<<<<<<<0<<<<<<0' + '<'.repeat(16) + '0');
  });

  it('unlocked: made, files, last change, keys', () => {
    const [, l2] = mrzInside({ ...v, volume_tag: '9Q6W9DD18' }, { created: Date.UTC(2026, 6, 1, 12), files: 171, changed: Date.UTC(2026, 9, 24, 12), keys: 3 });
    expect(l2).toHaveLength(44);
    expect(l2.slice(10, 17)).toBe('260701' + checkDigit('260701'));
    expect(l2.slice(17, 27)).toBe('000000171' + checkDigit('000000171'));
    expect(l2.slice(27, 34)).toBe('261024' + checkDigit('261024'));
    expect(l2.slice(34, 37)).toBe('K03');
  });

  it('place: the drive, never the name twice', () => {
    expect(placeOf(v.path)).toBe('CHIAVETTA');
    expect(placeOf('/run/media/u/SH2RE/SH2RE.nvault', 'SH2RE')).toBe('');
  });
});
