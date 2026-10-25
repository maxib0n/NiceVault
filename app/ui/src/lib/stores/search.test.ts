import { describe, expect, it } from 'vitest';
import { apiFilters, hasFilters, NO_FILTERS, parseFilterSwitch } from './search.svelte';

const MiB = 1024 * 1024;

describe('filter chips -> SearchFilters', () => {
  it('nothing chosen is no filter at all', () => {
    expect(hasFilters(NO_FILTERS)).toBe(false);
    expect(apiFilters(NO_FILTERS)).toBeNull();
  });

  it('size presets are inclusive byte bounds', () => {
    expect(apiFilters({ ...NO_FILTERS, size: 'small' })).toEqual({ max_size: MiB - 1 });
    expect(apiFilters({ ...NO_FILTERS, size: 'medium' })).toEqual({ min_size: MiB, max_size: 100 * MiB });
    expect(apiFilters({ ...NO_FILTERS, size: 'large' })).toEqual({ min_size: 100 * MiB + 1 });
  });

  it('dates count from local midnight / the first of January / now', () => {
    const now = new Date(2026, 9, 25, 15, 30);
    const ns = (d: Date) => d.getTime() * 1_000_000;
    expect(apiFilters({ ...NO_FILTERS, date: 'today' }, null, now)?.modified_after).toBe(ns(new Date(2026, 9, 25)));
    expect(apiFilters({ ...NO_FILTERS, date: 'year' }, null, now)?.modified_after).toBe(ns(new Date(2026, 0, 1)));
    expect(apiFilters({ ...NO_FILTERS, date: 'week' }, null, now)?.modified_after).toBe((now.getTime() - 7 * 86_400_000) * 1_000_000);
  });

  it('kinds, tags (all / any) and the search scope', () => {
    const f = { ...NO_FILTERS, kinds: ['photos' as const], tags: [2, 5], tagsAny: true };
    expect(apiFilters(f, 7)).toEqual({ kinds: ['photos'], tags: [2, 5], tags_any: true, within: 7 });
    expect(apiFilters({ ...f, tagsAny: false })).toEqual({ kinds: ['photos'], tags: [2, 5] });
  });

  it('the ?filters= switch', () => {
    expect(parseFilterSwitch('kinds:photos,videos;size:small;date:week;tags:1,2;any:1;scope:folder')).toEqual({
      filters: { kinds: ['photos', 'videos'], size: 'small', date: 'week', tags: [1, 2], tagsAny: true },
      scope: 'folder',
    });
    expect(parseFilterSwitch('size:huge;tags:x').filters).toEqual(NO_FILTERS);
  });
});
