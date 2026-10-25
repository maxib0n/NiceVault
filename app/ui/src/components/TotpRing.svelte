<!--
  The live 2FA code of an entry: the code in the mono (grouped 3+3 / 4+4), set large enough to
  be read across a desk, and beside it a small dial: its bezel ticked every five seconds, the
  arc of the time left draining round it, the seconds in its middle (in the danger ink for the
  last five). Asks the backend for the code at each new period (pm_totp); counts down locally in
  between. A new code decodes from glyphs as it turns over (lib/reveal.ts; the first one just
  shows). Hooks: [data-testid=totp-code], data-remaining.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import * as api from '../lib/api';
  import type { EntryId, PmTotpCode } from '../lib/api';
  import { decode } from '../lib/reveal';
  import type { VaultId } from '../lib/types';
  import { m } from '../paraglide/messages.js';

  let { vault, id, size = 32 }: { vault: VaultId; id: EntryId; size?: number } = $props();

  let code = $state<PmTotpCode | null>(null);
  /** codes seen for this entry: the first one shows, the ones after it decode */
  let turned = $state(0);
  let fetchedAt = 0;
  let now = $state(Date.now());
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    try {
      const c = await api.pmTotp(vault, id);
      fetchedAt = Date.now();
      if (code && c.code !== code.code) turned++;
      code = c;
    } catch {
      code = null;
    }
  }

  $effect(() => {
    void id;
    code = null;
    turned = 0;
    void refresh();
    clearInterval(timer);
    timer = setInterval(() => {
      now = Date.now();
      if (code && remaining <= 0) void refresh();
    }, 250);
    return () => clearInterval(timer);
  });
  onDestroy(() => clearInterval(timer));

  const remaining = $derived(code ? Math.max(0, code.remaining - Math.floor((now - fetchedAt) / 1000)) : 0);
  const frac = $derived(code ? remaining / code.period : 0);
  const grouped = $derived(code ? (code.code.length > 6 ? `${code.code.slice(0, 4)} ${code.code.slice(4)}` : `${code.code.slice(0, 3)} ${code.code.slice(3)}`) : '— — —');
  const low = $derived(code !== null && remaining <= 5);
  const R = 13;
  const C = 2 * Math.PI * R;
  // the bezel: a tick every five seconds of the period
  const ticks = $derived(Math.max(6, Math.round((code?.period ?? 30) / 5)));
</script>

<span class="totp" data-remaining={remaining} data-low={low}>
  <span class="code mono" data-testid="totp-code" aria-label={m.pm_totp()}
    >{#key code?.code}{#if turned > 0}<span use:decode={{ key: `totp:${id}:${code?.code}:${turned}`, speed: 2.4 }}>{grouped}</span>{:else}{grouped}{/if}{/key}</span
  >
  <svg class="dial" width={size} height={size} viewBox="0 0 36 36" role="img" aria-label={m.pm_totp_left({ count: remaining, n: String(remaining) })}>
    {#each { length: ticks } as _, i (i)}
      {@const a = (i / ticks) * Math.PI * 2 - Math.PI / 2}
      <line class="tick" x1={18 + 16.2 * Math.cos(a)} y1={18 + 16.2 * Math.sin(a)} x2={18 + 17.4 * Math.cos(a)} y2={18 + 17.4 * Math.sin(a)} />
    {/each}
    <circle class="track" cx="18" cy="18" r={R} />
    <circle class="left" cx="18" cy="18" r={R} stroke-dasharray={C} stroke-dashoffset={C * (1 - frac)} transform="rotate(-90 18 18)" />
    <text x="18" y="18.4" class="secs" text-anchor="middle" dominant-baseline="central">{code ? remaining : ''}</text>
  </svg>
</span>

<style>
  .totp {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 14px;
    max-width: 100%;
  }
  .code {
    white-space: nowrap;
    font-size: clamp(22px, 2vw, 27px);
    font-weight: 500;
    line-height: 1.1;
    letter-spacing: 0.07em;
    font-variant-numeric: tabular-nums;
    color: var(--title-ink);
  }
  .dial {
    flex: none;
    overflow: visible;
  }
  .tick {
    stroke: var(--ink-4);
    stroke-width: 1;
    stroke-linecap: round;
  }
  .track {
    fill: var(--paper-sunk);
    stroke: var(--paper-rule-2);
    stroke-width: 2;
  }
  .left {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2.4;
    stroke-linecap: round;
    transition: stroke-dashoffset 250ms linear;
  }
  .totp[data-low='true'] .left {
    stroke: var(--danger);
  }
  .secs {
    font-family: var(--font-mono);
    font-size: 10.5px;
    font-weight: 600;
    fill: var(--ink-2);
  }
  .totp[data-low='true'] .secs {
    fill: var(--danger);
  }
  @media (prefers-reduced-motion: reduce) {
    .left {
      transition: none;
    }
  }
</style>
