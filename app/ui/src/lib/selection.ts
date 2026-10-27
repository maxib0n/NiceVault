export interface Selection<T = number> {
  readonly selected: ReadonlySet<T>;
  readonly anchor: T | null;
  readonly focus: T | null;
}

export interface Mods {
  shift?: boolean;
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

export function move<T>(s: Selection<T>, order: readonly T[], delta: number, mods: Mods = {}): Selection<T> {
  if (order.length === 0) return s;
  const cur = s.focus !== null ? order.indexOf(s.focus) : -1;
  let next: number;
  if (cur < 0) next = delta >= 0 ? 0 : order.length - 1;
  else next = Math.min(order.length - 1, Math.max(0, cur + delta));
  return moveTo(s, order, next, mods);
}

export function moveTo<T>(s: Selection<T>, order: readonly T[], index: number, mods: Mods = {}): Selection<T> {
  if (order.length === 0) return s;
  const i = Math.min(order.length - 1, Math.max(0, index));
  const id = order[i];
  if (mods.shift) {
    const anchor = s.anchor !== null && order.includes(s.anchor) ? s.anchor : (s.focus ?? id);
    return { selected: new Set(range(order, anchor, id)), anchor, focus: id };
  }
  if (mods.toggle) return { ...s, focus: id };
  return { selected: new Set([id]), anchor: id, focus: id };
}

export function toggleFocus<T>(s: Selection<T>): Selection<T> {
  if (s.focus === null) return s;
  return click(s, [], s.focus, { toggle: true });
}

export function prune<T>(s: Selection<T>, order: readonly T[]): Selection<T> {
  const present = new Set(order);
  const selected = new Set([...s.selected].filter((id) => present.has(id)));
  return {
    selected,
    anchor: s.anchor !== null && present.has(s.anchor) ? s.anchor : null,
    focus: s.focus !== null && present.has(s.focus) ? s.focus : null,
  };
}

export function ordered<T>(s: Selection<T>, order: readonly T[]): T[] {
  return order.filter((id) => s.selected.has(id));
}
