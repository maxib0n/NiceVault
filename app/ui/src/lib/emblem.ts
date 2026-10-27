import type { Emblem, RecentEntry } from './types';
import { m } from '../paraglide/messages.js';
import { DEFAULT_FRAME, frameOf, type FrameId } from '../ornament/frames';

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

export const MAX_IMAGE_BYTES = 64 * 1024;
export const IMAGE_SIZE = 256;
export const LOCKED_IMAGE_BYTES = 24 * 1024;
export const LOCKED_IMAGE_SIZE = 128;

export type EmblemView = Emblem | { kind: 'hidden' };

export function isKnownIcon(id: string): id is EmblemIconId {
  return EMBLEM_ICONS.some((i) => i.id === id);
}

export function iconLabel(id: string): string {
  return EMBLEM_ICONS.find((i) => i.id === id)?.label() ?? id;
}

export function initialOf(name: string): string {
  const mt = name.trim().match(/\p{L}|\p{N}/u);
  return mt ? mt[0].toLocaleUpperCase() : '·';
}

export function lockedView(r: Pick<RecentEntry, 'emblem' | 'show_when_locked'>): EmblemView {
  if (r.show_when_locked === false) return { kind: 'hidden' };
  return r.emblem ?? INITIAL;
}

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

export function lockedFrame(r: Pick<RecentEntry, 'emblem_frame' | 'show_when_locked'> | null | undefined): FrameId {
  return r && r.show_when_locked !== false ? frameOf(r.emblem_frame) : DEFAULT_FRAME;
}

export function lockedAccent(r: Pick<RecentEntry, 'accent' | 'show_when_locked'> | null | undefined): string | null {
  return r && r.show_when_locked !== false ? (r.accent ?? null) : null;
}

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
      if (blob.type !== 'image/webp') break;
    }
  }
  return null;
}
