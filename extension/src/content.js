const api = globalThis.browser ?? globalThis.chrome;
const ICON_MIN_WIDTH = 120;
const MENU_W = 340;

let fields = [];
let host = null;
let shadow = null;
let menu = null;
let state = { icon: 'grey' };
let lastContext = null;

const HOST_STYLE = 'all: initial !important; position: fixed !important; inset: 0 auto auto 0 !important; width: 0 !important; height: 0 !important; z-index: 2147483647 !important; opacity: 1 !important; visibility: visible !important; display: block !important; pointer-events: none !important;';

function ensureHost() {
  if (host?.isConnected) return;
  host = document.createElement('nicevault-ui');
  host.setAttribute('style', HOST_STYLE);
  const h = host;
  new MutationObserver(() => {
    if (h !== host) return;
    closeMenu();
    if (h.getAttribute('style') !== HOST_STYLE) h.setAttribute('style', HOST_STYLE);
  }).observe(h, { attributes: true });
  shadow = host.attachShadow({ mode: 'closed' });
  const css = document.createElement('style');
  css.textContent = `
    .nv-icon { position: fixed; width: 22px; height: 22px; padding: 0; margin: 0; border: 0; border-radius: 6px; cursor: pointer;
      pointer-events: auto; background: transparent; display: grid; place-items: center; opacity: .9; }
    .nv-icon:hover { opacity: 1; background: rgba(127,127,127,.14); }
    .nv-icon svg { width: 16px; height: 16px; }
    .nv-icon[data-state="match"] svg { color: #a3263a; }
    .nv-icon[data-state="grey"] svg { color: #8a8580; }
    .nv-icon[data-state="warn"] svg { color: #c62828; }
    .nv-menu { position: fixed; pointer-events: auto; border: 0; border-radius: 10px; box-shadow: 0 8px 28px rgba(0,0,0,.28), 0 0 0 1px rgba(0,0,0,.08);
      background: #fbf8f3; overflow: hidden; }
    .nv-menu iframe { border: 0; width: 100%; height: 100%; display: block; color-scheme: normal; }
  `;
  shadow.append(css);
  document.documentElement.append(host);
}

const MARK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l7 3v5c0 4.4-3 8.3-7 9.5C8 19.3 5 15.4 5 11V6z"/><circle cx="12" cy="10.5" r="1.8"/><path d="M12 12.3V15"/></svg>';

function placeIcon(f) {
  const r = f.el.getBoundingClientRect();
  const show = r.width >= ICON_MIN_WIDTH && r.height >= 16 && r.bottom > 0 && r.top < innerHeight && visible(f.el);
  f.icon.style.display = show ? 'grid' : 'none';
  if (!show) return;
  const size = Math.min(22, Math.max(16, r.height - 6));
  f.icon.style.width = f.icon.style.height = `${size}px`;
  const pad = parseFloat(getComputedStyle(f.el).paddingRight) || 0;
  const right = pad > 24 ? pad + 4 : 6;
  f.icon.style.left = `${r.right - right - size}px`;
  f.icon.style.top = `${r.top + (r.height - size) / 2}px`;
}

function placeMenu() {
  if (!menu) return;
  const r = menu.field.getBoundingClientRect();
  let h = menu.h ?? 220;
  let top;
  if (r.bottom + 6 + h <= innerHeight) top = r.bottom + 6;
  else if (r.top - 6 - h >= 0) top = r.top - h - 6;
  else {
    h = Math.min(h, innerHeight - 8);
    top = Math.max(4, innerHeight - h - 4);
  }
  const left = Math.max(6, Math.min(r.left, innerWidth - MENU_W - 6));
  if (menu.at && (menu.at.left !== left || menu.at.top !== top)) menu.movedAt = performance.now();
  menu.at = { left, top };
  menu.wrap.style.left = `${left}px`;
  menu.wrap.style.top = `${top}px`;
  menu.wrap.style.width = `${Math.min(MENU_W, innerWidth - 12)}px`;
  menu.wrap.style.height = `${h}px`;
}

