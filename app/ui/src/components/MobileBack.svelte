<!-- Phone layout only (styles/mobile.css): "Indietro" above a detail that, on a phone, takes the
     place of its list. Hidden everywhere else; its presence tells mobile.css a detail is shown. -->
<script lang="ts">
  import { ArrowLeft } from '@lucide/svelte';
  import { m } from '../paraglide/messages.js';

  let { onback, label = m.action_back() }: { onback: () => void; label?: string } = $props();
</script>

<button type="button" class="btn ghost sm mobile-back" data-testid="mobile-back" onclick={onback}>
  <ArrowLeft size={16} aria-hidden="true" />
  {label}
</button>

<style>
  .mobile-back {
    display: none;
    align-self: flex-start;
    margin-bottom: var(--space-2);
  }
  @media (max-width: 640px) {
    .mobile-back {
      display: inline-flex;
    }
  }
</style>
