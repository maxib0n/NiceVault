// Random access to a file's decrypted bytes over its preview URL (nvault:// in the app, the
// mock's /mock/* assets in a browser), one HTTP Range request at a time. Every viewer reads
// through this: bytes go straight from the store into memory, never to a temporary file.
//
// The backend answers an explicit range with 206 + Content-Range (capped at 64 MiB per
// request, preview.rs); a server that ignores Range (or a body shorter than asked) is
// handled by slicing what came back.

export interface ByteSource {
  /** total size in bytes (from Content-Range / Content-Length, not the listing) */
  size(): Promise<number>;
  /** bytes [start, end) — shorter at EOF */
  read(start: number, end: number, signal?: AbortSignal): Promise<Uint8Array>;
}

/** the largest single request a viewer makes (below preview.rs's MAX_BODY) */
export const MAX_REQUEST = 32 * 1024 * 1024;

function totalFrom(r: Response): number | null {
  const cr = r.headers.get('content-range');
  const m = cr && /\/(\d+)\s*$/.exec(cr);
  if (m) return Number(m[1]);
  if (r.status === 200) {
    const cl = r.headers.get('content-length');
    if (cl !== null && cl !== '') return Number(cl);
  }
  return null;
}

export function urlSource(url: string): ByteSource {
  let total: Promise<number> | null = null;
  /** a server that answered 200 to a Range request: the whole body, kept */
  let whole: Promise<Uint8Array> | null = null;

  async function fetchRange(start: number, endIncl: number, signal?: AbortSignal): Promise<Response> {
    const r = await fetch(url, { headers: { Range: `bytes=${start}-${endIncl}` }, signal, cache: 'no-store' });
    if (r.status === 416) return r;
    if (!r.ok) throw new Error(`preview ${r.status}`);
    return r;
  }

  const src: ByteSource = {
    size() {
      total ??= (async () => {
        const r = await fetchRange(0, 0);
        if (r.status === 416) return 0; // empty file
        const t = totalFrom(r);
        if (r.status === 200) {
          const buf = new Uint8Array(await r.arrayBuffer());
          whole = Promise.resolve(buf);
          return t ?? buf.length;
        }
        void r.body?.cancel().catch(() => undefined);
        return t ?? 0;
      })();
      // a failed probe may be retried
      total.catch(() => (total = null));
      return total;
    },
    async read(start, end, signal) {
      const size = await src.size();
      end = Math.min(end, size);
      if (start >= end) return new Uint8Array(0);
      if (whole) return (await whole).subarray(start, end);
      const out = new Uint8Array(end - start);
      let got = 0;
      while (got < out.length) {
        const s = start + got;
        const e = Math.min(end, s + MAX_REQUEST) - 1;
        const r = await fetchRange(s, e, signal);
        if (r.status === 416) break;
        const buf = new Uint8Array(await r.arrayBuffer());
        if (r.status === 200) {
          // Range ignored: this is the whole file
          whole = Promise.resolve(buf);
          return buf.subarray(start, end);
        }
        if (buf.length === 0) break;
        const n = Math.min(buf.length, out.length - got);
        out.set(buf.subarray(0, n), got);
        got += n;
      }
      return got === out.length ? out : out.subarray(0, got);
    },
  };
  return src;
}

/** A source over bytes already in memory (tests, nested zip entries). */
export function memorySource(bytes: Uint8Array): ByteSource {
  return {
    size: () => Promise.resolve(bytes.length),
    read: (s, e) => Promise.resolve(bytes.subarray(Math.max(0, s), Math.min(bytes.length, e))),
  };
}