let raf = 0;
function relayout() {
  if (raf) return;
  raf = requestAnimationFrame(() => {
    raf = 0;
    for (const f of fields) placeIcon(f);
    placeMenu();
  });
}

function render() {
  ensureHost();
  for (const f of fields) {
    if (!f.icon) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'nv-icon';
      b.title = 'NiceVault';
      b.setAttribute('aria-label', 'NiceVault');
      b.innerHTML = MARK;
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', (e) => {
        if (!e.isTrusted) return;
        e.preventDefault();
        e.stopPropagation();
        void openMenu(f.el, { gesture: true });
      });
      shadow.append(b);
      f.icon = b;
    }
    f.icon.dataset.state = f.kind === 'otp' && !state.totp ? 'grey' : state.icon;
    f.icon.style.display = state.icon === 'off' ? 'none' : 'grid';
    if (state.icon !== 'off') placeIcon(f);
  }
}

let scanning = 0;
async function rescan() {
  const found = scan();
  const kept = [];
  for (const f of fields) {
    if (found.some((n) => n.el === f.el)) kept.push(f);
    else f.icon?.remove();
  }
  for (const n of found) {
    const old = kept.find((f) => f.el === n.el);
    if (old) Object.assign(old, { kind: n.kind, user: n.user, pass: n.pass });
    else kept.push(n);
  }
  const fresh = kept.length !== fields.length || kept.some((f, i) => f !== fields[i]);
  fields = kept;
  if (!fields.length) return;
  if (fresh) {
    const seq = ++scanning;
    try {
      const r = await api.runtime.sendMessage({ kind: 'frame_fields', otp: fields.some((f) => f.kind === 'otp'), origin: frameOrigin() });
      if (seq === scanning && r) state = r;
    } catch {
    }
  }
  render();
}

let timer = 0;
function soon() {
  clearTimeout(timer);
  timer = setTimeout(() => void rescan(), 350);
}

function fieldOf(el) {
  return fields.find((f) => f.el === el) ?? null;
}

async function openMenu(el, { gesture = false, focus = false } = {}) {
  let f = fieldOf(el);
  if (!f) {
    await rescan();
    f = fieldOf(el);
  }
  if (!f) return false;
  closeMenu();
  const form = f.el.form;
  let formAction = null;
  try {
    formAction = form?.action ? new URL(form.action, location.href).hostname || null : null;
  } catch {
    formAction = null;
  }
  const r = await api.runtime.sendMessage({ kind: 'open_menu', field: f.kind, formAction, gesture, origin: frameOrigin() });
  if (!r?.token) return false;
  ensureHost();
  const wrap = document.createElement('div');
  wrap.className = 'nv-menu';
  const iframe = document.createElement('iframe');
  iframe.src = api.runtime.getURL(`menu.html#${r.token}`);
  iframe.setAttribute('allowtransparency', 'false');
  iframe.title = 'NiceVault';
  wrap.append(iframe);
  shadow.append(wrap);
  menu = { token: r.token, field: f.el, f, wrap, iframe, opened: performance.now(), h: 220 };
  placeMenu();
  if (focus) iframe.addEventListener('load', () => iframe.focus(), { once: true });
  return true;
}

function closeMenu() {
  if (!menu) return;
  menu.wrap.remove();
  menu = null;
}

function menuIntact() {
  if (!menu?.wrap.isConnected) return false;
  if (menu.movedAt && performance.now() - menu.movedAt < 600) return false;
  if (host.getAttribute('style') !== HOST_STYLE) return false;
  for (const el of [host, document.documentElement]) {
    if (!styleIntact(getComputedStyle(el), el === host)) return false;
  }
  const r = menu.wrap.getBoundingClientRect();
  const hit = document.elementFromPoint(r.left + r.width / 2, r.top + Math.min(r.height / 2, 40));
  return hit === host;
}

