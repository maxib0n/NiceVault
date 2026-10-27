export function otpLike(a) {
  const ac = (a.autocomplete ?? '').toLowerCase();
  if (ac.includes('one-time-code')) return true;
  if (ac && /\b(username|email|current-password|new-password|cc-|tel|address|name)\b/.test(ac)) return false;
  const type = (a.type ?? 'text').toLowerCase();
  if (!['text', 'tel', 'number', ''].includes(type)) return false;
  const max = Number(a.maxlength ?? a.maxLength ?? 0);
  const numeric = /^(numeric|decimal)$/i.test(a.inputmode ?? '') || type === 'number' || type === 'tel';
  if (numeric && max >= 6 && max <= 8) return true;
  const name = `${a.name ?? ''} ${a.id ?? ''} ${a.placeholder ?? ''}`.toLowerCase();
  return /\b(otp|totp|2fa|mfa|one.?time|verification.?code|auth.?code|security.?code|codice)\b|otp|totp/.test(name) && (!max || (max >= 4 && max <= 10));
}

export function userLike(a) {
  const ac = (a.autocomplete ?? '').toLowerCase();
  if (/\b(username|email)\b/.test(ac)) return true;
  const type = (a.type ?? 'text').toLowerCase();
  if (!['text', 'email', 'tel', ''].includes(type)) return false;
  if (otpLike(a)) return false;
  const name = `${a.name ?? ''} ${a.id ?? ''}`.toLowerCase();
  return !/search|query|captcha|coupon|promo/.test(name);
}

function attrs(el) {
  return {
    type: el.getAttribute('type') ?? 'text',
    autocomplete: el.getAttribute('autocomplete') ?? '',
    inputmode: el.getAttribute('inputmode') ?? '',
    maxlength: el.getAttribute('maxlength') ?? '',
    name: el.getAttribute('name') ?? '',
    id: el.id ?? '',
    placeholder: el.getAttribute('placeholder') ?? '',
  };
}

export function visible(el) {
  if (!el.isConnected || el.disabled) return false;
  const r = el.getBoundingClientRect();
  if (r.width < 2 || r.height < 2) return false;
  for (let n = el; n && n.nodeType === 1; n = n.parentElement ?? n.getRootNode()?.host) {
    const s = getComputedStyle(n);
    if (s.display === 'none' || s.visibility === 'hidden' || s.visibility === 'collapse' || Number(s.opacity) < 0.1) return false;
  }
  return true;
}

export function inputs(root = document, out = [], budget = { hosts: 300 }) {
  for (const el of root.querySelectorAll('input')) out.push(el);
  if (budget.hosts > 0) {
    for (const el of root.querySelectorAll('*')) {
      if (el.shadowRoot && budget.hosts-- > 0) inputs(el.shadowRoot, out, budget);
    }
  }
  return out;
}

export function scan(isVisible = visible) {
  const all = inputs().filter((el) => !el.readOnly && isVisible(el));
  const out = [];
  const pws = all.filter((el) => (el.getAttribute('type') ?? '').toLowerCase() === 'password');
  const scopeOf = (el) => el.form ?? el.getRootNode();
  for (const pw of pws) {
    const scope = scopeOf(pw);
    const before = all.filter((el) => scopeOf(el) === scope && el !== pw && (pw.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_PRECEDING) && userLike(attrs(el)));
    const user = before.find((el) => /\b(username|email)\b/i.test(el.getAttribute('autocomplete') ?? '')) ?? before[before.length - 1] ?? null;
    out.push({ el: pw, kind: 'password', user });
    if (user && !out.some((f) => f.el === user)) out.push({ el: user, kind: 'username', pass: pw });
  }
  for (const el of all) {
    if (out.some((f) => f.el === el)) continue;
    const a = attrs(el);
    if (otpLike(a)) out.push({ el, kind: 'otp' });
    else if (/\busername\b/i.test(a.autocomplete) && !pws.some((p) => scopeOf(p) === scopeOf(el))) out.push({ el, kind: 'username', pass: null });
  }
  return out;
}

export function setValue(el, v) {
  const proto = Object.getPrototypeOf(el);
  const desc = Object.getOwnPropertyDescriptor(proto, 'value') ?? Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
  el.focus({ preventScroll: true });
  desc.set.call(el, v);
  el.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
}

export function acceptFill(msg, origin) {
  return typeof msg?.expect === 'string' && msg.expect !== 'null' && msg.expect === origin;
}

export function styleIntact(cs, own) {
  const none = (v) => v == null || v === '' || v === 'none';
  if (Number(cs.opacity ?? 1) < 1) return false;
  if (!none(cs.filter) || !none(cs.clipPath) || !none(cs.maskImage) || !none(cs.mask) || !none(cs.transform)) return false;
  if (cs.zoom != null && cs.zoom !== '' && cs.zoom !== 'normal' && Number(cs.zoom) !== 1) return false;
  if (cs.mixBlendMode != null && cs.mixBlendMode !== '' && cs.mixBlendMode !== 'normal') return false;
  if (own && (cs.visibility !== 'visible' || cs.display === 'none')) return false;
  return true;
}
