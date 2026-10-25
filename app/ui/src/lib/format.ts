// Formatting helpers in the UI language (i18n.tag for Intl, messages for words). Pure functions.
import { i18n } from './i18n.svelte';
import { m } from '../paraglide/messages.js';

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

// Intl formatters are costly to build (a folder of 500 formats two values per row): one per
// language and options, kept for the session.
const formatters = new Map<string, Intl.NumberFormat | Intl.DateTimeFormat>();
function cached<T extends Intl.NumberFormat | Intl.DateTimeFormat>(key: string, make: () => T): T {
  const k = `${i18n.tag}|${key}`;
  let f = formatters.get(k) as T | undefined;
  if (!f) formatters.set(k, (f = make()));
  return f;
}

/** A number in the UI's locale: 1234.5 -> "1.234,5" (it) / "1,234.5" (en). */
export function formatNumber(n: number, digits = 0): string {
  return cached(`n${digits}`, () => new Intl.NumberFormat(i18n.tag, { maximumFractionDigits: digits, minimumFractionDigits: 0 })).format(n);
}

/** 1536 -> "1,5 KB" (base 1024, one decimal under 10). */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '—';
  let i = 0;
  let v = n;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = i === 0 || v >= 10 ? 0 : 1;
  return `${formatNumber(v, digits)} ${UNITS[i]}`;
}

/** unix ns -> Date */
export const nsToDate = (ns: number) => new Date(ns / 1_000_000);

const dateFmt = () => cached('date', () => new Intl.DateTimeFormat(i18n.tag, { day: 'numeric', month: 'short', year: 'numeric' }));
const timeFmt = () => cached('time', () => new Intl.DateTimeFormat(i18n.tag, { hour: '2-digit', minute: '2-digit' }));

/** "oggi 14:02", "ieri 09:10", "3 mar 2024" */
export function formatDate(d: Date, now: Date = new Date()): string {
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(now) - day(d)) / 86_400_000);
  if (diff === 0) return m.fmt_today({ time: timeFmt().format(d) });
  if (diff === 1) return m.fmt_yesterday_at({ time: timeFmt().format(d) });
  return dateFmt().format(d);
}

/** A full date and time ("3 mar 2024, 14:02"). */
export function formatDateTime(ms: number): string {
  return cached('datetime', () => new Intl.DateTimeFormat(i18n.tag, { dateStyle: 'medium', timeStyle: 'short' })).format(new Date(ms));
}

/**
 * When something happened, as it reads best in the UI language: "adesso", "5 minuti fa",
 * "oggi 14:02", "ieri 09:10", "3 giorni fa", then the date ("3 mar 2024"). Pair it with
 * formatDateTime in a title for the exact moment.
 */
export function formatWhen(ms: number, now: number = Date.now()): string {
  const s = Math.round((now - ms) / 1000);
  if (s >= 0 && s < 60) return m.fmt_now();
  const min = Math.round(s / 60);
  if (s >= 0 && min < 60) return m.fmt_minutes_ago({ count: min, n: formatNumber(min) });
  const d = new Date(ms);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((day(new Date(now)) - day(d)) / 86_400_000);
  if (days === 0) return m.fmt_today({ time: timeFmt().format(d) });
  if (days === 1) return m.fmt_yesterday_at({ time: timeFmt().format(d) });
  if (days > 1 && days < 7) return m.fmt_days_ago({ count: days, n: formatNumber(days) });
  return dateFmt().format(d);
}

/** relative "2 ore fa" for recent volumes */
export function formatAgo(ms: number, now: number = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return m.fmt_now();
  const min = Math.round(s / 60);
  if (min < 60) return m.fmt_minutes_ago({ count: min, n: formatNumber(min) });
  const h = Math.round(min / 60);
  if (h < 24) return m.fmt_hours_ago({ count: h, n: formatNumber(h) });
  const d = Math.round(h / 24);
  if (d === 1) return m.fmt_yesterday();
  if (d < 30) return m.fmt_days_ago({ count: d, n: formatNumber(d) });
  return dateFmt().format(new Date(ms));
}

/** 754_000 ms -> "12:34" */
export function formatCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const mm = Math.floor(total / 60);
  const s = total % 60;
  return `${mm}:${String(s).padStart(2, '0')}`;
}

/** "3 elementi" / "3 items" */
export const itemCount = (count: number) => m.fmt_items({ count, n: formatNumber(count) });
/** "3 file" / "3 files" */
export const fileCount = (count: number) => m.fmt_files({ count, n: formatNumber(count) });

