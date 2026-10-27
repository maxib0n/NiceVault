import { Session, HOST } from './lib/session.js';
import { keystore } from './lib/keystore.js';
import { t } from './lib/i18n.js';
import { allowedFrom, originOf } from './lib/origin.js';

const api = globalThis.browser ?? globalThis.chrome;
const DEFAULTS = { launch: 'auto', fieldIcon: true, siteMemory: true, copyTotp: false };
const TOTP_KEEP_MS = 5 * 60 * 1000;
const TOKEN_KEEP_MS = 10 * 60 * 1000;

let platform = 'linux';
api.runtime.getPlatformInfo?.().then((p) => (platform = p.os)).catch(() => {});

async function browserLabel() {
  const os = { linux: 'Linux', win: 'Windows', mac: 'macOS', openbsd: 'OpenBSD' }[platform] ?? platform;
  try {
    if (api.runtime.getBrowserInfo) {
      const b = await api.runtime.getBrowserInfo();
      return `${b.name} ${String(b.version).split('.')[0]} · ${os}`;
    }
  } catch {
  }
  const brands = globalThis.navigator?.userAgentData?.brands ?? [];
  const b = brands.find((x) => !/not.?a.?brand|chromium/i.test(x.brand)) ?? brands.find((x) => /chromium/i.test(x.brand));
  return b ? `${b.brand} ${b.version} · ${os}` : `Browser · ${os}`;
}

async function opts() {
  const o = (await api.storage.local.get('opts')).opts ?? {};
  return { ...DEFAULTS, ...o };
}

const session = new Session({
  connect: () => api.runtime.connectNative(HOST),
  lastError: () => api.runtime.lastError?.message,
  store: keystore,
  onPush: (m) => {
    if (m.type === 'volumes_changed') void volumesChanged();
  },
  onState: () => broadcast({ kind: 'state_changed' }),
});

function broadcast(m) {
  api.runtime.sendMessage(m).catch(() => {});
}

async function tabUrl(tabId) {
  try {
    return (await api.tabs.get(tabId)).url ?? '';
  } catch {
    return '';
  }
}

let statusCache = null;
async function status() {
  if (statusCache && Date.now() - statusCache.at < 1500) return statusCache.v;
  const v = await session.request('status');
  statusCache = { at: Date.now(), v };
  return v;
}

let waiting = null;

async function volumesChanged() {
  statusCache = null;
  if (waiting) {
    try {
      const s = await status();
      if (s.volumes.some((v) => v.alias === waiting.alias && v.state === 'open')) waiting = null;
    } catch {
    }
  }
  broadcast({ kind: 'volumes_changed' });
}

async function suggestions(site, lockedAliases, volumes) {
  const o = await opts();
  const mem = o.siteMemory ? ((await api.storage.local.get('memory')).memory ?? {}) : {};
  const last = (await api.storage.local.get('lastUnlock')).lastUnlock;
  const locked = volumes.filter((v) => lockedAliases.includes(v.alias));
  const order = [mem[site], ...locked.filter((v) => v.default).map((v) => v.alias), last].filter(Boolean);
  locked.sort((a, b) => {
    const ia = order.indexOf(a.alias);
    const ib = order.indexOf(b.alias);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
  });
  return { list: locked, remembered: Boolean(mem[site] && locked[0]?.alias === mem[site]) };
}

