import { describe, expect, it } from 'vitest';
import * as S from './selection';

const order = [10, 11, 12, 13, 14, 15];
const ids = (s: S.Selection<number>) => [...s.selected].sort((a, b) => a - b);

describe('selection', () => {
  it('plain click selects one and sets anchor/focus', () => {
    const s = S.click(S.empty<number>(), order, 12);
    expect(ids(s)).toEqual([12]);
    expect(s.anchor).toBe(12);
    expect(s.focus).toBe(12);
  });

  it('shift-click selects a range from the anchor, both directions', () => {
    let s = S.click(S.empty<number>(), order, 12);
    s = S.click(s, order, 14, { shift: true });
    expect(ids(s)).toEqual([12, 13, 14]);
    s = S.click(s, order, 10, { shift: true });
    expect(ids(s)).toEqual([10, 11, 12]);
    expect(s.anchor).toBe(12);
  });

  it('ctrl-click toggles and moves the anchor', () => {
    let s = S.click(S.empty<number>(), order, 10);
    s = S.click(s, order, 13, { toggle: true });
    expect(ids(s)).toEqual([10, 13]);
    s = S.click(s, order, 10, { toggle: true });
    expect(ids(s)).toEqual([13]);
    expect(s.anchor).toBe(10);
  });

  it('ctrl+shift-click adds a range to the existing selection', () => {
    let s = S.click(S.empty<number>(), order, 10);
    s = S.click(s, order, 13, { toggle: true });
    s = S.click(s, order, 15, { toggle: true, shift: true });
    expect(ids(s)).toEqual([10, 13, 14, 15]);
  });

  it('arrow move with no focus starts at the ends', () => {
    expect(S.move(S.empty<number>(), order, 1).focus).toBe(10);
    expect(S.move(S.empty<number>(), order, -1).focus).toBe(15);
  });

  it('arrow move clamps and selects; shift extends; ctrl moves focus only', () => {
    let s = S.click(S.empty<number>(), order, 14);
    s = S.move(s, order, 5);
    expect(s.focus).toBe(15);
    expect(ids(s)).toEqual([15]);
    s = S.move(s, order, -2, { shift: true });
    expect(ids(s)).toEqual([13, 14, 15]);
    expect(s.anchor).toBe(15);
    s = S.move(s, order, -1, { toggle: true });
    expect(s.focus).toBe(12);
    expect(ids(s)).toEqual([13, 14, 15]);
    s = S.toggleFocus(s);
    expect(ids(s)).toEqual([12, 13, 14, 15]);
  });

  it('grid moves by column count', () => {
    let s = S.click(S.empty<number>(), order, 10);
    s = S.move(s, order, 3);
    expect(s.focus).toBe(13);
  });

  it('select all, clear, ensureSelected', () => {
    let s = S.selectAll(S.empty<number>(), order);
    expect(ids(s)).toEqual(order);
    expect(S.ensureSelected(s, 12).selected.size).toBe(6);
    s = S.clear(s);
    expect(s.selected.size).toBe(0);
    expect(ids(S.ensureSelected(s, 12))).toEqual([12]);
  });

  it('prune drops vanished ids; ordered follows display order', () => {
    let s = S.click(S.empty<number>(), order, 15);
    s = S.click(s, order, 11, { toggle: true });
    expect(S.ordered(s, order)).toEqual([11, 15]);
    s = S.prune(s, [10, 11, 12]);
    expect(ids(s)).toEqual([11]);
    expect(s.anchor).toBe(11);
  });
});
