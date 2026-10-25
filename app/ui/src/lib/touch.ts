// Touch behaviour of the phone / tablet shell (installed by lib/mobile.ts, isMobile only):
//  - a long press is the context menu: iOS's WKWebView never sends `contextmenu` for a touch and
//    Android's comes late and from the OS; both are replaced by one of our own, sent to the element
//    under the finger after LONG_PRESS_MS without moving (a scroll or a lift cancels it), so the
//    components' own `oncontextmenu` handlers open their sheets. The click that follows the lift is
//    swallowed. While the finger rests, the element carries `data-pressing` (styles/mobile.css).
//  - the page never zooms: Safari's `gesture*` events (pinch on the page) are cancelled; the
//    picture viewer zooms with Pointer Events of its own (viewers/ImageViewer.svelte).
//  - the keyboard: `--nv-kb` on <html> is how much of the layout viewport the on-screen keyboard
//    covers (iOS does not resize the page for it; Android's activity already pads it away: 0).

/** How long a finger rests before it is a long press (iOS's own is about 500 ms). */
export const LONG_PRESS_MS = 460;
/** Movement that turns a press into a scroll (CSS px). */
export const PRESS_SLOP = 10;

const editable = (t: Element) => !!t.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]');
/** where a long press stays the platform's own: fields (the caret, Paste) and the preview */
const exempt = (t: Element) => editable(t) || !!t.closest('.quick-look, [data-no-longpress]');

export interface LongPressOptions {
  delay?: number;
  /** a short buzz where the platform offers one (Android) */
  buzz?: () => void;
}

export function installLongPress(win: Window = window, opts: LongPressOptions = {}): () => void {
  const delay = opts.delay ?? LONG_PRESS_MS;
  const doc = win.document;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let start: { id: number; x: number; y: number; target: Element } | null = null;
  let pressed: Element | null = null;
  /** a touch is down, or lifted a moment ago: the OS's own contextmenu is not ours */
  let touchUntil = 0;
  /** the click the lift of a long press makes */
  let swallowUntil = 0;

  const unpress = () => {
    if (pressed) delete (pressed as HTMLElement).dataset.pressing;
    pressed = null;
  };
  const cancel = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    start = null;
    unpress();
  };
  const fire = () => {
    timer = null;
    const s = start;
    start = null;
    unpress();
    if (!s || !s.target.isConnected) return;
    swallowUntil = Date.now() + 900;
    opts.buzz?.();
    s.target.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, composed: true, clientX: s.x, clientY: s.y, button: 2, buttons: 0, view: win }),
    );
  };

  const down = (e: PointerEvent) => {
    if (e.pointerType !== 'touch' && e.pointerType !== 'pen') return;
    touchUntil = Number.POSITIVE_INFINITY;
    cancel();
    const t = e.target as Element | null;
    if (!e.isPrimary || !t || exempt(t)) return;
    start = { id: e.pointerId, x: e.clientX, y: e.clientY, target: t };
    // the row / tile under the finger sinks a little while the press is held
    pressed = t.closest('.file-item, [role="option"], [role="treeitem"], [data-longpress]');
    if (pressed) (pressed as HTMLElement).dataset.pressing = '';
    timer = setTimeout(fire, delay);
  };
  const move = (e: PointerEvent) => {
    if (!start || e.pointerId !== start.id) return;
    if (Math.hypot(e.clientX - start.x, e.clientY - start.y) > PRESS_SLOP) cancel();
  };
  const up = (e: PointerEvent) => {
    if (e.pointerType !== 'touch' && e.pointerType !== 'pen') return;
    touchUntil = Date.now() + 800;
    if (start && e.pointerId === start.id) cancel();
  };
  const scroll = () => cancel();
  const menu = (e: MouseEvent) => {
    // ours are untrusted; the OS's (Android's long press) are not, and come while a touch is down
    if (e.isTrusted && Date.now() < touchUntil) {
      e.preventDefault();
      e.stopImmediatePropagation();
    }
  };
  const click = (e: MouseEvent) => {
    if (Date.now() < swallowUntil) {
      swallowUntil = 0;
      e.preventDefault();
      e.stopImmediatePropagation();
    }
  };
  const gesture = (e: Event) => e.preventDefault();

  win.addEventListener('pointerdown', down, { capture: true, passive: true });
  win.addEventListener('pointermove', move, { capture: true, passive: true });
  win.addEventListener('pointerup', up, { capture: true, passive: true });
  win.addEventListener('pointercancel', up, { capture: true, passive: true });
  win.addEventListener('scroll', scroll, { capture: true, passive: true });
  win.addEventListener('contextmenu', menu, true);
  win.addEventListener('click', click, true);
  for (const g of ['gesturestart', 'gesturechange', 'gestureend']) doc.addEventListener(g, gesture, { passive: false });
  return () => {
    cancel();
    win.removeEventListener('pointerdown', down, true);
    win.removeEventListener('pointermove', move, true);
    win.removeEventListener('pointerup', up, true);
    win.removeEventListener('pointercancel', up, true);
    win.removeEventListener('scroll', scroll, true);
    win.removeEventListener('contextmenu', menu, true);
    win.removeEventListener('click', click, true);
    for (const g of ['gesturestart', 'gesturechange', 'gestureend']) doc.removeEventListener(g, gesture);
  };
}

