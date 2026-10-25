import { describe, expect, it } from 'vitest';
import { lineAdvances, parseColor } from './util';

const adv = (ch: string) => (ch === ' ' ? 3 : 10);

describe('lineAdvances (one-line glyph placement)', () => {
  it('gives every character its advance plus the letter spacing', () => {
    expect(lineAdvances(['a', 'b', ' ', 'c'], adv, 1, false)).toEqual([11, 11, 4, 11]);
  });

  it('collapses white space as CSS does: none at the ends, one space per run', () => {
    expect(lineAdvances([' ', ' ', 'a', ' ', ' ', 'b', ' '], adv, 0, false)).toEqual([0, 0, 10, 3, 0, 10, 0]);
  });

  it('keeps every space in pre-formatted text', () => {
    expect(lineAdvances([' ', 'a', ' ', ' '], adv, 0, true)).toEqual([3, 10, 3, 3]);
  });

  it('tells the advance which characters start a word (text-transform: capitalize)', () => {
    const firsts: string[] = [];
    lineAdvances(['a', 'b', ' ', 'c', 'd'], (ch, first) => (first && firsts.push(ch), 1), 0, false);
    expect(firsts).toEqual(['a', 'c']);
  });
});

describe('parseColor', () => {
  it('reads rgb() and rgba()', () => {
    expect(parseColor('rgb(1, 2, 3)')).toEqual([1, 2, 3, 1]);
    expect(parseColor('rgba(10, 20, 30, 0.5)')).toEqual([10, 20, 30, 0.5]);
  });
});