async function view(ctx, { gesture = false, query = null } = {}) {
  const o = await opts();
  const before = session.state;
  let st = await session.ensure({ launch: gesture && o.launch === 'auto' });
  const base = { os: platform, launchAsk: o.launch === 'ask' };
  if (st !== 'ready') {
    const map = { not_running: 'closed', idle: 'closed' };
    return { ...base, state: map[st] ?? st };
  }
  try {
    const s = await status();
    if (query != null) {
      const r = await session.request('search', { query, frame: ctx.frame });
      return { ...base, state: 'search', entries: r.entries, lookalike: r.lookalike, volumes: s.volumes };
    }
    const m = await session.request('match', { top: ctx.top, frame: ctx.frame });
    const sug = await suggestions(m.site, m.locked ?? [], s.volumes);
    const v = {
      ...base,
      site: m.site,
      host: m.host,
      host_unicode: m.host_unicode,
      insecure_page: m.insecure_page,
      cross: m.cross_site ? { frame: m.host, top: m.top_host } : null,
      lookalike: m.lookalike,
      entries: m.entries ?? [],
      locked: sug.list,
      volumes: s.volumes,
      blocked: m.blocked,
      formAction: ctx.formAction && m.site && !hostIn(ctx.formAction, m.site) ? ctx.formAction : null,
    };
    if (ctx.kind === 'otp') {
      const p = await totpPending(ctx.tabId);
      if (p) v.totp = { title: p.title };
    }
    if (m.blocked) v.state = 'none';
    else if (m.lookalike) v.state = 'lookalike';
    else if (v.entries.length) v.state = 'matches';
    else if (waiting && s.volumes.some((x) => x.alias === waiting.alias && x.state !== 'open') && Date.now() < waiting.until) v.state = 'waiting';
    else if (s.volumes.length && s.volumes.every((x) => x.state !== 'open')) v.state = 'locked';
    else v.state = 'none';
    if (gesture && before !== 'ready' && v.state === 'locked' && v.locked[0]?.state === 'locked') {
      await unlock(v.locked[0].alias);
      v.state = 'waiting';
    }
    return v;
  } catch (e) {
    return { ...base, state: session.state === 'ready' ? 'error' : (session.state === 'not_running' ? 'closed' : session.state), error: e?.code };
  }
}

function hostIn(host, site) {
  return host === site || host.endsWith('.' + site);
}

async function unlock(alias) {
  const r = await session.request('unlock_request', alias ? { alias } : {});
  if (r.shown && alias) {
    waiting = { alias, until: Date.now() + 5 * 60 * 1000 };
    await api.storage.local.set({ lastUnlock: alias });
  }
  return r;
}

async function totpPending(tabId) {
  const k = `totp:${tabId}`;
  const p = (await api.storage.session.get(k))[k];
  if (!p || p.until < Date.now()) return null;
  return p;
}

const tokens = new Map();

function newToken(ctx) {
  const tok = Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, '0')).join('');
  tokens.set(tok, { ...ctx, at: Date.now() });
  for (const [k, v] of tokens) if (Date.now() - v.at > TOKEN_KEEP_MS) tokens.delete(k);
  return tok;
}

async function fill(ctx, ref, confirm, entryMeta) {
  const r = await session.request('fill', { ref, frame: ctx.frame, top: ctx.top, confirm });
  const res = await api.tabs
    .sendMessage(ctx.tabId, { kind: 'fill', token: ctx.token, expect: ctx.frame, username: r.username, password: r.password, kind2: ctx.kind }, { frameId: ctx.frameId })
    .catch(() => ({ ok: false }));
  if (!res?.ok) return { ok: false, reason: res?.reason ?? 'covered' };
  const o = await opts();
  if (o.siteMemory && entryMeta?.alias && ctx.site) {
    const mem = (await api.storage.local.get('memory')).memory ?? {};
    mem[ctx.site] = entryMeta.alias;
    await api.storage.local.set({ memory: mem });
  }
  if (r.has_totp) {
    await api.storage.session.set({ [`totp:${ctx.tabId}`]: { ref, title: r.title, until: Date.now() + TOTP_KEEP_MS, frame: ctx.frame } });
    if (o.copyTotp) await session.request('copy', { ref, field: 'totp' }).catch(() => {});
  }
  return { ok: true };
}

