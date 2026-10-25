// Fallback for <video>/<audio> previews that the webview refuses to stream
// from the nvault:// scheme. WebKitGTK (Linux) reports MEDIA_ERR_SRC_NOT_SUPPORTED
// for media on a custom URI scheme even though fetch() of the same URL works
// (Range included), so on that error the decrypted bytes are pulled with ranged
// fetches into an in-memory Blob and played from a blob: URL. Plaintext still
// never touches the disk; memory is bounded by MEDIA_BLOB_MAX.

import { isIos } from './platform';

/** iOS: media streams from the loopback server (viewers.rs); a Blob that large in the web
 *  content process gets it killed for memory, so the fallback stays small there. */
export const MEDIA_BLOB_MAX = (isIos ? 48 : 512) * 1024 * 1024;
/** below the backend's 64 MiB cap for explicit ranges */
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
    if (r.status === 200) return new Blob([buf], { type }); // the whole file came back at once
    parts.push(buf);
  }
  return new Blob(parts, { type });
}

/** The error that means "this element can't load that URL" rather than a broken file. */
export function isSourceRejected(el: HTMLMediaElement | undefined): boolean {
  return !!el?.error && el.error.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED;
}
