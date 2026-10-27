const DB = 'nicevault';
const STORE = 'pair';

function db() {
  return new Promise((resolve, reject) => {
    const r = indexedDB.open(DB, 1);
    r.onupgradeneeded = () => r.result.createObjectStore(STORE);
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

async function tx(mode, f) {
  const d = await db();
  try {
    return await new Promise((resolve, reject) => {
      const t = d.transaction(STORE, mode);
      const r = f(t.objectStore(STORE));
      t.oncomplete = () => resolve(r?.result);
      t.onerror = () => reject(t.error);
    });
  } finally {
    d.close();
  }
}

export const keystore = {
  get: () => tx('readonly', (s) => s.get('current')).then((v) => v ?? null),
  set: (v) => tx('readwrite', (s) => s.put(v, 'current')).then(() => undefined),
  clear: () => tx('readwrite', (s) => s.delete('current')).then(() => undefined),
};
