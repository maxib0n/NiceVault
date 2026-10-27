import { localize, t } from './lib/i18n.js';
import { keys, render } from './lib/view.js';

const api = globalThis.browser ?? globalThis.chrome;
const root = document.getElementById('root');
let query = null;
let view = null;

function flash(text, cls = '') {
  root.querySelector('.flash')?.remove();
  const p = document.createElement('p');
  p.className = `note flash ${cls}`.trim();
  p.dataset.testid = 'flash';
  p.textContent = text;
  root.append(p);
}

const on = {
  async fill(e, confirm) {
    const r = await api.runtime.sendMessage({ kind: 'popup_fill', ref: e.ref, confirm, meta: { alias: e.alias }, site: view?.site });
    if (r?.ok) {
      flash(t('note_filled'));
      setTimeout(() => window.close(), 600);
    } else flash(r?.reason === 'rate_limited' ? t('note_rate') : r?.reason === 'never' ? t('note_never') : t('st_error'), 'warn');
  },
  totp() {},
  async unlock(alias) {
    await api.runtime.sendMessage({ kind: 'popup_unlock', alias });
    await load();
  },
  async launch() {
    root.dataset.state = 'starting';
    await load();
  },
  pair() {
    void api.tabs.create({ url: api.runtime.getURL('pair.html') });
    window.close();
  },
  async search(q) {
    query = q || null;
    await load();
  },
  async copy(e, field) {
    const r = await api.runtime.sendMessage({ kind: 'popup_copy', ref: e.ref, field });
    flash(r?.ok ? t('note_copied', { s: r.clear_after }) : t('st_error'), r?.ok ? '' : 'warn');
  },
  async openApp() {
    await api.runtime.sendMessage({ kind: 'show_app' });
    window.close();
  },
  help() {
    void api.runtime.openOptionsPage();
  },
};

async function load() {
  const v = query ? await api.runtime.sendMessage({ kind: 'popup_search', query }) : await api.runtime.sendMessage({ kind: 'popup_view' });
  view = v ?? { state: 'error' };
  const focused = document.activeElement?.classList?.contains('search');
  render(root, view, on, { popup: true, query });
  root.querySelector('.list > .row')?.classList.add('sel');
  if (focused || query) {
    const s = root.querySelector('.search');
    s?.focus();
    s?.setSelectionRange(s.value.length, s.value.length);
  }
}

document.addEventListener('keydown', (e) => {
  if (keys(root, e)) e.preventDefault();
});
api.runtime.onMessage.addListener((m) => {
  if (m?.kind === 'volumes_changed') void load();
});
localize();
void load();
