<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Eye, EyeOff } from '@lucide/svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { isMobile } from '../lib/platform';

  let {
    value = $bindable(''),
    label,
    id = `pw-${Math.random().toString(36).slice(2, 8)}`,
    invalid = false,
    describedby,
    autofocus = false,
    disabled = false,
    autocomplete = 'current-password',
    placeholder,
    input = $bindable(),
    onkeydown,
    variant = 'paper',
  }: {
    value?: string;
    label: string;
    id?: string;
    invalid?: boolean;
    describedby?: string;
    autofocus?: boolean;
    disabled?: boolean;
    autocomplete?: HTMLInputAttributes['autocomplete'];
    placeholder?: string;
    input?: HTMLInputElement;
    onkeydown?: (e: KeyboardEvent) => void;
    variant?: 'paper' | 'document';
  } = $props();

  let visible = $state(false);
  let caps = $state(false);

  function key(e: KeyboardEvent) {
    caps = e.getModifierState?.('CapsLock') ?? false;
    onkeydown?.(e);
  }

  $effect(() => {
    if (autofocus && input && !disabled && !isMobile) input.focus();
  });
</script>

<div class="password-field" data-variant={variant}>
  <label class="pf-label" for={id}>{label}</label>
  <div class="control" class:invalid>
    <input
      bind:this={input}
      bind:value
      {id}
      class="input"
      type={visible ? 'text' : 'password'}
      {autocomplete}
      {disabled}
      {placeholder}
      spellcheck="false"
      autocapitalize="off"
      aria-invalid={invalid}
      aria-describedby={[describedby, caps ? `${id}-caps` : null].filter(Boolean).join(' ') || undefined}
      onkeydown={key}
      onkeyup={(e) => (caps = e.getModifierState?.('CapsLock') ?? false)}
    />
    <button
      type="button"
      class="toggle btn ghost icon"
      aria-label={visible ? m.pf_hide() : m.pf_show()}
      aria-pressed={visible}
      aria-controls={id}
      {disabled}
      onclick={() => {
        visible = !visible;
        input?.focus();
      }}
    >
      {#if visible}<EyeOff size={16} />{:else}<Eye size={16} />{/if}
    </button>
  </div>
  {#if caps}
    <p class="caps" id="{id}-caps">{m.pf_caps()}</p>
  {/if}
</div>

<style>
  .password-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .pf-label {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--ink);
  }
  .control {
    position: relative;
    display: flex;
  }
  .input {
    flex: 1;
    padding-right: 40px;
  }
  .control.invalid .input {
    border-color: var(--danger);
  }
  .control .toggle {
    position: absolute;
    top: 0;
    bottom: 0;
    right: 3px;
    width: 26px;
    height: 26px;
    min-height: 26px;
    margin: auto 0;
    padding: 0;
    border-radius: var(--radius-sm);
    color: var(--ink-3);
  }
  .caps {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--accent);
  }

  .password-field[data-variant='document'] .pf-label {
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    font-weight: 500;
    letter-spacing: var(--label-track);
    text-transform: uppercase;
    color: var(--ink-3);
  }
  .password-field[data-variant='document'] .input {
    height: var(--key-h-lg);
    padding: 0 48px 0 16px;
    border-radius: var(--radius-lg);
    font-size: 18px;
    letter-spacing: 0.08em;
  }
  .password-field[data-variant='document'] .toggle {
    right: 7px;
    width: 34px;
    height: 34px;
    min-height: 34px;
  }
</style>
