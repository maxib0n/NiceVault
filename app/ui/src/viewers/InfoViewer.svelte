<script lang="ts" module>
  export type InfoReason = 'none' | 'too_big' | 'unsupported' | 'error' | 'binary';
</script>

<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { Download, ExternalLink, FileExclamationPoint, Scale } from '@lucide/svelte';
  import type { FsNode, VaultInfo } from '../lib/types';
  import type { ByteSource } from './source';
  import { HEX_BYTES, hexRows, magicLabel } from './kinds';
  import { fileType, typeLabel } from '../lib/filetype';
  import { formatBytes, formatDateTime, formatNumber } from '../lib/format';
  import FileIcon from '../components/FileIcon.svelte';
  import { openMounted } from './actions';

  let {
    vault,
    node,
    src,
    reason,
    cap,
    onexport,
  }: { vault: VaultInfo; node: FsNode; src: ByteSource | null; reason: InfoReason; cap: number; onexport: () => void } = $props();

  const type = $derived(fileType(node));
  let bytes = $state<Uint8Array | null>(null);
  let magic = $state<string | null>(null);
  let failed = $state(false);
  let more = $state(true);

  async function load(upto: number) {
    if (!src) return;
    try {
      const b = await src.read(0, upto);
      bytes = b;
      more = b.length >= upto && upto < (await src.size());
      magic ??= magicLabel(b);
    } catch {
      failed = true;
    }
  }
  $effect(() => {
    bytes = null;
    magic = null;
    failed = false;
    void load(HEX_BYTES);
  });

  const rows = $derived(bytes ? hexRows(bytes) : []);
  const title = $derived(
    reason === 'too_big'
      ? m.vw_too_big_title()
      : reason === 'unsupported'
        ? m.vw_unsupported_title()
        : reason === 'error'
          ? m.vw_error_title()
          : reason === 'binary'
            ? m.vw_binary_title()
            : m.vw_info_title(),
  );
</script>

<div class="vw" data-testid="viewer-info" data-reason={reason}>
  <div class="vw-scroll">
    <div class="vw-sheet info">
      <header class="head">
        <span class="vw-plate">{#if reason === 'too_big'}<Scale size={34} />{:else if reason === 'error'}<FileExclamationPoint size={34} />{:else}<FileIcon {type} size={34} />{/if}</span>
        <div>
          <h2 class="vw-state-title" data-testid="viewer-state">{title}</h2>
          <p class="hint">
            {#if reason === 'too_big'}
              {m.vw_too_big_hint({ size: formatBytes(node.size), cap: formatBytes(cap) })}
            {:else if reason === 'unsupported'}
              {m.vw_unsupported_hint()}
            {:else if reason === 'error'}
              {m.vw_error_hint()}
            {:else}
              {m.vw_info_hint()}
            {/if}
          </p>
          <div class="vw-actions start">
            <button type="button" class="btn" onclick={onexport}><Download size={16} aria-hidden="true" /> {m.tb_export()}</button>
            {#if vault.mountpoint}
              <button type="button" class="btn ghost" onclick={() => openMounted(vault.id, node.id)}><ExternalLink size={16} aria-hidden="true" /> {m.vw_open_external()}</button>
            {/if}
          </div>
        </div>
      </header>

      <hr class="vw-rule" />
      <dl class="vw-facts">
        <dt>{m.vw_f_name()}</dt>
        <dd>{node.name}</dd>
        <dt>{m.vw_f_type()}</dt>
        <dd>{typeLabel(type)}{#if magic}<span class="sub"> · {m.vw_f_content({ what: magic })}</span>{/if}</dd>
        <dt>{m.vw_f_size()}</dt>
        <dd><span class="vw-mono">{formatBytes(node.size)}</span> <span class="sub vw-mono">({m.vw_bytes({ n: formatNumber(node.size) })})</span></dd>
        <dt>{m.vw_f_modified()}</dt>
        <dd>{formatDateTime(node.mtime / 1e6)}</dd>
      </dl>

      {#if src}
        <hr class="vw-rule" />
        <p class="vw-eyebrow">{m.vw_hex_title()}</p>
        {#if failed}
          <p class="hint">{m.text_error()}</p>
        {:else if bytes && bytes.length === 0}
          <p class="hint">{m.vw_empty_file()}</p>
        {:else if bytes}
          <div class="hex vw-mono" role="table" aria-label={m.vw_hex_title()} data-testid="viewer-hex">
            {#each rows as r (r.off)}
              <div class="row" role="row"><span class="off" role="cell">{r.off}</span><span class="bytes" role="cell">{r.hex}</span><span class="ascii" role="cell">{r.ascii}</span></div>
            {/each}
          </div>
          {#if more}
            <button type="button" class="btn ghost more-hex" onclick={() => load((bytes?.length ?? 0) + HEX_BYTES)}>{m.vw_hex_more()}</button>
          {/if}
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .info {
    max-width: 780px;
  }
  .head {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: var(--space-5);
    align-items: center;
  }
  .head .hint {
    margin: var(--space-2) 0 var(--space-4);
  }
  .start {
    justify-content: flex-start;
  }
  .sub {
    color: var(--ink-3);
  }
  .hex {
    font-size: 12px;
    line-height: 1.6;
    color: var(--ink-2);
    overflow-x: auto;
    content-visibility: auto;
  }
  .row {
    display: flex;
    gap: var(--space-4);
    white-space: pre;
  }
  .off {
    color: var(--ink-4);
  }
  .ascii {
    color: var(--ink-3);
  }
  .more-hex {
    margin-top: var(--space-3);
  }
</style>