async function fillTotp(ctx, ref) {
  const p = ref ? { ref } : await totpPending(ctx.tabId);
  if (!p) return { ok: false, reason: 'no_totp' };
  const r = await session.request('totp', { ref: p.ref, frame: ctx.frame, top: ctx.top });
  const res = await api.tabs.sendMessage(ctx.tabId, { kind: 'fill_code', token: ctx.token, expect: ctx.frame, code: r.code }, { frameId: ctx.frameId }).catch(() => ({ ok: false }));
  if (res?.ok) await api.storage.session.remove(`totp:${ctx.tabId}`);
  return res?.ok ? { ok: true } : { ok: false, reason: res?.reason ?? 'covered' };
}

function errorReply(e) {
  return { ok: false, reason: e?.code ?? 'error' };
}

async function onMessage(msg, sender) {
  const tabId = sender.tab?.id;
  switch (msg.kind) {
    case 'frame_fields': {
      const o = await opts();
      if (!o.fieldIcon) return { icon: 'off' };
      if (session.state !== 'ready') {
        const st = await session.ensure({ launch: false });
        if (st !== 'ready') return { icon: 'grey', state: st };
      }
      try {
        const m = await session.request('match', { top: await tabUrl(tabId), frame: originOf(sender, msg.origin) });
        const n = m.entries?.length ?? 0;
        if (sender.frameId === 0) api.action.setBadgeText({ tabId, text: n ? String(n) : '' }).catch(() => {});
        const totp = msg.otp ? await totpPending(tabId) : null;
        return { icon: m.lookalike ? 'warn' : n || totp ? 'match' : 'grey', count: n, totp: Boolean(totp) };
      } catch {
        return { icon: 'grey' };
      }
    }
    case 'open_menu': {
      const frame = originOf(sender, msg.origin);
      const top = await tabUrl(tabId);
      const token = newToken({ tabId, frameId: sender.frameId, frame, top, kind: msg.field ?? 'login', formAction: msg.formAction ?? null, gesture: Boolean(msg.gesture) });
      return { token };
    }
    case 'menu_view':
    case 'menu_search': {
      const ctx = tokens.get(msg.token);
      if (!ctx || ctx.tabId !== tabId) return { state: 'error' };
      const gesture = msg.kind === 'menu_view' && ctx.gesture && !ctx.used;
      ctx.used = true;
      const v = await view(ctx, { gesture, query: msg.kind === 'menu_search' ? msg.query : null });
      ctx.site = v.site ?? ctx.site;
      return v;
    }
    case 'menu_fill': {
      const ctx = tokens.get(msg.token);
      if (!ctx || ctx.tabId !== tabId) return { ok: false, reason: 'error' };
      try {
        return msg.totp ? await fillTotp({ ...ctx, token: msg.token }, msg.ref) : await fill({ ...ctx, token: msg.token }, msg.ref, msg.confirm ?? [], msg.meta);
      } catch (e) {
        return errorReply(e);
      }
    }
    case 'menu_launch': {
      const ctx = tokens.get(msg.token);
      if (!ctx || ctx.tabId !== tabId) return { state: 'error' };
      await session.ensure({ launch: true });
      return view(ctx, { gesture: true });
    }
    case 'menu_unlock':
    case 'popup_unlock':
      if (msg.kind === 'menu_unlock') {
        const ctx = tokens.get(msg.token);
        if (!ctx || ctx.tabId !== tabId) return { ok: false, reason: 'error' };
      }
      try {
        await unlock(msg.alias);
        return { ok: true };
      } catch (e) {
        return errorReply(e);
      }
    case 'menu_resize': {
      const ctx = tokens.get(msg.token);
      if (ctx && ctx.tabId === tabId) api.tabs.sendMessage(ctx.tabId, { kind: 'menu_resize', token: msg.token, h: msg.h }, { frameId: ctx.frameId }).catch(() => {});
      return { ok: true };
    }
    case 'menu_close': {
      const ctx = tokens.get(msg.token);
      if (ctx && ctx.tabId === tabId) api.tabs.sendMessage(ctx.tabId, { kind: 'close_menu', token: msg.token }, { frameId: ctx.frameId }).catch(() => {});
      return { ok: true };
    }
    case 'popup_view':
    case 'popup_search': {
      const tab = (await api.tabs.query({ active: true, currentWindow: true }))[0];
      if (!tab) return { state: 'error' };
      const ctx = { tabId: tab.id, frameId: 0, frame: safeOrigin(tab.url), top: tab.url ?? '', kind: 'login' };
      const v = await view(ctx, { gesture: msg.kind === 'popup_view', query: msg.kind === 'popup_search' ? msg.query : null });
      return { ...v, hasTab: true };
    }
    case 'popup_fill': {
      const tab = (await api.tabs.query({ active: true, currentWindow: true }))[0];
      if (!tab) return { ok: false };
      const ctx = { tabId: tab.id, frameId: 0, frame: safeOrigin(tab.url), top: tab.url ?? '', kind: 'login', token: null, site: msg.site };
      try {
        return await fill(ctx, msg.ref, msg.confirm ?? [], msg.meta);
      } catch (e) {
        return errorReply(e);
      }
    }
    case 'popup_copy':
      try {
        const r = await session.request('copy', { ref: msg.ref, field: msg.field });
        return { ok: true, clear_after: r.clear_after };
      } catch (e) {
        return errorReply(e);
      }
    case 'pair_start': {
      const label = await browserLabel();
      const r = await session.pair(label, (code) => broadcast({ kind: 'pair_code', code }));
      statusCache = null;
      return { result: r };
    }
    case 'show_app':
      try {
        await session.request('show', {}, { launch: true });
        return { ok: true };
      } catch (e) {
        return errorReply(e);
      }
    case 'get_state': {
      const st = await session.ensure({ launch: false });
      return { state: st, app: session.app, os: platform };
    }
    case 'unpair_local':
      await keystore.clear();
      session.disconnect();
      return { ok: true };
    case 'forget_memory':
      await api.storage.local.remove(['memory', 'lastUnlock']);
      return { ok: true };
    default:
      return undefined;
  }
}

