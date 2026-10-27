<script lang="ts">
  import { onMount } from 'svelte';
  import { browserInfo, browserPairAnswer, onBrowser, type PairAnswer, type PairPrompt } from '../lib/browserLink';
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';

  let ask = $state<PairPrompt | null>(null);

  onMount(() => {
    const offs: (() => void)[] = [];
    void onBrowser<PairPrompt>('browser-pair', (p) => (ask = p)).then((u) => offs.push(u));
    void onBrowser<{ id: string }>('browser-pair-closed', (p) => {
      if (ask?.id === p.id) ask = null;
    }).then((u) => offs.push(u));
    void browserInfo()
      .then((i) => {
        if (i.pending) ask = i.pending;
      })
      .catch(() => {});
    return () => offs.forEach((f) => f());
  });

  async function answer(a: PairAnswer) {
    const p = ask;
    ask = null;
    if (p) await browserPairAnswer(p.id, a).catch(() => {});
  }

  const code = $derived(ask ? `${ask.code.slice(0, 3)} ${ask.code.slice(3)}` : '');
</script>

<Dialog open={!!ask} title={m.brw_pair_title({ browser: ask?.label ?? '' })} onclose={() => answer('refuse')}>
  <p>{m.brw_pair_body()}</p>
  <p class="code mono" data-testid="browser-pair-code">{code}</p>
  <p class="hint">{m.brw_pair_hint()}</p>
  {#snippet actions()}
    <button type="button" class="btn ghost" onclick={() => answer('block')} data-testid="browser-pair-block">{m.brw_pair_block()}</button>
    <button type="button" class="btn" onclick={() => answer('refuse')} data-testid="browser-pair-refuse">{m.brw_pair_refuse()}</button>
    <button type="button" class="btn primary" onclick={() => answer('accept')} data-testid="browser-pair-accept">{m.brw_pair_accept()}</button>
  {/snippet}
</Dialog>

<style>
  .code {
    margin: var(--space-3) 0;
    font-size: 34px;
    letter-spacing: 0.08em;
    color: var(--accent);
  }
</style>
