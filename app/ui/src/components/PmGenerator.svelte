<script lang="ts">
  import { Copy, RefreshCw } from '@lucide/svelte';
  import * as api from '../lib/api';
  import type { PmGenOptions } from '../lib/api';
  import { errorText } from '../lib/errors';
  import { radioKeys } from '../lib/radio';
  import { decode } from '../lib/reveal';
  import { toasts } from '../lib/stores/toasts.svelte';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';

  let { open, onclose, onuse }: { open: boolean; onclose: () => void; onuse?: (value: string) => void } = $props();

  type Kind = 'random' | 'passphrase';
  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem('nv.pm.gen') ?? 'null') as Record<string, unknown> | null;
    } catch {
      return null;
    }
  })();
  let kind = $state<Kind>(saved?.kind === 'passphrase' ? 'passphrase' : 'random');
  let length = $state(Number(saved?.length) || 20);
  let lower = $state(saved?.lower !== false);
  let upper = $state(saved?.upper !== false);
  let digits = $state(saved?.digits !== false);
  let symbols = $state(saved?.symbols !== false);
  let ambiguous = $state(saved?.ambiguous === true);
  let words = $state(Number(saved?.words) || 5);
  let language = $state<'it' | 'en'>(saved?.language === 'en' ? 'en' : 'it');
  let separator = $state(typeof saved?.separator === 'string' ? (saved.separator as string) : '-');
  let capitalize = $state(saved?.capitalize === true);
  let number = $state(saved?.number === true);

  let value = $state('');
  let rolled = $state(0);
  let bits = $state(0);
  let error = $state<string | null>(null);
  let seq = 0;

  const opts = $derived<PmGenOptions>(
    kind === 'random'
      ? { kind, length, lower, upper, digits, symbols, exclude_ambiguous: ambiguous }
      : { kind, words, language, separator: separator.slice(0, 3), capitalize, number },
  );

  async function generate(roll = false) {
    const mine = ++seq;
    try {
      const r = await api.pmGenerate($state.snapshot(opts) as PmGenOptions);
      if (mine !== seq) return;
      if (roll) rolled++;
      value = r.value;
      bits = r.entropy_bits;
      error = null;
    } catch (e) {
      if (mine === seq) error = errorText(e);
    }
  }

  $effect(() => {
    if (!open) return;
    void opts;
    void generate();
    try {
      localStorage.setItem('nv.pm.gen', JSON.stringify({ kind, length, lower, upper, digits, symbols, ambiguous, words, language, separator, capitalize, number }));
    } catch {
    }
  });

  const level = $derived(bits < 50 ? 'weak' : bits < 80 ? 'fair' : bits < 110 ? 'strong' : 'huge');
  const LEVEL: Record<string, () => string> = { weak: m.gen_level_weak, fair: m.gen_level_fair, strong: m.gen_level_strong, huge: m.gen_level_huge };
  const KINDS: { id: Kind; label: () => string }[] = [
    { id: 'random', label: m.gen_random },
    { id: 'passphrase', label: m.gen_passphrase },
  ];
  const LANGS: { id: 'it' | 'en'; label: () => string }[] = [
    { id: 'it', label: m.gen_lang_it },
    { id: 'en', label: m.gen_lang_en },
  ];
  const onlyClass = $derived([lower, upper, digits, symbols].filter(Boolean).length === 1);

  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      toasts.push(m.gen_copied(), 'success');
    } catch {
      toasts.push(m.err_clipboard(), 'error');
    }
  }
</script>

