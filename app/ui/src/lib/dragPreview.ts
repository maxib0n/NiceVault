// The picture under the pointer while files are dragged (HTML5 drag & drop): a compact chip
// instead of the browser's snapshot of the whole tile / row (WebKitGTK showed that one at
// device scale, i.e. huge). One item: its picture at a small size with its name. Several:
// like the Finder, up to MAX_STACK pictures fanned behind the one under the pointer and a
// count in the volume's ink at the top right. Built from the items' own pictures (envelope,
// print, sheet) so it looks like what is being moved, and rendered by the browser at the
// device pixel ratio like any other element.
import '../styles/drag.css';
import type { NodeId } from './types';

export const MAX_STACK = 5;
/** CSS size of one picture in the preview. */
export const CARD_W = 60;
export const CARD_H = 45;

/** WebKitGTK paints a drag snapshot at device pixels but shows it as CSS pixels (2x too big on
 *  a HiDPI screen); scaling the preview down by the ratio there gives the intended size. */
function snapshotScale(): number {
  const dpr = window.devicePixelRatio || 1;
  const gtk = /Linux/i.test(navigator.userAgent) && !/Android|Chrome|Chromium|Firefox/i.test(navigator.userAgent) && '__TAURI_INTERNALS__' in window;
  return gtk && dpr > 1 ? 1 / dpr : 1;
}

/** A copy of item `id`'s picture at its size on screen, scaled to fit one card (so an envelope's
 *  seal, a print's border keep their proportions). The copy sits in a shell carrying the item's
 *  own classes and layout, so the view's scoped styles still apply to it. */
function pictureOf(id: NodeId): HTMLElement | null {
  const item = document.getElementById(`item-${id}`);
  const visual = item?.querySelector<HTMLElement>('.visual');
  if (!item || !visual) return null;
  const r = visual.getBoundingClientRect();
  const w = r.width || CARD_W;
  const h = r.height || CARD_H;
  const k = Math.min(CARD_W / w, CARD_H / h);
  const shell = document.createElement('div');
  shell.className = item.className;
  for (const c of ['selected', 'focused', 'drop-target']) shell.classList.remove(c);
  for (const [key, v] of Object.entries(item.dataset)) if (v !== undefined) shell.dataset[key] = v;
  // inline styles win over the tile / row rules: just the picture, at its on-screen size
  shell.style.cssText = `position:absolute;left:${(CARD_W - w * k) / 2}px;top:${(CARD_H - h * k) / 2}px;width:${w}px;height:${h}px;padding:0;margin:0;gap:0;display:block;min-height:0;background:none;box-shadow:none;border-radius:0;transform-origin:0 0;transform:scale(${k})`;
  const copy = visual.cloneNode(true) as HTMLElement;
  copy.querySelectorAll('[id]').forEach((e) => e.removeAttribute('id'));
  copy.style.cssText = `width:${w}px;height:${h}px;margin:0`;
  shell.append(copy);
  const card = document.createElement('div');
  card.className = 'drag-card';
  card.append(shell);
  return card;
}

let current: HTMLElement | null = null;

/** Padding of the chip around the front card (CSS px). */
function padding(many: boolean, back: number) {
  return many ? { top: 8 + back * 3 + 6, right: 10 + back * 5 + 12, bottom: 8, left: 8 } : { top: 6, right: 12, bottom: 6, left: 6 };
}

/** Builds the preview element (in the document, behind the app) without attaching it to a drag. */
export function buildDragPreview(ids: NodeId[], anchor: NodeId, name: string): HTMLElement {
  current?.remove();
  const order = [anchor, ...ids.filter((i) => i !== anchor)].slice(0, MAX_STACK);
  const root = document.createElement('div');
  root.className = 'nv-drag-preview';
  root.dataset.testid = 'drag-preview';
  root.dataset.count = String(ids.length);
  root.setAttribute('aria-hidden', 'true');
  const stack = document.createElement('div');
  stack.className = 'stack';
  // back to front: the anchor (under the pointer) last, so it paints on top
  order
    .map((id, i) => ({ el: pictureOf(id), i }))
    .reverse()
    .forEach(({ el, i }) => {
      if (!el) return;
      el.style.setProperty('--i', String(i));
      el.style.setProperty('--tilt', `${i === 0 ? 0 : (i % 2 ? 1 : -1) * (2 + i * 1.5)}deg`);
      stack.append(el);
    });
  root.append(stack);
  if (ids.length > 1) {
    const badge = document.createElement('span');
    badge.className = 'count';
    badge.textContent = ids.length > 999 ? '999+' : String(ids.length);
    root.append(badge);
  } else {
    const label = document.createElement('span');
    label.className = 'label';
    label.textContent = name;
    root.append(label);
  }
  // room for the fanned cards (up-right) and the count
  const back = Math.max(0, Math.min(ids.length, MAX_STACK) - 1);
  const pad = padding(ids.length > 1, back);
  root.style.padding = `${pad.top}px ${pad.right}px ${pad.bottom}px ${pad.left}px`;
  const s = snapshotScale();
  if (s !== 1) root.style.zoom = String(s);
  document.body.append(root);
  current = root;
  return root;
}

/** On dragstart: use the compact preview instead of the snapshot of the element. */
export function setDragPreview(e: DragEvent, ids: NodeId[], anchor: NodeId, name: string): void {
  if (!e.dataTransfer || typeof e.dataTransfer.setDragImage !== 'function') return;
  const root = buildDragPreview(ids, anchor, name);
  const s = snapshotScale();
  // the pointer holds the front picture by its middle
  const pad = padding(ids.length > 1, Math.max(0, Math.min(ids.length, MAX_STACK) - 1));
  e.dataTransfer.setDragImage(root, Math.round((pad.left + CARD_W / 2) * s), Math.round((pad.top + CARD_H / 2) * s));
  // the browser has taken its picture once the event returns; keep it a moment for slow snapshots
  const el = root;
  setTimeout(() => {
    el.remove();
    if (current === el) current = null;
  }, 1500);
}
