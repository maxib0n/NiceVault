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
