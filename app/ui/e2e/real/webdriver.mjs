const ELEMENT = 'element-6066-11e4-a52e-4f735466cecf';

export function elementId(el) {
  if (typeof el === 'string') return el;
  if (el && typeof el === 'object' && el[ELEMENT]) return el[ELEMENT];
  throw new Error('not an element: ' + JSON.stringify(el));
}

export class WebDriver {
  constructor(base) {
    this.base = base.replace(/\/$/, '');
    this.session = null;
  }
  async cmd(method, path, body) {
    const url = this.session ? `${this.base}/session/${this.session}${path}` : `${this.base}${path}`;
    const res = await fetch(url, {
      method,
      headers: { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const json = await res.json().catch(() => ({}));
    if (!res.ok) {
      const v = json.value ?? {};
      throw new Error(`WebDriver ${method} ${path}: ${v.error ?? res.status} ${v.message ?? ''}`.trim());
    }
    return json.value;
  }
  async start(capabilities) {
    const v = await this.cmd('POST', '/session', { capabilities: { alwaysMatch: capabilities } });
    this.session = v.sessionId;
    return v;
  }
  async quit() {
    if (!this.session) return;
    try {
      await this.cmd('DELETE', '');
    } finally {
      this.session = null;
    }
  }
  exec(script, ...args) {
    return this.cmd('POST', '/execute/sync', { script, args });
  }
  execAsync(script, ...args) {
    return this.cmd('POST', '/execute/async', { script, args });
  }
  async find(css) {
    const v = await this.cmd('POST', '/element', { using: 'css selector', value: css });
    return v[ELEMENT];
  }
  click(el) {
    return this.cmd('POST', `/element/${elementId(el)}/click`, {});
  }
  sendKeys(el, text) {
    return this.cmd('POST', `/element/${elementId(el)}/value`, { text });
  }
  clear(el) {
    return this.cmd('POST', `/element/${elementId(el)}/clear`, {});
  }
  async screenshot() {
    return Buffer.from(await this.cmd('GET', '/screenshot'), 'base64');
  }
  async keys(...keys) {
    const actions = [];
    for (const k of keys) actions.push({ type: 'keyDown', value: k }, { type: 'keyUp', value: k });
    await this.cmd('POST', '/actions', { actions: [{ type: 'key', id: 'kbd', actions }] });
    await this.cmd('DELETE', '/actions');
  }
  async chord(mods, key) {
    const actions = [...mods.map((m) => ({ type: 'keyDown', value: m })), { type: 'keyDown', value: key }, { type: 'keyUp', value: key }, ...mods.reverse().map((m) => ({ type: 'keyUp', value: m }))];
    await this.cmd('POST', '/actions', { actions: [{ type: 'key', id: 'kbd', actions }] });
    await this.cmd('DELETE', '/actions');
  }
  get title() {
    return this.cmd('GET', '/title');
  }
}

export const KEY = { Enter: '', Escape: '', Backspace: '', Delete: '', Space: ' ', ArrowDown: '', ArrowUp: '', ArrowRight: '', ArrowLeft: '', F2: '', Control: '', Shift: '', Home: '', End: '', Tab: '' };
