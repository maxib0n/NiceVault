import { MESSAGES } from '../messages.js';

const api = globalThis.browser ?? globalThis.chrome;
const ui = (api?.i18n?.getUILanguage?.() ?? globalThis.navigator?.language ?? 'en').toLowerCase();
export const lang = ui.startsWith('it') ? 'it' : 'en';

export function t(key, params = {}) {
  const s = MESSAGES[lang][key] ?? MESSAGES.en[key] ?? key;
  return s.replace(/\{(\w+)\}/g, (_, k) => String(params[k] ?? ''));
}

export function localize(root = document) {
  document.documentElement.lang = lang;
  for (const el of root.querySelectorAll('[data-i18n]')) el.textContent = t(el.dataset.i18n);
  for (const el of root.querySelectorAll('[data-i18n-ph]')) el.placeholder = t(el.dataset.i18nPh);
}
