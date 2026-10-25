// Search, filters and tags of ONE open volume: the filter chips'
// state (remembered per volume for the session), their translation into the backend's
// SearchFilters, and the volume's tags (tags_list, kept fresh on tags-changed by FileBrowser).
import * as api from '../api';
import { errorCode, errorText } from '../errors';
import { m } from '../../paraglide/messages.js';
import type { KindGroup, NodeId, SearchFilters, Tag, TagColor, TagId, VaultId } from '../types';

export type SizePreset = 'small' | 'medium' | 'large';
export type DatePreset = 'today' | 'week' | 'month' | 'year';
export type SearchScope = 'volume' | 'folder';

export interface UiFilters {
  kinds: KindGroup[];
  size: SizePreset | null;
  date: DatePreset | null;
  tags: TagId[];
  /** any of the tags instead of all of them */
  tagsAny: boolean;
}

export const NO_FILTERS: UiFilters = { kinds: [], size: null, date: null, tags: [], tagsAny: false };
export const SIZE_PRESETS: readonly SizePreset[] = ['small', 'medium', 'large'];
export const DATE_PRESETS: readonly DatePreset[] = ['today', 'week', 'month', 'year'];

export const hasFilters = (f: UiFilters) => f.kinds.length > 0 || f.size !== null || f.date !== null || f.tags.length > 0;

const MiB = 1024 * 1024;
const NS = 1_000_000;
const DAY = 86_400_000;

/** The backend's filters for `f` (dates from `now`, local time); `within` only for fs_search. */
export function apiFilters(f: UiFilters, within: NodeId | null = null, now: Date = new Date()): SearchFilters | null {
  const out: SearchFilters = {};
  if (f.kinds.length) out.kinds = [...f.kinds];
  if (f.size === 'small') out.max_size = MiB - 1;
  else if (f.size === 'medium') {
    out.min_size = MiB;
    out.max_size = 100 * MiB;
  } else if (f.size === 'large') out.min_size = 100 * MiB + 1;
  if (f.date) {
    const start =
      f.date === 'today'
        ? new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
        : f.date === 'year'
          ? new Date(now.getFullYear(), 0, 1).getTime()
          : now.getTime() - (f.date === 'week' ? 7 : 30) * DAY;
    out.modified_after = start * NS;
  }
  if (f.tags.length) {
    out.tags = [...f.tags];
    if (f.tagsAny) out.tags_any = true;
  }
  if (within !== null) out.within = within;
  return Object.keys(out).length ? out : null;
}

/** Filters / scope per volume (by path), for this session only. */
const remembered = new Map<string, { filters: UiFilters; scope: SearchScope }>();
export const rememberedFor = (path: string) => remembered.get(path) ?? { filters: NO_FILTERS, scope: 'volume' as SearchScope };
export const remember = (path: string, filters: UiFilters, scope: SearchScope) => remembered.set(path, { filters, scope });

/** "kinds:photos,videos;size:small;date:week;tags:1,2;any:1;scope:folder" (the ?filters= switch). */
export function parseFilterSwitch(s: string): { filters: UiFilters; scope: SearchScope | null } {
  const f: UiFilters = { ...NO_FILTERS, kinds: [], tags: [] };
  let scope: SearchScope | null = null;
  for (const part of s.split(';')) {
    const [k, v = ''] = part.split(':');
    const list = v.split(',').map((x) => x.trim()).filter(Boolean);
    if (k === 'kinds') f.kinds = list as KindGroup[];
    else if (k === 'size' && (SIZE_PRESETS as readonly string[]).includes(v)) f.size = v as SizePreset;
    else if (k === 'date' && (DATE_PRESETS as readonly string[]).includes(v)) f.date = v as DatePreset;
    else if (k === 'tags') f.tags = list.map(Number).filter((n) => Number.isInteger(n) && n > 0);
    else if (k === 'any') f.tagsAny = v !== '0';
    else if (k === 'scope' && (v === 'volume' || v === 'folder')) scope = v;
  }
  return { filters: f, scope };
}

/** A tag error in words (a name taken reads as a tag's, not a file's). */
export function tagErrorText(e: unknown): string {
  return errorCode(e) === 'exists' ? m.tag_err_exists() : errorText(e);
}

/** The volume's tags, by name, with their counts. */
export class VolumeTags {
  list = $state.raw<Tag[]>([]);
  loaded = $state(false);
  byId = $derived(new Map(this.list.map((t) => [t.id, t])));

  constructor(private readonly vault: VaultId) {}

  async load(): Promise<void> {
    try {
      this.list = await api.tagsList(this.vault);
      this.loaded = true;
    } catch {
      // locked meanwhile: the screen goes with it
    }
  }

  /** Throws the backend's error (the picker shows it under its field). */
  async create(name: string, color: TagColor): Promise<Tag> {
    const t = await api.tagsCreate(this.vault, name.trim(), color);
    await this.load();
    return t;
  }

  async update(tag: TagId, patch: { name?: string; color?: TagColor }): Promise<Tag> {
    const t = await api.tagsUpdate(this.vault, tag, { ...patch, name: patch.name?.trim() });
    await this.load();
    return t;
  }

  async remove(tag: TagId): Promise<void> {
    await api.tagsDelete(this.vault, tag);
    await this.load();
  }

  async assign(tags: TagId[], nodes: NodeId[], on: boolean): Promise<void> {
    if (!tags.length || !nodes.length) return;
    this.list = await (on ? api.tagsAssign : api.tagsUnassign)(this.vault, tags, nodes);
  }
}
