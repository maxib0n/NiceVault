export const RATE_TAU_MS = 3000;
export const ETA_AFTER_MS = 2000;

export interface Rate {
  t0: number;
  t: number;
  done: number;
  rate: number | null;
}

export const startRate = (t: number, done = 0): Rate => ({ t0: t, t, done, rate: null });

export function feedRate(r: Rate, t: number, done: number): void {
  const dt = t - r.t;
  if (dt <= 0) return;
  const inst = Math.max(0, done - r.done) / (dt / 1000);
  const a = 1 - Math.exp(-dt / RATE_TAU_MS);
  r.rate = r.rate === null ? inst : r.rate + a * (inst - r.rate);
  r.t = t;
  r.done = done;
}

export function etaSeconds(r: Rate, total: number): number | null {
  if (r.rate === null || r.rate <= 0 || r.t - r.t0 < ETA_AFTER_MS || total <= r.done) return null;
  return (total - r.done) / r.rate;
}

export function etaParts(sec: number): { unit: 's' | 'min' | 'h'; n: number; min?: number } {
  if (sec < 60) return { unit: 's', n: Math.max(5, Math.round(sec / 5) * 5) };
  const min = Math.ceil(sec / 60);
  if (min < 90) return { unit: 'min', n: min };
  const h = Math.floor(min / 60);
  return { unit: 'h', n: h, min: min - h * 60 };
}
