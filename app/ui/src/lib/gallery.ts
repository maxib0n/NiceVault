import type { FsNode } from './types';
import { extension } from './filetype';

const EXTS = new Set('jpg jpeg jfif png gif heic heif webp tif tiff dng bmp avif mp4 m4v mov 3gp webm mkv'.split(' '));

export function isGalleryMedia(n: Pick<FsNode, 'name' | 'kind'>): boolean {
  return n.kind === 'file' && EXTS.has(extension(n.name));
}
