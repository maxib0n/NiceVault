// Subtle UI sounds, synthesized in the webview with the Web Audio API: no audio files, no
// dependencies. The recipes (sine partials with a soft attack and an exponential decay, a
// filtered noise burst for the mechanical bits, low "executive" chords) are adapted from the
// an earlier terminal synth (ADSR partial banks, Caldo/Marimba packs). Sober by design:
// 40-400 ms, soft, warm, low master volume. Never blocks the UI: scheduling is a handful of
// nodes; the AudioContext is created lazily on the first play (always inside a user gesture
// or right after one); everything fails silently (no audio device, no Web Audio).

export type SoundName = 'unlock' | 'lock' | 'wrong' | 'done' | 'error' | 'delete' | 'copy' | 'totp' | 'device';

const MASTER = 0.16;

let enabled = true;
let ctx: AudioContext | null = null;
let master: GainNode | null = null;
let noiseBuf: AudioBuffer | null = null;

/** Names played (newest last, capped): lets tests and the e2e mock assert what played. */
export const played: SoundName[] = [];

export function setSoundsEnabled(on: boolean): void {
  enabled = on;
}
export function soundsEnabled(): boolean {
  return enabled;
}

function audio(): AudioContext | null {
  if (ctx) return ctx;
  try {
    const C = globalThis.AudioContext ?? (globalThis as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!C) return null;
    ctx = new C();
    master = ctx.createGain();
    master.gain.value = MASTER;
    master.connect(ctx.destination);
    return ctx;
  } catch {
    return null;
  }
}

/** A soft bell/marimba-like note: sine partials (mult, amp, decay-share), 6 ms-ish attack. */
function note(c: AudioContext, t: number, hz: number, dur: number, gain = 0.5, partials: [number, number, number][] = [[1, 1, 1], [2, 0.18, 0.5], [3.01, 0.06, 0.3]], glideTo?: number): void {
  for (const [mult, amp, share] of partials) {
    const o = c.createOscillator();
    const g = c.createGain();
    o.type = 'sine';
    o.frequency.setValueAtTime(hz * mult, t);
    if (glideTo) o.frequency.exponentialRampToValueAtTime(glideTo * mult, t + dur);
    const end = t + dur * share + 0.02;
    g.gain.setValueAtTime(0.0001, t);
    g.gain.linearRampToValueAtTime(gain * amp, t + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, end);
    o.connect(g).connect(master!);
    o.start(t);
    o.stop(end + 0.02);
  }
}

/** A short filtered noise burst: latches, clicks, paper. */
function burst(c: AudioContext, t: number, dur: number, hz: number, q: number, gain: number, toHz = hz): void {
  noiseBuf ??= (() => {
    const b = c.createBuffer(1, Math.floor(c.sampleRate * 0.5), c.sampleRate);
    const d = b.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
    return b;
  })();
  const s = c.createBufferSource();
  s.buffer = noiseBuf;
  const f = c.createBiquadFilter();
  f.type = 'bandpass';
  f.Q.value = q;
  f.frequency.setValueAtTime(hz, t);
  if (toHz !== hz) f.frequency.exponentialRampToValueAtTime(toHz, t + dur);
  const g = c.createGain();
  g.gain.setValueAtTime(gain, t);
  g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
  s.connect(f).connect(g).connect(master!);
  s.start(t, Math.random() * 0.2);
  s.stop(t + dur + 0.02);
}

const PURE: [number, number, number][] = [[1, 1, 1]];
const BELL: [number, number, number][] = [[1, 1, 1], [4.0, 0.12, 0.35]];

const RECIPES: Record<SoundName, (c: AudioContext, t: number) => void> = {
  // soft mechanical latch + low warm chord (D3 A3 F#4)
  unlock: (c, t) => {
    burst(c, t, 0.03, 2400, 3, 0.7);
    note(c, t, 80, 0.12, 0.9, PURE, 52);
    for (const [i, hz] of [146.83, 220, 369.99].entries()) note(c, t + 0.09 + i * 0.045, hz, 0.38, 0.32, [[1, 1, 1], [2, 0.15, 0.6]]);
  },
  // latch closing: a duller double knock
  lock: (c, t) => {
    burst(c, t, 0.028, 1500, 2.5, 0.6);
    note(c, t, 70, 0.1, 0.9, PURE, 46);
    burst(c, t + 0.075, 0.03, 2000, 3, 0.45);
    note(c, t + 0.075, 98, 0.16, 0.5, PURE, 73);
  },
  // muted low double tick
  wrong: (c, t) => {
    for (const d of [0, 0.085]) {
      note(c, t + d, 140, 0.07, 0.8, PURE, 105);
      burst(c, t + d, 0.02, 700, 1.5, 0.3);
    }
  },
  // gentle two-note chime (G5, D6)
  done: (c, t) => {
    note(c, t, 783.99, 0.34, 0.34, BELL);
    note(c, t + 0.11, 1174.66, 0.3, 0.26, BELL);
  },
  // soft low falling tone
  error: (c, t) => note(c, t, 196, 0.26, 0.55, [[1, 1, 1], [2, 0.1, 0.5]], 164.81),
  // paper rustle: two short noise sweeps
  delete: (c, t) => {
    burst(c, t, 0.16, 3600, 0.8, 0.5, 1400);
    burst(c, t + 0.08, 0.12, 5200, 0.7, 0.3, 2400);
  },
  copy: (c, t) => note(c, t, 2200, 0.04, 0.22, PURE),
  totp: (c, t) => {
    note(c, t, 2200, 0.04, 0.22, PURE);
    note(c, t + 0.06, 3300, 0.05, 0.18, PURE);
  },
  // three rising soft notes (G5 B5 D6) for a stick / device operation
  device: (c, t) => {
    for (const [i, hz] of [783.99, 987.77, 1174.66].entries()) note(c, t + i * 0.1, hz, 0.34, 0.3, BELL);
  },
};

/** Play a sound if sounds are on and the window is visible. Fire and forget: never throws. */
export function play(name: SoundName): void {
  try {
    if (!enabled) return;
    if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
    // automated runs (Playwright, WebDriver e2e:real) log the sound but stay silent
    if (typeof navigator !== 'undefined' && navigator.webdriver) {
      played.push(name);
      if (played.length > 50) played.shift();
      return;
    }
    const c = audio();
    if (!c) return;
    if (c.state === 'suspended') void c.resume().catch(() => undefined);
    RECIPES[name](c, c.currentTime + 0.005);
    played.push(name);
    if (played.length > 50) played.shift();
  } catch {
    /* no audio: silence */
  }
}

/** For tests: forget the context and the log. */
export function resetSound(): void {
  ctx = null;
  master = null;
  noiseBuf = null;
  enabled = true;
  played.length = 0;
}

// the e2e mock reads what played from the page
if (typeof window !== 'undefined') (window as unknown as { __nvSounds?: SoundName[] }).__nvSounds = played;
