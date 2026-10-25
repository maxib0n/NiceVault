// The UI language. Messages live in messages/{it,en}.json (Paraglide JS, inlang message
// format; Italian is the source) and compile to src/paraglide. Every message function asks
// getLocale() when it is called; it is overwritten here to read a $state, so any template
// that prints a message re-renders when the language changes — no reload, no remount.
//
// Which language: the ?lang=it|en switch (screenshots, works everywhere like ?theme=), else
// the choice saved in the global settings (Impostazioni; "system" = follow the OS), else the
// OS / browser locale (navigator.languages): Italian for it-*, English otherwise.
// Numbers, sizes and dates are formatted with Intl in the same locale (`tag`).
import { baseLocale, locales, overwriteGetLocale, type Locale } from '../paraglide/runtime.js';

export type LangChoice = 'system' | Locale;
export const LANG_CHOICES: readonly LangChoice[] = ['system', ...locales];

const nav = (): readonly string[] =>
  typeof navigator === 'undefined' ? [] : navigator.languages?.length ? navigator.languages : navigator.language ? [navigator.language] : [];

/** The OS / browser preference mapped to a UI language (it-* -> it, anything else -> en). */
export function systemLocale(prefs: readonly string[] = nav()): Locale {
  for (const p of prefs) {
    const l = p.toLowerCase().split(/[-_]/)[0];
    if ((locales as readonly string[]).includes(l)) return l as Locale;
  }
  return prefs.length ? 'en' : baseLocale;
}

function urlLang(): Locale | null {
  if (typeof location === 'undefined') return null;
  const v = new URLSearchParams(location.search).get('lang');
  return v && (locales as readonly string[]).includes(v) ? (v as Locale) : null;
}

/** BCP 47 tag for Intl in `locale`: the OS's own regional variant when it speaks that language. */
export function intlTag(locale: Locale, prefs: readonly string[] = nav()): string {
  const own = prefs.find((p) => p.toLowerCase().split(/[-_]/)[0] === locale && /[-_]/.test(p));
  return own ? own.replace('_', '-') : locale === 'it' ? 'it-IT' : 'en-GB';
}

class I18n {
  /** what the person chose in Impostazioni ("system" = the OS language) */
  choice = $state<LangChoice>('system');
  /** ?lang= wins over everything (screenshots) */
  readonly forced: Locale | null = urlLang();
  current = $derived<Locale>(this.forced ?? (this.choice === 'system' ? systemLocale() : this.choice));
  /** Intl locale of the current language */
  tag = $derived(intlTag(this.current));

  /** From the global settings (app_settings_get); unknown values mean "system". */
  adopt(choice: string | null | undefined) {
    this.choice = (LANG_CHOICES as readonly string[]).includes(choice ?? '') ? (choice as LangChoice) : 'system';
  }
}

export const i18n = new I18n();

overwriteGetLocale(() => i18n.current);

/** <html lang> follows the language (screen readers, hyphenation, quotes). */
export function installLang(): () => void {
  if (typeof document === 'undefined') return () => undefined;
  return $effect.root(() => {
    $effect(() => {
      document.documentElement.lang = i18n.current;
    });
  });
}
