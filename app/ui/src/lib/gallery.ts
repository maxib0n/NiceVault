// "Salva nella Galleria" on phones (src-tauri/src/mobile.rs mobile_save_gallery): which files are pictures / videos the system gallery takes. The backend has the
// last word per platform (mobile_paths::gallery_kind: iOS Photos refuses WebM / Matroska / AVIF)
// and hands those back as `others`, exported to Files as before.
import type { FsNode } from './types';
import { extension } from './filetype';

const EXTS = new Set('jpg jpeg jfif png gif heic heif webp tif tiff dng bmp avif mp4 m4v mov 3gp webm mkv'.split(' '));

export function isGalleryMedia(n: Pick<FsNode, 'name' | 'kind'>): boolean {
  return n.kind === 'file' && EXTS.has(extension(n.name));
}
