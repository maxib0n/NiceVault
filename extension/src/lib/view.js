import { t } from './i18n.js';

function h(tag, props = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'class') el.className = v;
    else if (k === 'dataset') Object.assign(el.dataset, v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const c of kids.flat()) if (c != null && c !== false) el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  return el;
}

const STATE_TEXT = {
  missing: 'st_missing',
  unpaired: 'st_unpaired',
  closed: 'st_closed',
  starting: 'st_starting',
  launch_failed: 'st_launch_failed',
  version_app: 'st_version_app',
  version_ext: 'st_version_ext',
  unsupported: 'st_unsupported',
  error: 'st_error',
  waiting: 'st_waiting',
};

function monogram(e) {
  if (e.icon) return h('img', { class: 'ico', src: e.icon, alt: '' });
  return h('span', { class: 'ico mono', 'aria-hidden': 'true' }, (e.title || '?').trim().charAt(0).toUpperCase());
}

function siteLabel(v) {
  if (!v.host) return v.site ?? '';
  return v.host_unicode ? `${v.host_unicode} (${v.host})` : v.host;
}

export function render(root, v, on, o = {}) {
  root.replaceChildren();
  root.dataset.state = v.state;
  const head = h('div', { class: 'head' }, h('span', { class: 'brand' }, 'NiceVault'), v.host ? h('span', { class: 'site', title: v.host }, siteLabel(v)) : null);
  root.append(head);

  if (v.cross) root.append(h('p', { class: 'note' }, t('note_frame', { frame: v.cross.frame, top: v.cross.top })));
  if (v.formAction) root.append(h('p', { class: 'note warn' }, t('note_form_action', { host: v.formAction })));

  const actions = h('div', { class: 'actions' });
  const btn = (label, f, cls = '') => h('button', { type: 'button', class: `btn ${cls}`.trim(), onclick: f }, label);

  if (STATE_TEXT[v.state]) {
    root.append(h('p', { class: `msg ${v.state === 'waiting' ? 'pulse' : ''}`.trim(), dataset: { testid: `st-${v.state}` } }, t(STATE_TEXT[v.state])));
    if (v.state === 'missing') {
      if (v.os === 'linux') root.append(h('p', { class: 'note' }, t('st_sandbox')));
      actions.append(btn(t('act_install'), on.help));
    }
    if (v.state === 'unpaired') actions.append(btn(t('act_pair'), on.pair, 'primary'));
    if (v.state === 'closed') actions.append(btn(t('act_launch'), on.launch, 'primary'));
    if (v.state === 'launch_failed' || v.state === 'error') actions.append(btn(t('act_retry'), on.launch));
    if (v.state === 'waiting') actions.append(btn(t('act_show_window'), () => on.unlock(v.locked?.[0]?.alias)));
    if (actions.childElementCount) root.append(actions);
    return;
  }

  if (v.state === 'lookalike') {
    root.append(h('p', { class: 'msg danger', dataset: { testid: 'st-lookalike' } }, t('st_lookalike', { of: v.lookalike.of_unicode ?? v.lookalike.of })));
    return;
  }

  if (v.insecure_page && v.state !== 'search') root.append(h('p', { class: 'note amber', dataset: { testid: 'st-insecure' } }, t('st_insecure')));

  const list = h('div', { class: 'list', role: 'listbox' });
  if (v.totp) {
    list.append(h('button', { type: 'button', class: 'row', role: 'option', dataset: { testid: 'row-totp' }, onclick: () => on.totp() }, h('span', { class: 'ico mono' }, '2FA'), h('span', { class: 'txt' }, h('span', { class: 't1' }, t('row_totp', { title: v.totp.title })))));
  }
  for (const e of v.entries ?? []) {
    const vol = e.kind === 'space' ? t('row_space', { name: e.volume }) : e.volume;
    const matchless = v.state === 'search' && !e.quality;
    const needs = e.insecure ? ['insecure'] : matchless ? ['mismatch'] : [];
    const row = h(
      'div',
      { class: 'row', role: 'option', tabindex: '-1', dataset: { testid: 'row', title: e.title } },
      monogram(e),
      h('span', { class: 'txt' }, h('span', { class: 't1' }, e.title, e.username ? h('span', { class: 'user' }, e.username) : null), h('span', { class: 't2' }, vol)),
    );
    const confirm = () => {
      const box = h(
        'div',
        { class: 'confirm' },
        h('p', { class: 'note' }, e.insecure ? t('st_insecure') : t('note_mismatch', { entry: e.url_host ?? '—', page: v.host ?? '' })),
        h('div', { class: 'actions' }, btn(t('act_fill_anyway'), () => on.fill(e, needs), 'primary'), btn(t('act_cancel'), () => box.remove())),
      );
      row.after(box);
    };
    const doFill = () => (needs.length ? confirm() : on.fill(e, []));
    row.addEventListener('click', (ev) => {
      if (ev.target.closest('.mini')) return;
      doFill();
    });
    row.fill = doFill;
    if (o.popup) {
      const mini = h('span', { class: 'minis' }, btn(t('act_fill'), doFill, 'mini primary'), btn(t('act_copy_user'), () => on.copy(e, 'username'), 'mini'), btn(t('act_copy_password'), () => on.copy(e, 'password'), 'mini'), e.has_totp ? btn(t('act_copy_totp'), () => on.copy(e, 'totp'), 'mini') : null);
      row.append(mini);
    }
    list.append(row);
  }
  if (list.childElementCount) root.append(list);
  else if (v.state === 'search') root.append(h('p', { class: 'msg' }, t('search_none')));
  else if (v.state === 'locked') {
    const first = v.locked?.[0];
    root.append(h('p', { class: 'msg', dataset: { testid: 'st-locked' } }, first && v.site ? t('st_site_locked', { site: v.site, name: first.name }) : t('st_all_locked')));
  } else if (v.state === 'none') root.append(h('p', { class: 'msg', dataset: { testid: 'st-none' } }, t('st_none', { site: v.site ?? '' })));

  const locked = (v.locked ?? []).slice(0, v.state === 'locked' ? 4 : 2);
  for (const [i, l] of locked.entries()) {
    const label = l.state === 'absent' ? t('st_absent', { name: l.name }) : v.state === 'locked' && i === 0 ? null : t('row_locked_hint', { name: l.name });
    const line = h('div', { class: 'lockrow' }, label ? h('span', { class: 'txt t2' }, label) : null, l.state === 'absent' ? null : btn(i === 0 && v.state === 'locked' ? t('act_unlock', { name: l.name }) : t('act_unlock', { name: l.name }), () => on.unlock(l.alias), i === 0 && v.state === 'locked' ? 'primary' : ''));
    root.append(line);
  }

  const search = h('input', { type: 'search', class: 'search', placeholder: t('act_search_all'), value: o.query ?? '', 'aria-label': t('search_placeholder') });
  search.addEventListener('input', () => on.search(search.value));
  root.append(h('div', { class: 'foot' }, search, btn(t('act_open_app'), on.openApp, 'ghost')));
}

export function keys(root, e) {
  const rows = [...root.querySelectorAll('.list > .row')];
  if (!rows.length) return false;
  const i = rows.findIndex((r) => r.classList.contains('sel'));
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    const n = e.key === 'ArrowDown' ? Math.min(rows.length - 1, i + 1) : Math.max(0, i - 1);
    rows.forEach((r, k) => r.classList.toggle('sel', k === n));
    rows[n].scrollIntoView({ block: 'nearest' });
    return true;
  }
  if (e.key === 'Enter' && i >= 0) {
    (rows[i].fill ?? (() => rows[i].click()))();
    return true;
  }
  return false;
}
