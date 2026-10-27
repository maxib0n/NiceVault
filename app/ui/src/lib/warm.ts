import { mount, unmount, type Component } from 'svelte';

let lastInput = typeof performance === 'undefined' ? 0 : performance.now();
const QUIET = 2500;

type Step = [name: string, run: () => void];

const idle = (fn: () => void) => {
  const ric = (window as unknown as { requestIdleCallback?: (cb: () => void, o?: { timeout: number }) => number }).requestIdleCallback;
  if (ric) ric(fn, { timeout: 1000 });
  else setTimeout(fn, 50);
};

function runQuietly(steps: Step[]) {
  const next = () => {
    if (!steps.length) return;
    if (performance.now() - lastInput < QUIET || document.hidden) {
      setTimeout(next, QUIET);
      return;
    }
    idle(() => {
      if (performance.now() - lastInput < QUIET) return void setTimeout(next, QUIET);
      const [name, run] = steps.shift()!;
      const t = performance.now();
      run();
      ((window as unknown as { __nvWarm?: [string, number][] }).__nvWarm ??= []).push([name, Math.round(performance.now() - t)]);
      setTimeout(next, 120);
    });
  };
  setTimeout(next, QUIET);
}

function fontSteps(): Step[] {
  const steps: Step[] = [];
  const ctx = document.createElement('canvas').getContext('2d');
  if (!ctx || !document.fonts) return steps;
  const seen = new Set<string>();
  for (const f of document.fonts) {
    if (f.status !== 'loaded') continue;
    const family = f.family.replace(/^["']|["']$/g, '');
    const [lo, hi] = String(f.weight).split(/\s+/).map(Number);
    const weights = [400, 500, 600].filter((w) => w >= (lo || 400) && w <= (hi || lo || 400));
    for (const w of weights) {
      const key = `${f.style} ${w} ${family}`;
      if (seen.has(key)) continue;
      seen.add(key);
      for (const sizes of [[3.8, 9, 10.5, 11, 12, 13, 14, 16], [18, 20, 22, 24, 28, 30, 34, 44, 76]])
        steps.push([
          `font ${key} ${sizes[0]}`,
          () => {
            for (const px of sizes) {
              ctx.font = `${f.style === 'normal' ? '' : f.style + ' '}${w} ${px}px "${family}"`;
              ctx.measureText('Hg0<A');
            }
          },
        ]);
    }
  }
  return steps;
}

function warmChips(chips: Component) {
  const host = document.createElement('div');
  document.body.appendChild(host);
  const inst = mount(chips, { target: host });
  requestAnimationFrame(() =>
    requestAnimationFrame(() =>
      setTimeout(() => {
        void unmount(inst);
        host.remove();
      }, 0),
    ),
  );
}

let started = false;
export function warmUp(chips: Component): void {
  if (started || typeof window === 'undefined') return;
  started = true;
  for (const ev of ['pointerdown', 'keydown', 'wheel', 'pointermove'])
    window.addEventListener(ev, () => (lastInput = performance.now()), { capture: true, passive: true });
  runQuietly([
    ['chips', () => warmChips(chips)],
    ['fonts', () => void document.fonts?.ready.then(() => runQuietly(fontSteps()))],
  ]);
}
