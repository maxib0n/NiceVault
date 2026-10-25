import { describe, expect, it } from 'vitest';
import { fileType } from './filetype';
import { formatBytes, formatCountdown } from './format';
import { sortNodes } from './sort';
import { humanDuration } from './strength';
import type { FsNode } from './types';

const n = (id: number, name: string, kind: 'file' | 'dir' = 'file', size = 0, mtime = 0): FsNode => ({ id, parent: 1, name, kind, size, mtime });

describe('helpers', () => {
  it('formats bytes and countdowns', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1536)).toBe('1,5 KB');
    expect(formatBytes(5 * 1024 ** 3)).toBe('5 GB');
    expect(formatCountdown(754_000)).toBe('12:34');
  });

  it('classifies files', () => {
    expect(fileType(n(1, 'a.JPG'))).toBe('image');
    expect(fileType(n(1, 'x.pdf'))).toBe('pdf');
    expect(fileType(n(1, 'id_ed25519'))).toBe('key');
    expect(fileType(n(1, 'Foto', 'dir'))).toBe('dir');
    expect(fileType(n(1, 'noext'))).toBe('other');
  });

  it('sorts folders first, natural name order, by size desc', () => {
    const list = [n(2, 'b10.txt', 'file', 5), n(3, 'b2.txt', 'file', 50), n(4, 'Zeta', 'dir'), n(5, 'àlfa.txt', 'file', 1)];
    expect(sortNodes(list, { key: 'name', dir: 'asc' }).map((x) => x.name)).toEqual(['Zeta', 'àlfa.txt', 'b2.txt', 'b10.txt']);
    expect(sortNodes(list, { key: 'size', dir: 'desc' }).map((x) => x.name)[1]).toBe('b2.txt');
  });

  it('describes crack times in Italian', () => {
    expect(humanDuration(0.5)).toBe('meno di un secondo');
    expect(humanDuration(3 * 3600)).toBe('3 ore');
    expect(humanDuration(1e12)).toBe('secoli');
  });
});
