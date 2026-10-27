const MAIL_KEYS = new Set(['subject', 'body', 'cc', 'bcc']);
// eslint-disable-next-line no-control-regex
const MAIL_CTRL = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/;

export function safeHref(raw: string): string | null {
  const h = raw.trim().replace(/^<|>$/g, '');
  if (!/^(https?:\/\/|mailto:)/i.test(h) || /[\s"<>\\`$]/.test(h) || h.length > 2048) return null;
  let u: URL;
  try {
    u = new URL(h);
  } catch {
    return null;
  }
  if (u.protocol === 'http:' || u.protocol === 'https:') return u.username || u.password || !u.hostname ? null : u.href;
  if (u.protocol !== 'mailto:' || !/^[A-Za-z0-9@.\-_+,!#'*^{}~]*$/.test(u.pathname)) return null;
  const q: string[] = [];
  for (const part of u.search.slice(1).split('&')) {
    const i = part.indexOf('=');
    const k = (i < 0 ? part : part.slice(0, i)).toLowerCase();
    if (!MAIL_KEYS.has(k)) continue;
    let v: string;
    try {
      v = decodeURIComponent(i < 0 ? '' : part.slice(i + 1));
    } catch {
      return null;
    }
    if (MAIL_CTRL.test(v)) return null;
    q.push(`${k}=${encodeURIComponent(v)}`);
  }
  return `mailto:${u.pathname}${q.length ? '?' + q.join('&') : ''}`;
}

export function localImagePath(src: string): string | null {
  const s = src.trim();
  if (!s || s.length > 1024 || /^[a-z][a-z0-9+.-]*:/i.test(s) || s.startsWith('//') || s.startsWith('\\')) return null;
  let p = s.replace(/[?#].*$/, '');
  try {
    p = decodeURIComponent(p);
  } catch {
    return null;
  }
  // eslint-disable-next-line no-control-regex
  return p && !/[\u0000-\u001f]/.test(p) ? p : null;
}

export function resolveVolumePath(dir: string, rel: string): string[] | null {
  const out = rel.startsWith('/') ? [] : dir.split('/').filter(Boolean);
  for (const part of rel.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (!out.length) return null;
      out.pop();
    } else out.push(part);
  }
  return out.length ? out : null;
}

