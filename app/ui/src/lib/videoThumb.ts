import { isTauri, mediaRelease, mediaUrl, onVaultLocked, previewUrl } from "./api";
import { fetchAsBlob } from "./mediaBlob";
import { isIos } from "./platform";
import type { FsNode, VaultId } from "./types";

const PREFIX = (isIos ? 6 : 12) * 1024 * 1024;
const WHOLE_MAX = (isIos ? 24 : 64) * 1024 * 1024;
const MAX_PX = isIos ? 256 : 512;
const ENTRIES = 300;
const TIMEOUT_MS = 10_000;

type Job = {
  key: string;
  vault: VaultId;
  node: FsNode;
  px: number;
  done: ((u: string | null) => void)[];
};

const cache = new Map<string, string | null>();
const queue: Job[] = [];
let running = false;

export const thumbKey = (vault: VaultId, n: FsNode) =>
  `${vault}:${n.id}:${n.mtime}:${n.size}`;

let lockHooked = false;
function hookLock() {
  if (lockHooked) return;
  lockHooked = true;
  void Promise.resolve(
    onVaultLocked((p) => {
      for (const [k, u] of cache) {
        if (k.startsWith(`${p.vault}:`)) {
          if (u) URL.revokeObjectURL(u);
          cache.delete(k);
        }
      }
    }),
  ).catch(() => undefined);
}

export function videoThumbSettled(vault: VaultId, n: FsNode): boolean {
  return cache.has(thumbKey(vault, n));
}

export function peekVideoThumb(vault: VaultId, n: FsNode): string | null {
  return cache.get(thumbKey(vault, n)) ?? null;
}

export function videoThumb(
  vault: VaultId,
  node: FsNode,
  px: number,
  signal?: AbortSignal,
): Promise<string | null> {
  hookLock();
  const key = thumbKey(vault, node);
  if (cache.has(key)) {
    const u = cache.get(key)!;
    cache.delete(key);
    cache.set(key, u);
    return Promise.resolve(u);
  }
  return new Promise((resolve) => {
    let job = queue.find((j) => j.key === key);
    if (job) {
      queue.splice(queue.indexOf(job), 1);
      job.done.push(resolve);
    } else
      job = {
        key,
        vault,
        node,
        px: Math.min(MAX_PX, Math.max(64, Math.round(px))),
        done: [resolve],
      };
    queue.push(job);
    const j = job;
    signal?.addEventListener("abort", () => {
      const i = j.done.indexOf(resolve);
      if (i >= 0) j.done.splice(i, 1);
      if (!j.done.length && queue.includes(j))
        queue.splice(queue.indexOf(j), 1);
      resolve(null);
    });
    pump();
  });
}

function idle(): Promise<void> {
  return new Promise((r) => {
    const ric = (
      globalThis as {
        requestIdleCallback?: (cb: () => void, o?: { timeout: number }) => void;
      }
    ).requestIdleCallback;
    if (ric) ric(() => r(), { timeout: 500 });
    else setTimeout(r, 60);
  });
}

async function pump() {
  if (running) return;
  running = true;
  try {
    while (queue.length) {
      await idle();
      const job = queue.pop();
      if (!job || !job.done.length) continue;
      let url: string | null = null;
      const ac = new AbortController();
      let timer: ReturnType<typeof setTimeout> | undefined;
      let late = false;
      try {
        url = await Promise.race([
          make(job, ac.signal),
          new Promise<null>((res) => (timer = setTimeout(() => ((late = true), ac.abort(), res(null)), TIMEOUT_MS))),
        ]);
      } catch {
        url = null;
      } finally {
        clearTimeout(timer);
      }
      if (late) {
        for (const d of job.done) d(null);
        continue;
      }
      cache.set(job.key, url);
      while (cache.size > ENTRIES) {
        const [k, u] = cache.entries().next().value as [string, string | null];
        if (u) URL.revokeObjectURL(u);
        cache.delete(k);
      }
      for (const d of job.done) d(url);
    }
  } finally {
    running = false;
  }
}

export function moovFirst(head: Uint8Array): boolean | null {
  const dv = new DataView(head.buffer, head.byteOffset, head.byteLength);
  let off = 0;
  while (off + 8 <= head.length) {
    let size = dv.getUint32(off);
    const type = String.fromCharCode(
      head[off + 4],
      head[off + 5],
      head[off + 6],
      head[off + 7],
    );
    if (type === "moov") return true;
    if (type === "mdat") return false;
    if (size === 1 && off + 16 <= head.length)
      size = Number(dv.getBigUint64(off + 8));
    if (size < 8) return null;
    off += size;
  }
  return null;
}

