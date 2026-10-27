export const LONG_PRESS_MS = 460;
export const PRESS_SLOP = 10;

const editable = (t: Element) => !!t.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]');
const exempt = (t: Element) => editable(t) || !!t.closest('.quick-look, [data-no-longpress]');

export interface LongPressOptions {
  delay?: number;
  buzz?: () => void;
}

export function installLongPress(win: Window = window, opts: LongPressOptions = {}): () => void {
  const delay = opts.delay ?? LONG_PRESS_MS;
  const doc = win.document;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let start: { id: number; x: number; y: number; target: Element } | null = null;
  let pressed: Element | null = null;
  let touchUntil = 0;
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

const KB_MIN = 60;
const KB_AIR = 12;

const scrollable = (el: Element) => /(auto|scroll)/.test(getComputedStyle(el).overflowY);

function keepRect(field: HTMLElement, room: number): { top: number; bottom: number } {
  const f = field.getBoundingClientRect();
  const submit = field.closest('form')?.querySelector<HTMLElement>('[type="submit"]');
  const s = submit?.getClientRects().length ? submit.getBoundingClientRect() : null;
  if (s && s.bottom > f.bottom && s.bottom - f.top + KB_AIR * 2 <= room) return { top: f.top, bottom: s.bottom };
  return { top: f.top, bottom: f.bottom };
}

export function installKeyboardInset(win: Window = window): () => void {
  const vv = win.visualViewport;
  if (!vv) return () => {};
  const doc = win.document;
  const root = doc.documentElement;
  let raf = 0;
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
    root.style.setProperty('--nv-kb', `${kb > KB_MIN ? kb : 0}px`);
    root.toggleAttribute('data-keyboard', kb > KB_MIN);
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
