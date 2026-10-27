import { fsList, previewUrl } from './api';
import type { FsNode, VaultId } from './types';

export type Sidecar = { label: string; lang: string; vtt: string };

export function srtToVtt(srt: string): string {
  const body = srt
    .replace(/^﻿/, '')
    .replace(/\r\n?/g, '\n')
    .replace(/(\d{1,2}:\d{2}:\d{2}),(\d{3})/g, '$1.$2');
  return `WEBVTT\n\n${body.trim()}\n`;
}

export function splitSub(name: string): { base: string; lang: string; ext: 'srt' | 'vtt' } | null {
  const m = /^(.*?)(?:\.([A-Za-z]{2,3}(?:-[A-Za-z]{2,4})?))?\.(srt|vtt)$/i.exec(name);
  return m ? { base: m[1], lang: m[2] ?? '', ext: m[3].toLowerCase() as 'srt' | 'vtt' } : null;
}

export async function sidecarTracks(vault: VaultId, video: FsNode): Promise<Sidecar[]> {
  const dot = video.name.lastIndexOf('.');
  const base = (dot > 0 ? video.name.slice(0, dot) : video.name).toLowerCase();
  let items: FsNode[];
  try {
    items = await fsList(vault, video.parent);
  } catch {
    return [];
  }
  const out: Sidecar[] = [];
  for (const n of items) {
    if (n.kind !== 'file' || n.size > 4 * 1024 * 1024) continue;
    const s = splitSub(n.name);
    if (!s || s.base.toLowerCase() !== base) continue;
    const url = previewUrl(vault, n.id);
    if (!url) continue;
    try {
      const text = await (await fetch(url)).text();
      out.push({ label: s.lang || n.name, lang: s.lang, vtt: s.ext === 'srt' ? srtToVtt(text) : text });
    } catch {
    }
  }
  return out;
}
