import type { BrowserAccess, BrowserInfo, PairAnswer, PairPrompt, PairedBrowser } from './browserLink';
import type { EventMap, EventName } from './types';

interface Deps {
  emit: <E extends EventName>(event: E, payload: EventMap[E]) => void;
  installed: () => boolean;
  volumes: () => { path: string; name: string }[];
}

export class BrowserLinkMock {
  link = false;
  defaultVolume: string | null = null;
  paired: PairedBrowser[] = [];
  accesses: BrowserAccess[] = [];
  pending: PairPrompt | null = null;
  sandboxed: string[] = [];

  constructor(private deps: Deps, scenario: string | null = null) {
    const now = Date.now();
    if (scenario === 'paired' || scenario === 'pair') {
      this.paired = [{ id: 'b1', label: 'Firefox 139 · Linux', created: now - 9 * 864e5, last_used: now - 36e5 }];
      this.accesses = [
        { at: now - 6e4, site: 'mail.example.com', title: 'Posta', kind: 'fill', browser: 'Firefox 139 · Linux', refused: false },
        { at: now - 5e4, site: 'mail.example.com', title: 'Posta', kind: 'totp', browser: 'Firefox 139 · Linux', refused: false },
      ];
    }
    if (scenario === 'sandbox') this.sandboxed = ['Firefox (Flatpak)'];
    if (scenario === 'pair') setTimeout(() => this.prompt(), 400);
  }

  info(): BrowserInfo {
    const installed = this.deps.installed();
    return {
      link: installed && this.link,
      portable: !installed,
      bridge: installed ? '/usr/bin/nicevault' : null,
      registered: installed && this.link
        ? [
            { browser: 'Google Chrome', path: '/home/utente/.config/google-chrome/NativeMessagingHosts/app.nicevault.bridge.json' },
            { browser: 'Firefox', path: '/home/utente/.mozilla/native-messaging-hosts/app.nicevault.bridge.json' },
          ]
        : [],
      error: null,
      sandboxed: this.sandboxed,
      paired: this.paired.map((p) => ({ ...p })),
      accesses: this.accesses.map((a) => ({ ...a })),
      default_volume: this.defaultVolume,
      volumes: this.deps.volumes(),
      pending: this.pending,
    };
  }

  prompt(label = 'Chrome 156 · Linux', code = '482913') {
    this.pending = { id: 'pp' + Date.now(), label, code, caller: 'chrome-extension://npegemkakgbadljnibnipnaleoffjfbf/' };
    this.deps.emit('browser-pair', this.pending);
  }

  async invoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
    switch (cmd) {
      case 'browser_info':
        return this.info();
      case 'browser_set': {
        const p = (a.patch ?? {}) as { link?: boolean; default_volume?: string };
        if (typeof p.link === 'boolean' && this.deps.installed()) this.link = p.link;
        if (typeof p.default_volume === 'string') this.defaultVolume = p.default_volume || null;
        return this.info();
      }
      case 'browser_unpair':
        this.paired = this.paired.filter((p) => p.id !== a.id);
        return this.info();
      case 'browser_pair_answer': {
        if (!this.pending || this.pending.id !== a.id) return false;
        const p = this.pending;
        this.pending = null;
        if ((a.answer as PairAnswer) === 'accept') this.paired.push({ id: 'b' + (this.paired.length + 2), label: p.label, created: Date.now(), last_used: Date.now() });
        this.deps.emit('browser-pair-closed', { id: p.id });
        this.deps.emit('browser-changed', null);
        return true;
      }
      case 'browser_unlock_cancel':
        return null;
      default:
        throw { code: 'unknown_command', message: `Unknown command: ${cmd}` };
    }
  }
}
