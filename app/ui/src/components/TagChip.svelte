<!--
  A tag as it is printed on an item: a small inked pill (its name), or only its dot. The colour
  is one of the volume inks ([data-accent] re-inks --accent for this element, in either theme);
  neutral (and a colour this build does not know) is drawn in the page's own grey ink.
  Hook: [data-testid=tag-chip][data-tag=<id>].
-->
<script lang="ts">
  import { tagColor } from '../lib/search';
  import type { Tag } from '../lib/types';

  let { tag, dot = false }: { tag: Pick<Tag, 'id' | 'name' | 'color'>; dot?: boolean } = $props();
  const ink = $derived(tagColor(tag.color));
</script>

<span
  class="tag-chip"
  class:dot
  data-testid="tag-chip"
  data-tag={tag.id}
  data-accent={ink === 'neutral' ? undefined : ink}
  data-neutral={ink === 'neutral' ? '' : undefined}
  title={tag.name}
>
  <span class="ink" aria-hidden="true"></span>{#if !dot}<span class="text">{tag.name}</span>{:else}<span class="visually-hidden">{tag.name}</span>{/if}
</span>

<style>
  .tag-chip {
    --tag-ink: var(--accent);
    --tag-wash: color-mix(in oklab, var(--accent) 13%, transparent);
    --tag-edge: color-mix(in oklab, var(--accent-line) 70%, transparent);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 100%;
    height: 16px;
    padding: 0 6px 0 5px;
    border-radius: var(--radius-pill);
    background: var(--tag-wash);
    box-shadow: inset 0 0 0 0.5px var(--tag-edge);
    color: var(--tag-ink);
    font-family: var(--font-ui);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 16px;
    letter-spacing: 0.01em;
    white-space: nowrap;
    vertical-align: middle;
  }
  .tag-chip[data-neutral] {
    --tag-ink: var(--ink-2);
    --tag-wash: color-mix(in oklab, var(--ink) 7%, transparent);
    --tag-edge: var(--paper-rule-2);
  }
  /* the ink itself: a small engraved bead */
  .ink {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--tag-ink);
    box-shadow: inset 0 -1px 0 rgb(0 0 0 / 0.18);
  }
  .tag-chip[data-neutral] .ink {
    background: var(--ink-3);
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tag-chip.dot {
    width: 8px;
    height: 8px;
    padding: 0;
    gap: 0;
    background: none;
    box-shadow: none;
  }
  .tag-chip.dot .ink {
    width: 8px;
    height: 8px;
    box-shadow:
      inset 0 -1px 0 rgb(0 0 0 / 0.2),
      0 0 0 1.5px var(--paper);
  }
</style>