function safeOrigin(u) {
  try {
    return new URL(u).origin;
  } catch {
    return 'null';
  }
}

api.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg || typeof msg.kind !== 'string' || !allowedFrom(msg.kind, sender, api.runtime.getURL(''), api.runtime.id)) return false;
  onMessage(msg, sender).then(sendResponse, () => sendResponse({ ok: false, reason: 'error' }));
  return true;
});

api.commands?.onCommand.addListener(async (command, tab) => {
  tab = tab ?? (await api.tabs.query({ active: true, currentWindow: true }))[0];
  if (!tab?.id) return;
  const kind = command === 'fill-totp' ? 'shortcut_totp' : 'shortcut';
  const hits = await api.tabs.sendMessage(tab.id, { kind }).catch(() => null);
  if (!hits?.ok && command === 'open-menu') api.action.openPopup?.().catch(() => {});
});

function installMenus() {
  const menus = api.contextMenus ?? api.menus;
  if (!menus) return;
  menus.removeAll(() => {
    menus.create({ id: 'nv-fill', title: t('ctx_fill'), contexts: ['editable'] }, () => void api.runtime.lastError);
  });
}
api.runtime.onInstalled.addListener((d) => {
  installMenus();
  if (d?.reason === 'install') void api.tabs.create({ url: api.runtime.getURL('pair.html') });
});
api.runtime.onStartup?.addListener(installMenus);
(api.contextMenus ?? api.menus)?.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== 'nv-fill' || !tab?.id) return;
  api.tabs.sendMessage(tab.id, { kind: 'context_open' }, { frameId: info.frameId ?? 0 }).catch(() => {});
});

api.tabs.onRemoved.addListener((tabId) => void api.storage.session.remove(`totp:${tabId}`));
