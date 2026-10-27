import { m } from '../paraglide/messages.js';

export const ACCENTS = [
  { id: 'velluto', name: m.accent_velluto },
  { id: 'prugna', name: m.accent_prugna },
  { id: 'prussia', name: m.accent_prussia },
  { id: 'bottiglia', name: m.accent_bottiglia },
  { id: 'seppia', name: m.accent_seppia },
  { id: 'ardesia', name: m.accent_ardesia },
  { id: 'ottanio', name: m.accent_ottanio },
] as const;

export type AccentId = (typeof ACCENTS)[number]['id'];

export const DEFAULT_ACCENT: AccentId = 'velluto';

export function isAccent(v: unknown): v is AccentId {
  return ACCENTS.some((a) => a.id === v);
}

export function applyAccent(id: string, root: { dataset: DOMStringMap } | undefined = globalThis.document?.documentElement): void {
  if (root) root.dataset.accent = isAccent(id) ? id : DEFAULT_ACCENT;
}
