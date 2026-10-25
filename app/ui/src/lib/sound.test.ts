import { beforeEach, describe, expect, it, vi } from 'vitest';
import { play, played, resetSound, setSoundsEnabled, type SoundName } from './sound';

function fakeCtx() {
  const node = () => ({ connect: vi.fn((n: unknown) => n), start: vi.fn(), stop: vi.fn() });
  const param = () => ({ value: 0, setValueAtTime: vi.fn(), linearRampToValueAtTime: vi.fn(), exponentialRampToValueAtTime: vi.fn() });
  const created = { osc: 0, src: 0 };
  class Ctx {
    state = 'running';
    currentTime = 1;
    sampleRate = 8000;
    destination = {};
    resume = vi.fn(async () => undefined);
    createGain() { return { ...node(), gain: param() }; }
    createOscillator() { created.osc++; return { ...node(), type: '', frequency: param() }; }
    createBiquadFilter() { return { ...node(), type: '', Q: { value: 0 }, frequency: param() }; }
    createBuffer(_c: number, n: number) { return { getChannelData: () => new Float32Array(n) }; }
    createBufferSource() { created.src++; return { ...node(), buffer: null }; }
  }
  vi.stubGlobal('AudioContext', Ctx);
  return created;
}

const ALL: SoundName[] = ['unlock', 'lock', 'wrong', 'done', 'error', 'delete', 'copy', 'totp', 'device'];

describe('sound', () => {
  beforeEach(() => {
    resetSound();
    vi.unstubAllGlobals();
  });

  it('every recipe renders nodes and is logged', () => {
    const made = fakeCtx();
    for (const n of ALL) play(n);
    expect(played).toEqual(ALL);
    expect(made.osc).toBeGreaterThan(ALL.length);
    expect(made.src).toBeGreaterThan(0);
  });

  it('is silent when disabled', () => {
    fakeCtx();
    setSoundsEnabled(false);
    play('done');
    expect(played).toEqual([]);
  });

  it('is silent while the window is hidden', () => {
    fakeCtx();
    vi.stubGlobal('document', { visibilityState: 'hidden' });
    play('done');
    expect(played).toEqual([]);
  });

  it('never throws without Web Audio or when the context fails', () => {
    vi.stubGlobal('AudioContext', undefined);
    expect(() => play('unlock')).not.toThrow();
    resetSound();
    vi.stubGlobal('AudioContext', class { constructor() { throw new Error('no device'); } });
    expect(() => play('unlock')).not.toThrow();
    expect(played).toEqual([]);
  });
});
