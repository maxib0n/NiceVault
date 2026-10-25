import { m } from '../paraglide/messages.js';

// Accent presets: the volume's ink (name = its message). It is stitched and printed on anthracite
// Alcantara (the default, dark theme) and printed in intaglio on ivory paper (light).
// Values live in styles/accents.css (<html data-accent="<id>">); the ids are also
// validated by the backend (app/src-tauri/src/appearance.rs ACCENTS).

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

/** New volumes, and locked screens that show no volume's look. */
export const DEFAULT_ACCENT: AccentId = 'velluto';

export function isAccent(v: unknown): v is AccentId {
  return ACCENTS.some((a) => a.id === v);
}

/** Puts the accent on the app root (<html>), where accents.css picks it up. */
export function applyAccent(id: string, root: { dataset: DOMStringMap } | undefined = globalThis.document?.documentElement): void {
  if (root) root.dataset.accent = isAccent(id) ? id : DEFAULT_ACCENT;
}
