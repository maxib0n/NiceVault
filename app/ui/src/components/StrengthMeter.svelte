<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { estimate, MIN_SCORE, scoreLabel, type Strength } from '../lib/strength';
  import { i18n } from '../lib/i18n.svelte';

  let {
    password,
    userInputs = [],
    strength = $bindable(null),
    id,
  }: {
    password: string;
    userInputs?: string[];
    strength?: Strength | null;
    id?: string;
  } = $props();

  let seq = 0;
  $effect(() => {
    const pw = password;
    const inputs = userInputs;
    void i18n.current;
    const mine = ++seq;
    if (!pw) {
      strength = null;
      return;
    }
    void estimate(pw, inputs).then((s) => {
      if (mine === seq) strength = s;
    });
  });
</script>

<div class="strength" {id} data-score={strength?.score ?? -1} aria-live="polite">
  <div
    class="bar"
    role="meter"
    aria-label={m.sm_label()}
    aria-valuemin={0}
    aria-valuemax={4}
    aria-valuenow={strength?.score ?? 0}
    aria-valuetext={strength ? scoreLabel(strength.score) : m.sm_none()}
  >
    {#each [0, 1, 2, 3] as i (i)}
      <span class="seg" class:on={strength !== null && strength.score > i}></span>
    {/each}
  </div>
  {#if strength}
    <p class="summary">
      <span class="verdict">{scoreLabel(strength.score)}</span>
      <span class="time">{m.sm_time()} <span class="num">{strength.crackTime}</span></span>
    </p>
    {#if strength.warning}
      <p class="warning hint">{strength.warning}</p>
    {:else if !strength.acceptable}
      <p class="warning hint">
        {password.length < 8 ? m.sm_min8() : m.sm_easy()}
      </p>
    {/if}
    {#if !strength.acceptable && strength.suggestions.length}
      <p class="suggestion hint">{strength.suggestions[0]}</p>
    {/if}
  {:else}
    <p class="summary hint">{m.sm_need({ level: scoreLabel(MIN_SCORE).toLowerCase() })}</p>
  {/if}
</div>

<style>
  .strength {
    --meter: var(--ink-4);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .strength[data-score='0'],
  .strength[data-score='1'],
  .strength[data-score='2'] {
    --meter: var(--danger);
  }
  .strength[data-score='3'],
  .strength[data-score='4'] {
    --meter: var(--accent);
  }
  .bar {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 4px;
  }
  .seg {
    height: 6px;
    border-radius: 2px;
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 1px var(--field-shade),
      0 1px 0 var(--paper-hi);
  }
  .seg.on {
    background: linear-gradient(180deg, color-mix(in oklab, var(--meter) 82%, #fff), var(--meter));
    box-shadow: 0 1px 0 var(--paper-hi);
  }
  .summary {
    font-size: var(--text-sm);
    color: var(--ink-3);
  }
  .verdict {
    font-weight: 600;
    color: var(--ink);
  }
  .strength[data-score='0'] .verdict,
  .strength[data-score='1'] .verdict,
  .strength[data-score='2'] .verdict {
    color: var(--danger);
  }
  .strength[data-score='3'] .verdict,
  .strength[data-score='4'] .verdict {
    color: var(--accent);
  }
  .warning,
  .suggestion {
    font-size: var(--text-sm);
  }
</style>
