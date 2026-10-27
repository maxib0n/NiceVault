import { localize, t } from './lib/i18n.js';
import { keys, render } from './lib/view.js';

const api = globalThis.browser ?? globalThis.chrome;
const token = location.hash.slice(1);
const root = document.getElementById('root');
const shown = performance.now();
let query = null;
let view = null;
let busy = false;

const send = (m) => api.runtime.sendMessage({ token, ...m });

const IO2 = typeof IntersectionObserverEntry !== 'undefined' && 'isVisible' in IntersectionObserverEntry.prototype;
let visibleSince = 0;
if (IO2) {
  try {
    new IntersectionObserver(
      (es) => {
        const e = es[es.length - 1];
        if (e.isVisible) visibleSince ||= performance.now();
        else visibleSince = 0;
        document.documentElement.dataset.visible = e.isVisible ? '1' : '0';
      },
      { trackVisibility: true, delay: 100 },
    ).observe(root);
  } catch {
  }
}

function actionable() {
  const now = performance.now();
  if (now - shown < 400) return false;
  return !IO2 || (visibleSince > 0 && now - visibleSince >= 400);
}

function resize() {
  void send({ kind: 'menu_resize', h: Math.ceil(root.getBoundingClientRect().height) + 2 });
}

function note(text, cls = '') {
  root.querySelector('.flash')?.remove();
  const p = document.createElement('p');
  p.className = `note flash ${cls}`.trim();
  p.textContent = text;
  root.append(p);
  resize();
}

const on = {
  async fill(e, confirm) {
    if (busy) return;
    if (confirm.length && !IO2) {
      note(t('note_use_popup'), 'warn');
      return;
    }
    busy = true;
    const r = await send({ kind: 'menu_fill', ref: e.ref, confirm, meta: { alias: e.alias } });
    busy = false;
    if (!r?.ok) note(r?.reason === 'covered' ? t('note_covered') : r?.reason === 'rate_limited' ? t('note_rate') : r?.reason === 'never' ? t('note_never') : t('st_error'), 'warn');
  },
  async totp() {
    const r = await send({ kind: 'menu_fill', totp: true });
    if (!r?.ok) note(t('st_error'), 'warn');
  },
  async unlock(alias) {
    await send({ kind: 'menu_unlock', alias });
    await load();
  },
  async launch() {
    root.dataset.state = 'starting';
    show(await send({ kind: 'menu_launch' }));
  },
  pair() {
    api.tabs?.create ? api.tabs.create({ url: api.runtime.getURL('pair.html') }) : open(api.runtime.getURL('pair.html'));
    void send({ kind: 'menu_close' });
  },
  async search(q) {
    query = q;
    show(q ? await send({ kind: 'menu_search', query: q }) : await send({ kind: 'menu_view' }));
  },
  copy() {},
  async openApp() {
    await api.runtime.sendMessage({ kind: 'show_app' });
    void send({ kind: 'menu_close' });
  },
  help() {
    void api.runtime.openOptionsPage();
  },
};

function show(v) {
  view = v ?? { state: 'error' };
  const focused = document.activeElement?.classList?.contains('search');
  render(root, view, on, { query });
  root.querySelector('.list > .row')?.classList.add('sel');
  if (focused) {
    const s = root.querySelector('.search');
    s?.focus();
    s?.setSelectionRange(s.value.length, s.value.length);
  }
  resize();
}

async function load() {
  show(query ? await send({ kind: 'menu_search', query }) : await send({ kind: 'menu_view' }));
}

document.addEventListener(
  'click',
  (e) => {
    if (!e.isTrusted || !actionable()) {
      e.stopImmediatePropagation();
      e.preventDefault();
    }
  },
  true,
);
document.addEventListener('keydown', (e) => {
  if (!e.isTrusted) return;
  if (e.key === 'Escape') {
    void send({ kind: 'menu_close' });
    return;
  }
  if ((e.key === 'Enter' || e.key === ' ') && !actionable()) {
    e.preventDefault();
    e.stopImmediatePropagation();
    return;
  }
  if (keys(root, e)) {
    e.preventDefault();
    return;
  }
  const s = root.querySelector('.search');
  if (s && document.activeElement !== s && e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) s.focus();
});
api.runtime.onMessage.addListener((m) => {
  if (m?.kind === 'volumes_changed' || m?.kind === 'state_changed') void load();
});

localize();
document.title = 'NiceVault';
void load();
