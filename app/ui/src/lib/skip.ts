export const SKIP_STEPS = [10, 10, 10, 20, 20, 30, 30, 60];
export const SKIP_RUN_MS = 700;

export function skipper(steps = SKIP_STEPS, runMs = SKIP_RUN_MS) {
  let n = 0;
  let last = -Infinity;
  let lastDir = 0;
  return (now: number, dir = 1): number => {
    n = now - last < runMs && dir === lastDir ? n + 1 : 0;
    last = now;
    lastDir = dir;
    return steps[Math.min(n, steps.length - 1)];
  };
}
