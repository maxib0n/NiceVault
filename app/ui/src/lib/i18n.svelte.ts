import { baseLocale, locales, overwriteGetLocale, type Locale } from '../paraglide/runtime.js';

export type LangChoice = 'system' | Locale;
export const LANG_CHOICES: readonly LangChoice[] = ['system', ...locales];

const nav = (): readonly string[] =>
  typeof navigator === 'undefined' ? [] : navigator.languages?.length ? navigator.languages : navigator.language ? [navigator.language] : [];

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

export function intlTag(locale: Locale, prefs: readonly string[] = nav()): string {
  const own = prefs.find((p) => p.toLowerCase().split(/[-_]/)[0] === locale && /[-_]/.test(p));
  return own ? own.replace('_', '-') : locale === 'it' ? 'it-IT' : 'en-GB';
}

class I18n {
  choice = $state<LangChoice>('system');
  readonly forced: Locale | null = urlLang();
  current = $derived<Locale>(this.forced ?? (this.choice === 'system' ? systemLocale() : this.choice));
  tag = $derived(intlTag(this.current));

  adopt(choice: string | null | undefined) {
    this.choice = (LANG_CHOICES as readonly string[]).includes(choice ?? '') ? (choice as LangChoice) : 'system';
  }
}

export const i18n = new I18n();

overwriteGetLocale(() => i18n.current);

export function installLang(): () => void {
  if (typeof document === 'undefined') return () => undefined;
  return $effect.root(() => {
    $effect(() => {
      document.documentElement.lang = i18n.current;
    });
  });
}
