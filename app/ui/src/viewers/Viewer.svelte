<!--
  The in-app viewer for one file (the quick look's body). Picks the viewer by name, then by
  the first bytes for names that say nothing; every viewer reads the decrypted bytes over
  the preview URL with Range (source.ts) — into memory only, never a temporary file.
  Files over a viewer's cap, and anything a viewer cannot show, get the info panel (facts,
  first bytes in hex) with Esporta and — only while the volume is mounted — "Apri con app
  esterna" through the mounted drive.
-->
<script lang="ts">
  import './viewer.css';
  import type { FsNode, VaultInfo } from '../lib/types';
  import { urlSource } from './source';
  import { CAPS, sniff, viewerKind, type ViewerKind } from './kinds';
  import { detectEncoding } from './encoding';
  import { languageFor } from './highlight';
  import { extension } from '../lib/filetype';
  import ImageViewer from './ImageViewer.svelte';
  import MediaViewer from './MediaViewer.svelte';
  import PdfViewer from './PdfViewer.svelte';
  import TextViewer from './TextViewer.svelte';
  import MarkdownViewer from './MarkdownViewer.svelte';
  import OfficeViewer from './OfficeViewer.svelte';
  import ArchiveViewer from './ArchiveViewer.svelte';
  import InfoViewer, { type InfoReason } from './InfoViewer.svelte';

  let {
    vault,
    node,
    url,
    onexport,
    onzoom,
  }: {
    vault: VaultInfo;
    node: FsNode;
    /** previewUrl(vault, node); '' when there are no bytes to show (mock without a sample) */
    url: string;
    onexport: () => void;
    /** the picture is zoomed in (the quick look's swipe-to-step pauses meanwhile) */
    onzoom?: (zoomed: boolean) => void;
  } = $props();

  const src = $derived(url ? urlSource(url) : null);
  const named = $derived(viewerKind(node.name));
  /** what the first bytes say, for a name that said nothing */
  let sniffed = $state<ViewerKind | null | undefined>(undefined);
  let failure = $state<InfoReason | null>(null);

  $effect(() => {
    const s = src;
    sniffed = undefined;
    failure = null;
    if (named !== 'info' || !s) {
      sniffed = null;
      return;
    }
    const ac = new AbortController();
    s.read(0, 4096, ac.signal).then(
      (head) => {
        if (ac.signal.aborted) return;
        const k = sniff(head);
        sniffed = k ?? (head.length && !detectEncoding(head).binary ? 'text' : null);
      },
      () => {
        if (!ac.signal.aborted) sniffed = null;
      },
    );
    return () => ac.abort();
  });

  const kind = $derived<ViewerKind | undefined>(sniffed === undefined ? undefined : (sniffed ?? named));
  const tooBig = $derived(kind !== undefined && node.size > CAPS[kind]);
  const reason = $derived<InfoReason | null>(!src ? 'none' : failure ?? (tooBig ? 'too_big' : kind === 'info' ? 'none' : null));
  const fail = (r: InfoReason) => (failure = r);
</script>

<div class="vw-host" data-viewer={reason ? 'info' : (kind ?? 'loading')} data-testid="viewer">
  {#if kind === undefined && src}
    <!-- a blink while the first bytes are read -->
  {:else if reason || !src}
    <InfoViewer {vault} {node} {src} reason={reason ?? 'none'} cap={kind ? CAPS[kind] : 0} {onexport} />
  {:else if kind === 'image'}
    <ImageViewer {url} {node} key={`ql:${vault.id}:${node.id}`} onfail={fail} {onzoom} />
  {:else if kind === 'video' || kind === 'audio'}
    <MediaViewer {vault} {node} {url} kind={kind} onfail={fail} />
  {:else if kind === 'pdf'}
    <PdfViewer src={src} onfail={fail} {onzoom} />
  {:else if kind === 'markdown'}
    <MarkdownViewer src={src} {node} onfail={fail} />
  {:else if kind === 'office'}
    <OfficeViewer src={src} ext={extension(node.name)} onfail={fail} />
  {:else if kind === 'archive'}
    <ArchiveViewer src={src} {node} onfail={fail} />
  {:else}
    <TextViewer src={src} {node} lang={languageFor(node.name)} onfail={fail} />
  {/if}
</div>

<style>
  .vw-host {
    width: 100%;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
