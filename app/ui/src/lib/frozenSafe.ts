const PROTO_NAMES = new Set(Object.getOwnPropertyNames(Object.prototype));

export function frozenSafe<T extends Record<string, unknown>>(dictionary: T): T {
  return Object.fromEntries(
    Object.entries(dictionary).map(([k, v]) => [k, Array.isArray(v) ? v.filter((w) => !PROTO_NAMES.has(w)) : v]),
  ) as T;
}
