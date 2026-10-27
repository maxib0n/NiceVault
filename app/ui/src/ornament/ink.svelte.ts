class Ink {
  epoch = $state(0);
  fontEpoch = $state(0);
  dpr = $state(typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1);
}

let colourEpoch = 0;

export const ink = new Ink();

let installed = false;

export function installInk(): void {
  if (installed || typeof window === 'undefined') return;
  installed = true;
  const bump = () => ink.epoch++;
  const recolour = () => {
    colourEpoch++;
    bump();
  };
  new MutationObserver(recolour).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-accent'] });
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', recolour);
  const watchDpr = () => {
    const m = matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    m.addEventListener(
      'change',
      () => {
        ink.dpr = window.devicePixelRatio || 1;
        bump();
        watchDpr();
      },
      { once: true },
    );
  };
  watchDpr();
  let fontRaf = 0;
  const fontsChanged = () => (fontRaf ||= requestAnimationFrame(() => ((fontRaf = 0), ink.fontEpoch++)));
  document.fonts?.ready.then(fontsChanged);
  document.fonts?.addEventListener?.('loadingdone', fontsChanged);
}

const resolved = new WeakMap<HTMLElement, { sig: string; colours: Map<string, string> }>();

export function inkOf(el: HTMLElement, expr: string): string {
  const sig = `${colourEpoch}|${el.closest('[data-accent]')?.getAttribute('data-accent') ?? ''}|${el.closest('[data-theme]')?.getAttribute('data-theme') ?? ''}`;
  let entry = resolved.get(el);
  if (!entry || entry.sig !== sig) resolved.set(el, (entry = { sig, colours: new Map() }));
  const hit = entry.colours.get(expr);
  if (hit !== undefined) return hit;
  const c = resolve(el, expr);
  entry.colours.set(expr, c);
  return c;
}

function resolve(el: HTMLElement, expr: string): string {
  const prev = el.style.color;
  const tr = el.style.getPropertyValue('transition');
  const trPrio = el.style.getPropertyPriority('transition');
  el.style.setProperty('transition', 'none', 'important');
  el.style.color = expr;
  const c = getComputedStyle(el).color || '#000';
  el.style.color = prev;
  if (tr) el.style.setProperty('transition', tr, trPrio);
  else el.style.removeProperty('transition');
  return c;
}

export function numberOf(el: Element, prop: string, fallback = 0): number {
  const v = parseFloat(getComputedStyle(el).getPropertyValue(prop));
  return Number.isFinite(v) ? v : fallback;
}

export function hair(dpr: number, k = 1): number {
  return (dpr >= 2 ? 0.55 : 0.72) * k;
}
