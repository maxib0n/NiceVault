// Pure multi-selection logic for a list of items in display order.
// Mirrors common file-manager behaviour: click, shift-range, ctrl/cmd toggle,
// arrow navigation (with shift to extend, ctrl to move focus only), select all.

export interface Selection<T = number> {
  readonly selected: ReadonlySet<T>;
  /** fixed end of a shift-range */
  readonly anchor: T | null;
  /** keyboard cursor (also the moving end of a shift-range) */
  readonly focus: T | null;
}

export interface Mods {
  shift?: boolean;
  /** ctrl on Linux/Windows, cmd on macOS */
  toggle?: boolean;
}

export function empty<T>(): Selection<T> {
  return { selected: new Set(), anchor: null, focus: null };
}

function range<T>(order: readonly T[], a: T, b: T): T[] {
  const i = order.indexOf(a);
  const j = order.indexOf(b);
  if (i < 0 || j < 0) return j >= 0 ? [b] : [];
  const [lo, hi] = i <= j ? [i, j] : [j, i];
  return order.slice(lo, hi + 1);
}

/** Pointer click on `id`. */
export function click<T>(s: Selection<T>, order: readonly T[], id: T, mods: Mods = {}): Selection<T> {
  if (mods.shift) {
    const anchor = s.anchor !== null && order.includes(s.anchor) ? s.anchor : id;
    const r = range(order, anchor, id);
    const selected = new Set(mods.toggle ? [...s.selected, ...r] : r);
    return { selected, anchor, focus: id };
  }
  if (mods.toggle) {
    const selected = new Set(s.selected);
    if (selected.has(id)) selected.delete(id);
    else selected.add(id);
    return { selected, anchor: id, focus: id };
  }
  return { selected: new Set([id]), anchor: id, focus: id };
}

/** Select `id` only, unless it is already part of the selection (right-click / drag start behaviour). */
export function ensureSelected<T>(s: Selection<T>, id: T): Selection<T> {
  if (s.selected.has(id)) return { ...s, focus: id };
  return { selected: new Set([id]), anchor: id, focus: id };
}

export function selectAll<T>(s: Selection<T>, order: readonly T[]): Selection<T> {
  return {
    selected: new Set(order),
    anchor: s.anchor ?? order[0] ?? null,
    focus: s.focus ?? order[0] ?? null,
  };
}

export function clear<T>(s: Selection<T>): Selection<T> {
  return { selected: new Set(), anchor: s.focus, focus: s.focus };
}

/** Move the keyboard cursor by `delta` items (±1 in a list, ±columns for up/down in a grid). */
export function move<T>(s: Selection<T>, order: readonly T[], delta: number, mods: Mods = {}): Selection<T> {
  if (order.length === 0) return s;
  const cur = s.focus !== null ? order.indexOf(s.focus) : -1;
  let next: number;
  if (cur < 0) next = delta >= 0 ? 0 : order.length - 1;
  else next = Math.min(order.length - 1, Math.max(0, cur + delta));
  return moveTo(s, order, next, mods);
}

/** Jump the cursor to an index (Home/End/PageUp...). */
export function moveTo<T>(s: Selection<T>, order: readonly T[], index: number, mods: Mods = {}): Selection<T> {
  if (order.length === 0) return s;
  const i = Math.min(order.length - 1, Math.max(0, index));
  const id = order[i];
  if (mods.shift) {
    const anchor = s.anchor !== null && order.includes(s.anchor) ? s.anchor : (s.focus ?? id);
    return { selected: new Set(range(order, anchor, id)), anchor, focus: id };
  }
  if (mods.toggle) return { ...s, focus: id }; // move cursor only; Space toggles
  return { selected: new Set([id]), anchor: id, focus: id };
}

/** Toggle the item under the cursor (ctrl+Space). */
export function toggleFocus<T>(s: Selection<T>): Selection<T> {
  if (s.focus === null) return s;
  return click(s, [], s.focus, { toggle: true });
}

/** Drop ids that no longer exist after a refresh. */
export function prune<T>(s: Selection<T>, order: readonly T[]): Selection<T> {
  const present = new Set(order);
  const selected = new Set([...s.selected].filter((id) => present.has(id)));
  return {
    selected,
    anchor: s.anchor !== null && present.has(s.anchor) ? s.anchor : null,
    focus: s.focus !== null && present.has(s.focus) ? s.focus : null,
  };
}

/** Selected ids in display order. */
export function ordered<T>(s: Selection<T>, order: readonly T[]): T[] {
  return order.filter((id) => s.selected.has(id));
}
