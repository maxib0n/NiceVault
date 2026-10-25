// Which theme and ink the app wears, by precedence:
//   1. a volume is open and on screen: that volume's own settings (Dettagli > Aspetto);
//   2. Sblocca of a volume whose look is visible while locked: its ink (the header carries no
//      theme, so the app's theme);
//   3. everywhere else (Benvenuto, Impostazioni, the wizard, Sblocca of an encrypted look):
//      the app's own look (Impostazioni > Aspetto dell'app).
import { DEFAULT_ACCENT, isAccent, type AccentId } from './accents';
import { lockedAccent } from './emblem';
import type { RecentEntry, Settings, ThemeId } from './types';

export interface Worn {
  theme: ThemeId;
  accent: AccentId;
}

export function worn(
  open: Pick<Settings, 'theme' | 'accent'> | null,
  unlockTarget: Pick<RecentEntry, 'accent' | 'show_when_locked'> | null,
  app: { theme: ThemeId; accent: string },
): Worn {
  const ink = (a: string | null | undefined, fallback: AccentId): AccentId => (isAccent(a) ? a : fallback);
  const appInk = ink(app.accent, DEFAULT_ACCENT);
  if (open) return { theme: open.theme, accent: ink(open.accent, appInk) };
  return { theme: app.theme, accent: ink(lockedAccent(unlockTarget), appInk) };
}
