// Arrow-key seeking in a full-screen film: each press jumps, and presses in quick succession
// (or a held key) jump further: 10 10 10 20 20 30 30 60 60 60 … seconds.
export const SKIP_STEPS = [10, 10, 10, 20, 20, 30, 30, 60];
/** Presses closer than this continue the run; a longer pause starts again at 10 s. */
export const SKIP_RUN_MS = 700;

export function skipper(steps = SKIP_STEPS, runMs = SKIP_RUN_MS) {
  let n = 0;
  let last = -Infinity;
  let lastDir = 0;
  /** Seconds to jump for a press at `now` (ms) in direction `dir` (a turn starts over). */
  return (now: number, dir = 1): number => {
    n = now - last < runMs && dir === lastDir ? n + 1 : 0;
    last = now;
    lastDir = dir;
    return steps[Math.min(n, steps.length - 1)];
  };
}
