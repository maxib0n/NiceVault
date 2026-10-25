// Honest password strength. zxcvbn-ts estimates how many guesses an attacker
// needs; we convert guesses to time using NiceVault's real cost: every guess
// runs Argon2id with 512 MiB, so even a large GPU farm manages only ~10^4/s.
// Dictionaries are loaded lazily (they are large).

import type { ZxcvbnFactory, ZxcvbnResult } from '@zxcvbn-ts/core';
import { i18n } from './i18n.svelte';
import { m } from '../paraglide/messages.js';
import { frozenSafe } from './frozenSafe';

export const ATTACKER_GUESSES_PER_SECOND = 1e4;
/** minimum zxcvbn score accepted for a new password */
export const MIN_SCORE = 3;

export interface Strength {
  score: 0 | 1 | 2 | 3 | 4;
  label: string;
  /** "3 ore", "200 anni", "secoli" */
  crackTime: string;
  warning: string | null;
  suggestions: string[];
  acceptable: boolean;
}

const SCORE_TEXT = [m.strength_0, m.strength_1, m.strength_2, m.strength_3, m.strength_4] as const;
/** "Molto debole" … "Ottima" in the UI language. */
export const scoreLabel = (score: number) => SCORE_TEXT[Math.max(0, Math.min(4, score))]();

// one factory per UI language: zxcvbn's warnings and suggestions come in it
const factories = new Map<string, Promise<ZxcvbnFactory>>();

function load(): Promise<ZxcvbnFactory> {
  const lang = i18n.current;
  let factory = factories.get(lang);
  if (!factory) {
    factory = (async () => {
      const [{ ZxcvbnFactory }, common, it, en] = await Promise.all([
        import('@zxcvbn-ts/core'),
        import('@zxcvbn-ts/language-common'),
        import('@zxcvbn-ts/language-it'),
        import('@zxcvbn-ts/language-en'),
      ]);
      return new ZxcvbnFactory({
        dictionary: frozenSafe({ ...common.dictionary, ...en.dictionary, ...it.dictionary }),
        graphs: common.adjacencyGraphs,
        translations: lang === 'it' ? it.translations : en.translations,
      });
    })();
    factories.set(lang, factory);
  }
  return factory;
}

/** Warm the dictionaries (call when the password step appears). */
export function preloadStrength(): void {
  void load();
}

export function humanDuration(seconds: number): string {
  if (seconds < 1) return m.dur_instant();
  const units: [number, (i: { count: number; n: string }) => string][] = [
    [60, m.dur_seconds],
    [60, m.dur_minutes],
    [24, m.dur_hours],
    [30, m.dur_days],
    [12, m.dur_months],
    [100, m.dur_years],
  ];
  let v = seconds;
  for (const [step, say] of units) {
    if (v < step) {
      const n = Math.round(v);
      return say({ count: n, n: String(n) });
    }
    v /= step;
  }
  return m.dur_centuries();
}

export function fromResult(r: Pick<ZxcvbnResult, 'score' | 'guesses' | 'feedback'>, password: string): Strength {
  const seconds = r.guesses / 2 / ATTACKER_GUESSES_PER_SECOND;
  return {
    score: r.score,
    label: scoreLabel(r.score),
    crackTime: humanDuration(seconds),
    warning: r.feedback.warning || null,
    suggestions: r.feedback.suggestions ?? [],
    acceptable: r.score >= MIN_SCORE && password.length >= 8,
  };
}

export async function estimate(password: string, userInputs: string[] = []): Promise<Strength | null> {
  if (!password) return null;
  const z = await load();
  return fromResult(z.check(password, userInputs), password);
}
