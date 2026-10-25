// Sorting of directory entries. Folders always first.
import type { FsNode } from './types';
import { extension } from './filetype';
import { i18n } from './i18n.svelte';
import { m } from '../paraglide/messages.js';

export type SortKey = 'name' | 'size' | 'mtime' | 'type';
export type SortDir = 'asc' | 'desc';
export interface SortSpec {
  key: SortKey;
  dir: SortDir;
}

let cached: { tag: string; c: Intl.Collator } | null = null;
const collator = () => {
  if (cached?.tag !== i18n.tag) cached = { tag: i18n.tag, c: new Intl.Collator(i18n.tag, { numeric: true, sensitivity: 'base' }) };
  return cached.c;
};

export function sortNodes(nodes: readonly FsNode[], spec: SortSpec): FsNode[] {
  const sign = spec.dir === 'asc' ? 1 : -1;
  const cmp = (a: FsNode, b: FsNode): number => {
    if (a.kind !== b.kind) return a.kind === 'dir' ? -1 : 1;
    let r = 0;
    if (spec.key === 'size') r = a.size - b.size;
    else if (spec.key === 'mtime') r = a.mtime - b.mtime;
    else if (spec.key === 'type') r = collator().compare(extension(a.name), extension(b.name));
    if (r === 0) r = collator().compare(a.name, b.name);
    return r * sign;
  };
  return [...nodes].sort(cmp);
}

const SORT_TEXT: Record<SortKey, () => string> = {
  name: m.sort_name,
  size: m.sort_size,
  mtime: m.sort_mtime,
  type: m.sort_type,
};

export const sortLabel = (k: SortKey) => SORT_TEXT[k]();
