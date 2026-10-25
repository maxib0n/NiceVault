<script lang="ts">
  import {
    File,
    FileArchive,
    FileCode,
    FileImage,
    FileKey,
    FileMusic,
    FilePen,
    FileSpreadsheet,
    FileText,
    FileVideoCamera,
    Folder,
    Presentation,
  } from '@lucide/svelte';
  import type { FileType } from '../lib/filetype';

  let { type, size = 20 }: { type: FileType; size?: number } = $props();

  const ICONS = {
    dir: Folder,
    image: FileImage,
    video: FileVideoCamera,
    audio: FileMusic,
    pdf: FileText,
    text: FileCode,
    archive: FileArchive,
    document: FilePen,
    spreadsheet: FileSpreadsheet,
    presentation: Presentation,
    key: FileKey,
    other: File,
  } as const;

  const Icon = $derived(ICONS[type]);
</script>

<span class="file-icon" data-type={type} aria-hidden="true">
  <Icon {size} strokeWidth={1.5} />
</span>

<style>
  .file-icon {
    display: inline-flex;
    color: inherit;
  }
</style>
