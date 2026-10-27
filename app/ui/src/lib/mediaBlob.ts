import { isIos } from './platform';

export const MEDIA_BLOB_MAX = (isIos ? 48 : 512) * 1024 * 1024;
const CHUNK = 32 * 1024 * 1024;

export async function fetchAsBlob(url: string, size: number, signal?: AbortSignal): Promise<Blob> {
  const parts: ArrayBuffer[] = [];
  let type = '';
  for (let off = 0; off < Math.max(size, 1); off += CHUNK) {
    const end = Math.min(size, off + CHUNK) - 1;
    const r = await fetch(url, { headers: size > 0 ? { Range: `bytes=${off}-${end}` } : {}, signal });
    if (!r.ok) throw new Error(`preview ${r.status}`);
    type ||= r.headers.get('content-type') ?? '';
    const buf = await r.arrayBuffer();
    if (r.status === 200) return new Blob([buf], { type });
    parts.push(buf);
  }
  return new Blob(parts, { type });
}

export function isSourceRejected(el: HTMLMediaElement | undefined): boolean {
  return !!el?.error && el.error.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED;
}