/** Keyboard heights below this are the browser's chrome settling, not a keyboard. */
const KB_MIN = 60;
/** Air kept between the field (and its form's key) and the top of the keyboard. */
const KB_AIR = 12;

const scrollable = (el: Element) => /(auto|scroll)/.test(getComputedStyle(el).overflowY);

/** The part of the page to keep above the keys: the field, with its form's submit key when it fits. */
function keepRect(field: HTMLElement, room: number): { top: number; bottom: number } {
  const f = field.getBoundingClientRect();
  const submit = field.closest('form')?.querySelector<HTMLElement>('[type="submit"]');
  const s = submit?.getClientRects().length ? submit.getBoundingClientRect() : null;
  if (s && s.bottom > f.bottom && s.bottom - f.top + KB_AIR * 2 <= room) return { top: f.top, bottom: s.bottom };
  return { top: f.top, bottom: f.bottom };
}

/**
 * `--nv-kb`: the part of the layout viewport under the on-screen keyboard (iOS keeps the page its
 * full height and slides the visual viewport; a sheet with a field rides on top of the keys).
 *
 * And the field with the caret stays above the keys (2026-10-25, iPhone: "mette tastiera, si alza
 * tutto e poi torna indietro"). iOS does not shrink the page for the keyboard: it slides the whole
 * page up to show the field, then slides it back once the keys are up, so the field and Sblocca
 * ended under the keyboard. Here, while the keyboard is up, the page's own scroller (the nearest
 * one round the field) gets the keyboard's height as room at its foot and is scrolled so the field
 * (and its form's key) sit in the visible part of the screen, wherever iOS has slid the viewport;
 * every resize / scroll of the visual viewport checks again.
 */
export function installKeyboardInset(win: Window = window): () => void {
  const vv = win.visualViewport;
  if (!vv) return () => {};
  const doc = win.document;
  const root = doc.documentElement;
  let raf = 0;
  /** the scroller given room at its foot, and its own padding */
  let roomy: { el: HTMLElement; pad: string } | null = null;
  const unroom = () => {
    if (roomy) roomy.el.style.paddingBottom = roomy.pad;
    roomy = null;
  };
  const follow = (keys: number) => {
    const field = doc.activeElement;
    if (keys <= KB_MIN || !(field instanceof HTMLElement) || !editable(field) || field.closest('dialog.m-rename')) return unroom();
    let sc: HTMLElement | null = field.parentElement;
    while (sc && sc !== doc.body && !scrollable(sc)) sc = sc.parentElement;
    if (!sc || sc === doc.body) return unroom();
    if (roomy?.el !== sc) {
      unroom();
      roomy = { el: sc, pad: sc.style.paddingBottom };
      const pad = Number.parseFloat(getComputedStyle(sc).paddingBottom) || 0;
      sc.style.paddingBottom = `${Math.round(pad + keys)}px`;
    }
    // the visible part of the page, in the page's (layout viewport's) coordinates
    const box = sc.getBoundingClientRect();
    const top = Math.max(vv.offsetTop, box.top) + KB_AIR;
    const bottom = Math.min(vv.offsetTop + vv.height, box.bottom) - KB_AIR;
    const k = keepRect(field, bottom - top);
    let d = 0;
    if (k.bottom > bottom) d = Math.min(k.bottom - bottom, k.top - top);
    else if (k.top < top) d = k.top - top;
    if (Math.abs(d) >= 1) sc.scrollTop += d;
  };
  const set = () => {
    raf = 0;
    const kb = Math.max(0, Math.round(win.innerHeight - vv.height - vv.offsetTop));
    // less than a toolbar's height is the browser's chrome settling, not a keyboard
    root.style.setProperty('--nv-kb', `${kb > KB_MIN ? kb : 0}px`);
    root.toggleAttribute('data-keyboard', kb > KB_MIN);
    // the keys' own height: iOS may have slid the viewport up by part of it (kb above is then less)
    follow(Math.round(win.innerHeight - vv.height));
  };
  const on = () => {
    if (!raf) raf = win.requestAnimationFrame(set);
  };
  vv.addEventListener('resize', on);
  vv.addEventListener('scroll', on);
  doc.addEventListener('focusin', on);
  doc.addEventListener('focusout', on);
  set();
  return () => {
    vv.removeEventListener('resize', on);
    vv.removeEventListener('scroll', on);
    doc.removeEventListener('focusin', on);
    doc.removeEventListener('focusout', on);
    if (raf) win.cancelAnimationFrame(raf);
    unroom();
  };
}
