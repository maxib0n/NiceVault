// The redraw clock for line work drawn on canvas. Inks are CSS tokens (accent,
// theme); canvases read them at draw time, so they must redraw whenever the
// theme, the accent, the device pixel ratio or the fonts change.

class Ink {
  /** bumps on anything that needs a redraw (colours, pixel ratio, fonts) */
  epoch = $state(0);
  /** bumps when web fonts arrive: only canvases that set text redraw for it */
  fontEpoch = $state(0);
  dpr = $state(typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1);
}

/** bumps only when colours may have changed (theme, accent, OS scheme): inkOf's cache key */
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
  // fonts arrive in bursts (one loadingdone per face): one redraw per frame at most
  let fontRaf = 0;
  const fontsChanged = () => (fontRaf ||= requestAnimationFrame(() => ((fontRaf = 0), ink.fontEpoch++)));
  document.fonts?.ready.then(fontsChanged);
  document.fonts?.addEventListener?.('loadingdone', fontsChanged);
}

/** inkOf results per element: valid while the colour epoch and the element's scope stay the same */
const resolved = new WeakMap<HTMLElement, { sig: string; colours: Map<string, string> }>();

/**
 * Resolves a CSS colour expression (e.g. "var(--engrave)") in the context of `el` to
 * a canvas-ready colour: the element's own colour is borrowed for a moment, so the
 * expression sees the same tokens (theme, [data-accent] subtree) as the element.
 * Transitions are held off meanwhile: under reduced motion every element transitions
 * every property for 0.01 ms (base.css), and a transitioning colour reads as the old one.
 */
export function inkOf(el: HTMLElement, expr: string): string {
  // cached per element (local token overrides stay right) for this colour epoch and the
  // theme / ink of its scope: a redraw for fonts, size or pixel ratio forces no style recalc
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

/** Reads a numeric custom property (e.g. --ink-relief) in the context of `el`. */
export function numberOf(el: Element, prop: string, fallback = 0): number {
  const v = parseFloat(getComputedStyle(el).getPropertyValue(prop));
  return Number.isFinite(v) ? v : fallback;
}

/** Hairline width in CSS px: one device pixel on HiDPI, a little heavier at 1x so it still reads as ink. */
export function hair(dpr: number, k = 1): number {
  return (dpr >= 2 ? 0.55 : 0.72) * k;
}
