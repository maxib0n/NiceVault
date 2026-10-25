// Volume emblem helpers (pure; the icon components are mapped in components/Emblem.svelte).
import type { Emblem, RecentEntry } from './types';
import { m } from '../paraglide/messages.js';
import { DEFAULT_FRAME, frameOf, type FrameId } from '../ornament/frames';

/** Curated icons for "things worth keeping": id = lucide icon name, label = its message. Order = picker order. */
export const EMBLEM_ICONS = [
  { id: 'house', label: m.icon_house },
  { id: 'file-text', label: m.icon_file_text },
  { id: 'camera', label: m.icon_camera },
  { id: 'briefcase', label: m.icon_briefcase },
  { id: 'users', label: m.icon_users },
  { id: 'heart-pulse', label: m.icon_heart_pulse },
  { id: 'car', label: m.icon_car },
  { id: 'plane', label: m.icon_plane },
  { id: 'wallet', label: m.icon_wallet },
  { id: 'piggy-bank', label: m.icon_piggy_bank },
  { id: 'landmark', label: m.icon_landmark },
  { id: 'receipt', label: m.icon_receipt },
  { id: 'key-round', label: m.icon_key_round },
  { id: 'heart', label: m.icon_heart },
  { id: 'star', label: m.icon_star },
  { id: 'graduation-cap', label: m.icon_graduation_cap },
  { id: 'baby', label: m.icon_baby },
  { id: 'paw-print', label: m.icon_paw_print },
  { id: 'book-open', label: m.icon_book_open },
  { id: 'gift', label: m.icon_gift },
  { id: 'shield', label: m.icon_shield },
  { id: 'gem', label: m.icon_gem },
  { id: 'leaf', label: m.icon_leaf },
  { id: 'mountain', label: m.icon_mountain },
] as const;

export type EmblemIconId = (typeof EMBLEM_ICONS)[number]['id'];

export const INITIAL: Emblem = { kind: 'initial' };

/** Max decoded size of a custom image (the backend refuses more). */
export const MAX_IMAGE_BYTES = 64 * 1024;
export const IMAGE_SIZE = 256;
/** The header (locked-screen) copy of an image emblem: nv-core caps (MAX_LOCKED_IMAGE / _SIDE). */
export const LOCKED_IMAGE_BYTES = 24 * 1024;
export const LOCKED_IMAGE_SIZE = 128;

/** What a mark shows: an emblem, or the neutral rosette (volume opted out while locked). */
export type EmblemView = Emblem | { kind: 'hidden' };

export function isKnownIcon(id: string): id is EmblemIconId {
  return EMBLEM_ICONS.some((i) => i.id === id);
}

export function iconLabel(id: string): string {
  return EMBLEM_ICONS.find((i) => i.id === id)?.label() ?? id;
}

/** First letter of the volume name (monogram), uppercase; "·" when the name has none. */
export function initialOf(name: string): string {
  const mt = name.trim().match(/\p{L}|\p{N}/u);
  return mt ? mt[0].toLocaleUpperCase() : '·';
}

/** Locked screens (Benvenuto, Sblocca): the header's emblem, else the initial; hidden when the look is encrypted. */
export function lockedView(r: Pick<RecentEntry, 'emblem' | 'show_when_locked'>): EmblemView {
  if (r.show_when_locked === false) return { kind: 'hidden' };
  return r.emblem ?? INITIAL;
}

/** Normalizes what the UI renders: unknown icon ids fall back to the initial. */
export function viewOf(e: EmblemView | null | undefined): EmblemView {
  if (!e) return INITIAL;
  if (e.kind === 'icon' && !isKnownIcon(e.id)) return INITIAL;
  return e;
}

export function imageMime(b64: string): string {
  return b64.startsWith('UklGR') ? 'image/webp' : 'image/png';
}

export function imageSrc(b64: string): string {
  return `data:${imageMime(b64)};base64,${b64}`;
}

export function sameEmblem(a: Emblem, b: Emblem): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === 'icon' && b.kind === 'icon') return a.id === b.id;
  if (a.kind === 'image' && b.kind === 'image') return a.data_base64 === b.data_base64;
  return true;
}

function bytesToBase64(bytes: Uint8Array): string {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/**
 * Locked screens: the frame round the emblem, as this computer last saw it open (the frame lives
 * inside the volume: RecentEntry.emblem_frame); the default rosette for an encrypted look or a
 * volume never opened here.
 */
export function lockedFrame(r: Pick<RecentEntry, 'emblem_frame' | 'show_when_locked'> | null | undefined): FrameId {
  return r && r.show_when_locked !== false ? frameOf(r.emblem_frame) : DEFAULT_FRAME;
}

/** Locked screens: the header's accent of a visible volume, else null (= the default accent). */
export function lockedAccent(r: Pick<RecentEntry, 'accent' | 'show_when_locked'> | null | undefined): string | null {
  return r && r.show_when_locked !== false ? (r.accent ?? null) : null;
}

/**
 * Browser only: centre-crops a picked image to a square, scales it to 256x256 and
 * encodes WebP (PNG where the webview can't encode WebP), shrinking until it fits 64 KB.
 * Returns base64 without the data: prefix.
 */
export async function imageFileToEmblem(file: Blob): Promise<string> {
  let bmp: ImageBitmap;
  try {
    bmp = await createImageBitmap(file);
  } catch {
    throw { code: 'image_unreadable', message: 'This image cannot be read.' };
  }
  const out = await encodeSquare(bmp, [IMAGE_SIZE, 192, 128], MAX_IMAGE_BYTES);
  bmp.close();
  if (out) return out;
  throw { code: 'image_complex', message: 'Image too complex: try another one.' };
}

/**
 * The header copy of an image emblem (base64 in, base64 out): <= 128 px, <= 24 KB WebP/PNG.
 * null where the platform can't decode/encode images (vitest) or nothing fits: the
 * backend then shows the initial on locked screens.
 */
export async function lockedImageOf(data_base64: string): Promise<string | null> {
  try {
    if (typeof createImageBitmap !== 'function' || typeof document === 'undefined') return null;
    const bin = atob(data_base64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const bmp = await createImageBitmap(new Blob([bytes], { type: imageMime(data_base64) }));
    const out = await encodeSquare(bmp, [LOCKED_IMAGE_SIZE, 96, 64], LOCKED_IMAGE_BYTES);
    bmp.close();
    return out;
  } catch {
    return null;
  }
}

/** Centre-crop to a square, try each size x quality until the encoding fits `maxBytes`. */
async function encodeSquare(bmp: ImageBitmap, sizes: number[], maxBytes: number): Promise<string | null> {
  const side = Math.min(bmp.width, bmp.height);
  const sx = (bmp.width - side) / 2;
  const sy = (bmp.height - side) / 2;
  for (const size of sizes) {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = size;
    const ctx = canvas.getContext('2d')!;
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(bmp, sx, sy, side, side, 0, 0, size, size);
    for (const q of [0.86, 0.72, 0.55]) {
      const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, 'image/webp', q));
      if (!blob) continue;
      if (blob.size <= maxBytes) return bytesToBase64(new Uint8Array(await blob.arrayBuffer()));
      if (blob.type !== 'image/webp') break; // PNG fallback ignores quality: try a smaller size
    }
  }
  return null;
}