async function source(job: Job, signal?: AbortSignal): Promise<Blob | null> {
  const url = previewUrl(job.vault, job.node.id);
  if (!url) return null;
  const r = await fetch(url, { headers: { Range: `bytes=0-${PREFIX - 1}` }, signal });
  if (!r.ok) return null;
  const type = r.headers.get("content-type") ?? "";
  const buf = await r.arrayBuffer();
  if (r.status === 200) return new Blob([buf], { type });
  const total = Number(
    r.headers.get("content-range")?.split("/")[1] ?? job.node.size,
  );
  if (
    total > buf.byteLength &&
    moovFirst(new Uint8Array(buf, 0, Math.min(buf.byteLength, 1 << 20))) ===
      false
  ) {
    if (total > WHOLE_MAX) return null;
    return fetchAsBlob(url, total, signal);
  }
  return new Blob([buf], { type });
}

function once(el: HTMLMediaElement, ev: string, signal?: AbortSignal): Promise<void> {
  return new Promise((res, rej) => {
    const ok = () => {
      el.removeEventListener("error", bad);
      signal?.removeEventListener("abort", bad);
      res();
    };
    const bad = () => {
      el.removeEventListener(ev, ok);
      el.removeEventListener("error", bad);
      signal?.removeEventListener("abort", bad);
      rej(new Error("media"));
    };
    if (signal?.aborted) return bad();
    el.addEventListener(ev, ok, { once: true });
    el.addEventListener("error", bad, { once: true });
    signal?.addEventListener("abort", bad, { once: true });
  });
}

async function make(job: Job, signal: AbortSignal): Promise<string | null> {
  const r = await frame(job, signal, true);
  if (r !== "tainted") return r;
  return frame(job, signal, false).then((x) => (x === "tainted" ? null : x));
}

async function frame(job: Job, signal: AbortSignal, loopback: boolean): Promise<string | null | "tainted"> {
  let ticket: string | null = null;
  if (isTauri && loopback) {
    try {
      ticket = await mediaUrl(job.vault, job.node.id);
    } catch {
      ticket = null;
    }
  }
  let src: string;
  let isBlob = false;
  if (signal.aborted) {
    if (ticket) void Promise.resolve(mediaRelease(ticket)).catch(() => undefined);
    return null;
  }
  if (ticket) src = ticket;
  else {
    const blob = await source(job, signal);
    if (!blob) return null;
    src = URL.createObjectURL(blob);
    isBlob = true;
  }
  const v = document.createElement("video");
  v.muted = true;
  v.playsInline = true;
  v.setAttribute("playsinline", "");
  if (ticket) {
    v.crossOrigin = "anonymous";
    v.loop = true;
    v.preload = "metadata";
  } else {
    v.preload = "auto";
  }
  try {
    const work = (async () => {
      const meta = once(v, ticket ? "loadedmetadata" : "loadeddata", signal);
      v.src = src;
      await meta;
      const d = Number.isFinite(v.duration) ? v.duration : 0;
      const t = d > 0 ? Math.min(1, d * 0.1) : 0;
      if (t > 0 || ticket) {
        const seeked = once(v, "seeked", signal);
        v.currentTime = t;
        await seeked;
      }
      const w0 = v.videoWidth;
      const h0 = v.videoHeight;
      if (!w0 || !h0) return null;
      const k = Math.min(1, job.px / Math.max(w0, h0));
      const c = document.createElement("canvas");
      c.width = Math.max(1, Math.round(w0 * k));
      c.height = Math.max(1, Math.round(h0 * k));
      const g = c.getContext("2d");
      if (!g) return null;
      let out: Blob | null;
      try {
        g.drawImage(v, 0, 0, c.width, c.height);
        out = await new Promise<Blob | null>((res) =>
          c.toBlob(res, "image/jpeg", 0.8),
        );
      } catch {
        c.width = c.height = 0;
        return "tainted" as const;
      }
      c.width = c.height = 0;
      return out ? URL.createObjectURL(out) : null;
    })();
    try {
      return await work;
    } catch (e) {
      if (ticket && !signal.aborted) return "tainted";
      throw e;
    }
  } finally {
    v.removeAttribute("src");
    v.load();
    if (isBlob) URL.revokeObjectURL(src);
    if (ticket) void Promise.resolve(mediaRelease(ticket)).catch(() => undefined);
  }
}