<Dialog {open} title={m.gen_title()} variant="wide" {onclose}>
  <div class="gen" data-testid="pm-generator" data-kind={kind}>
    <div class="output">
      <output class="value mono" data-testid="gen-value" aria-live="polite"
        >{#key value}{#if rolled > 0}<span use:decode={{ key: `gen:${rolled}`, speed: 2.8, pointerOnly: true }}>{value}</span>{:else}{value || ' '}{/if}{/key}</output
      >
      <button
        type="button"
        class="btn ghost icon"
        aria-label={m.gen_again()}
        title={m.gen_again()}
        onclick={() => generate(true)}
        data-testid="gen-again"><RefreshCw size={16} /></button
      >
      <button type="button" class="btn ghost icon" aria-label={m.action_copy()} title={m.action_copy()} onclick={copy} disabled={!value}><Copy size={16} /></button>
    </div>
    <div class="meter" data-level={level}>
      <span class="caps">{m.gen_entropy()}</span>
      <span class="bar" role="meter" aria-label={m.gen_entropy()} aria-valuemin={0} aria-valuemax={128} aria-valuenow={Math.round(bits)} aria-valuetext="{m.gen_bits({ n: String(Math.round(bits)) })}, {LEVEL[level]()}">
        <span class="fill" style:--k={Math.min(1, bits / 128)}></span>
        {#each [50, 80, 110] as b (b)}<span class="notch" style:left="{(b / 128) * 100}%"></span>{/each}
      </span>
      <span class="bits mono" data-testid="gen-bits">{m.gen_bits({ n: String(Math.round(bits)) })} · {LEVEL[level]()}</span>
    </div>
    {#if error}<p class="error-text" role="alert">{error}</p>{/if}

    <div class="segmented" role="radiogroup" aria-label={m.gen_kind()} tabindex="-1" data-testid="gen-kind" onkeydown={(e) => radioKeys(e, KINDS.map((k) => k.id), kind, (v) => (kind = v))}>
      {#each KINDS as k (k.id)}
        <button type="button" role="radio" aria-checked={kind === k.id} tabindex={kind === k.id ? 0 : -1} onclick={() => (kind = k.id)}>{k.label()}</button>
      {/each}
    </div>

    {#if kind === 'random'}
      <label class="range">
        <span>{m.gen_length()}</span>
        <input type="range" min="4" max="128" bind:value={length} data-testid="gen-length" style:--fill="{((length - 4) / 124) * 100}%" />
        <span class="mono num">{length}</span>
      </label>
      <div class="toggles">
        {#each [{ label: m.gen_lower, get: () => lower, set: (v: boolean) => (lower = v) }, { label: m.gen_upper, get: () => upper, set: (v: boolean) => (upper = v) }, { label: m.gen_digits, get: () => digits, set: (v: boolean) => (digits = v) }, { label: m.gen_symbols, get: () => symbols, set: (v: boolean) => (symbols = v) }] as t, i (i)}
          <div class="switch-row">
            <span class="switch-label" id="gen-t-{i}">{t.label()}</span>
            <button type="button" class="switch" role="switch" aria-checked={t.get()} aria-labelledby="gen-t-{i}" disabled={t.get() && onlyClass} onclick={() => t.set(!t.get())}><span class="knob"></span></button>
          </div>
        {/each}
        <div class="switch-row">
          <span class="switch-label" id="gen-amb">{m.gen_ambiguous()}</span>
          <button type="button" class="switch" role="switch" aria-checked={ambiguous} aria-labelledby="gen-amb" onclick={() => (ambiguous = !ambiguous)}><span class="knob"></span></button>
        </div>
      </div>
    {:else}
      <label class="range">
        <span>{m.gen_words()}</span>
        <input type="range" min="3" max="20" bind:value={words} data-testid="gen-words" style:--fill="{((words - 3) / 17) * 100}%" />
        <span class="mono num">{words}</span>
      </label>
      <div class="row2">
        <div class="field">
          <span class="field-label" id="gen-lang">{m.gen_language()}</span>
          <div class="segmented" role="radiogroup" aria-labelledby="gen-lang" tabindex="-1" onkeydown={(e) => radioKeys(e, LANGS.map((l) => l.id), language, (v) => (language = v))}>
            {#each LANGS as l (l.id)}
              <button type="button" role="radio" aria-checked={language === l.id} tabindex={language === l.id ? 0 : -1} onclick={() => (language = l.id)}>{l.label()}</button>
            {/each}
          </div>
        </div>
        <div class="field sep">
          <label for="gen-sep">{m.gen_separator()}</label>
          <input id="gen-sep" class="input mono" maxlength="3" bind:value={separator} />
        </div>
      </div>
      <div class="toggles">
        <div class="switch-row">
          <span class="switch-label" id="gen-cap">{m.gen_capitalize()}</span>
          <button type="button" class="switch" role="switch" aria-checked={capitalize} aria-labelledby="gen-cap" onclick={() => (capitalize = !capitalize)}><span class="knob"></span></button>
        </div>
        <div class="switch-row">
          <span class="switch-label" id="gen-num">{m.gen_number()}</span>
          <button type="button" class="switch" role="switch" aria-checked={number} aria-labelledby="gen-num" onclick={() => (number = !number)}><span class="knob"></span></button>
        </div>
      </div>
    {/if}
  </div>
  {#snippet actions()}
    <button type="button" class="btn" onclick={onclose}>{m.action_close()}</button>
    {#if onuse}
      <button type="button" class="btn primary" disabled={!value} onclick={() => onuse(value)} data-testid="gen-use">{m.gen_use()}</button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .gen {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .output {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 6px 6px 14px;
    border-radius: var(--radius-md);
    background: var(--field);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      inset 0 0 0 1px var(--field-edge);
  }
  .value {
    flex: 1;
    min-width: 0;
    font-size: 17px;
    line-height: 1.5;
    color: var(--title-ink);
    overflow-wrap: anywhere;
  }
  .meter {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: var(--space-3);
  }
  .bar {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: var(--paper-sunk);
    box-shadow:
      inset 0 1px 2px var(--field-shade),
      0 1px 0 var(--paper-hi);
    overflow: hidden;
  }
  .notch {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1.5px;
    margin-left: -0.75px;
    background: var(--paper-hi);
    opacity: 0.85;
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
    transform: scaleX(var(--k));
    transform-origin: 0 50%;
    transition: transform var(--dur-micro) var(--ease-out);
  }
  .meter[data-level='weak'] .fill {
    background: var(--danger);
  }
  .meter[data-level='weak'] .bits {
    color: var(--danger);
  }
  .meter[data-level='huge'] .bits,
  .meter[data-level='strong'] .bits {
    color: var(--ink);
  }
  .bits {
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .range {
    display: grid;
    grid-template-columns: 8em 1fr 3ch;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-base);
    color: var(--ink);
  }
  .range input {
    width: 100%;
  }
  .num {
    text-align: right;
  }
  .toggles {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row2 {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: flex-end;
  }
  .sep .input {
    width: 6ch;
  }
</style>