function fillLogin(f, username, password) {
  const userEl = f.kind === 'password' ? f.user : f.kind === 'username' ? f.el : null;
  const passEl = f.kind === 'password' ? f.el : f.kind === 'username' ? f.pass : null;
  let n = 0;
  if (userEl && visible(userEl) && username) {
    setValue(userEl, username);
    n++;
  }
  if (passEl && visible(passEl) && password) {
    setValue(passEl, password);
    n++;
  }
  return n > 0;
}

function pageField() {
  const a = document.activeElement;
  return fieldOf(a) ?? fields.find((f) => f.kind === 'password') ?? fields.find((f) => f.kind === 'username') ?? null;
}

function frameOrigin() {
  return typeof globalThis.origin === 'string' ? globalThis.origin : location.origin;
}

api.runtime.onMessage.addListener((msg, sender, reply) => {
  if (sender.id !== api.runtime.id) return false;
  if ((msg.kind === 'fill' || msg.kind === 'fill_code') && !acceptFill(msg, frameOrigin())) {
    closeMenu();
    reply({ ok: false, reason: 'origin' });
    return false;
  }
  switch (msg.kind) {
    case 'fill': {
      let f;
      if (msg.token) {
        if (menu?.token !== msg.token || !menuIntact()) {
          closeMenu();
          reply({ ok: false, reason: 'covered' });
          return false;
        }
        f = menu.f;
      } else {
        scan();
        f = pageField();
      }
      const ok = Boolean(f) && fillLogin(f, msg.username, msg.password);
      if (msg.token) closeMenu();
      reply({ ok, reason: ok ? undefined : 'no_field' });
      return false;
    }
    case 'fill_code': {
      if (msg.token && (menu?.token !== msg.token || !menuIntact())) {
        closeMenu();
        reply({ ok: false, reason: 'covered' });
        return false;
      }
      const f = msg.token ? menu.f : fields.find((x) => x.kind === 'otp' && visible(x.el));
      const ok = Boolean(f && visible(f.el));
      if (ok) setValue(f.el, msg.code);
      closeMenu();
      reply({ ok });
      return false;
    }
    case 'close_menu':
      if (menu?.token === msg.token) {
        const el = menu.field;
        closeMenu();
        el.focus({ preventScroll: true });
      }
      return false;
    case 'menu_resize':
      if (menu?.token === msg.token) {
        menu.h = Math.max(80, Math.min(420, Number(msg.h) || 220));
        placeMenu();
      }
      return false;
    case 'shortcut':
    case 'shortcut_totp': {
      if (!document.hasFocus()) return false;
      let a = document.activeElement;
      while (a?.shadowRoot?.activeElement) a = a.shadowRoot.activeElement;
      const f = fieldOf(a);
      if (!f) {
        void rescan();
        return false;
      }
      void openMenu(f.el, { gesture: true, focus: true });
      reply({ ok: true });
      return false;
    }
    case 'context_open':
      if (lastContext) void openMenu(lastContext, { gesture: true, focus: true });
      return false;
    default:
      return false;
  }
});

document.addEventListener('contextmenu', (e) => {
  if (!e.isTrusted) return;
  const p = e.composedPath?.()[0] ?? e.target;
  lastContext = p instanceof HTMLInputElement ? p : null;
}, true);
document.addEventListener('mousedown', (e) => {
  if (menu && e.isTrusted && !e.composedPath().includes(host)) closeMenu();
}, true);
document.addEventListener('keydown', (e) => {
  if (menu && e.key === 'Escape') closeMenu();
  if (menu && e.key === 'ArrowDown' && e.target === menu.field) {
    e.preventDefault();
    menu.iframe.focus();
  }
}, true);
document.addEventListener('focusin', soon, true);
addEventListener('scroll', relayout, { capture: true, passive: true });
addEventListener('resize', relayout, { passive: true });
addEventListener('pagehide', closeMenu);
new MutationObserver(soon).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ['type', 'style', 'class', 'hidden'] });
api.runtime.onMessage.addListener((m) => {
  if (m?.kind === 'volumes_changed') soon();
});
soon();
