const PROTO_NAMES = new Set(Object.getOwnPropertyNames(Object.prototype));

/**
 * The app runs with Object.prototype frozen (tauri.conf.json app.security.freezePrototype).
 * zxcvbn builds its ranked dictionaries with `result[word] = rank` on plain objects, and
 * assigning a name Object.prototype already has ("constructor", "toString", "valueOf", ...)
 * then throws in strict mode, so the estimate never resolved and no volume could be created.
 * Those few words are left out of the dictionaries (they still score as weak by other patterns).
 */
export function frozenSafe<T extends Record<string, unknown>>(dictionary: T): T {
  return Object.fromEntries(
    Object.entries(dictionary).map(([k, v]) => [k, Array.isArray(v) ? v.filter((w) => !PROTO_NAMES.has(w)) : v]),
  ) as T;
}
