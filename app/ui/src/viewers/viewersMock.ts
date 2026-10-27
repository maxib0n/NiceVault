import type { ApiError, NodeId, VaultId } from '../lib/types';
import { safeHref } from './links';

export interface ViewersMockHost {
  file(vault: VaultId, node: NodeId): { name: string; kind: 'file' | 'dir'; path: string; mountpoint: string | null };
  wait(ms: number): Promise<void>;
}

const err = (code: string, message: string): ApiError => ({ code, message });

export const checkedLink = (url: string): string | null => safeHref(url);

const MEDIA = /\.(mp4|m4v|mov|webm|mkv|ogv|avi|mp3|m4a|aac|wav|ogg|oga|opus|flac)$/i;
const RUNS =
  /\.(exe|com|bat|cmd|ps1|psm1|psd1|vbs|vbe|js|jse|wsf|wsh|hta|msi|msp|mst|scr|pif|lnk|cpl|reg|jar|url|inf|scf|application|appref-ms|gadget|msc|msh|xll|settingcontent-ms|library-ms|search-ms|chm|dll|sys|appx|msix|appinstaller|diagcab|py|pyw|pyz|pyc|ws|wsc|sct|ade|adp|mde|accde|xbap|jnlp|vb|msixbundle|appxbundle|desktop|kdesktop|sh|bash|zsh|csh|ksh|tcsh|fish|run|bin|appimage|flatpak|flatpakref|flatpakrepo|snap|deb|rpm|pl|rb|so|ko|x86_64|app|command|tool|terminal|webloc|inetloc|fileloc|pkg|mpkg|scpt|scptd|applescript|workflow|action|prefpane|qlgenerator|kext|osax|saver|plugin|bundle|dylib|mobileconfig|dmg|shortcut|rdp|theme|themepack|deskthemepack|msu|wsb|ps1xml|psc1|msh1|msh2|mshxml|mdb|accdb|cab|iso|img|vhd|vhdx|afploc|ftploc)[. ]*$/i;
export const runsCode = (name: string): boolean => RUNS.test(name);

export class ViewersMock {
  opened: { mounted: string[]; links: string[] } = { mounted: [], links: [] };
  constructor(private host: ViewersMockHost) {}

  async invoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
    switch (cmd) {
      case 'media_url': {
        const f = this.host.file(String(a.vault), Number(a.node));
        if (f.kind !== 'file') throw err('not_a_file', 'Not a file.');
        if (!MEDIA.test(f.name)) throw err('not_media', 'Only audio and video files are played.');
        return null;
      }
      case 'media_release':
        return null;
      case 'fs_open_mounted': {
        await this.host.wait(60);
        const f = this.host.file(String(a.vault), Number(a.node));
        if (f.kind !== 'file') throw err('not_a_file', 'Not a file.');
        if (runsCode(f.name)) throw err('unsafe_type', 'Programs and scripts are not opened from a volume: export the file first.');
        if (!f.mountpoint) throw err('not_mounted', 'The volume is not mounted as a drive.');
        this.opened.mounted.push(f.mountpoint + f.path);
        return null;
      }
      case 'open_link': {
        const u = checkedLink(String(a.url ?? ''));
        if (!u) throw err('bad_link', 'Only web and e-mail links can be opened.');
        this.opened.links.push(u);
        return null;
      }
      default:
        throw err('unknown_command', `Unknown command: ${cmd}`);
    }
  }
}

function mockLog(): string {
  const lvl = ['INFO', 'INFO', 'INFO', 'WARN', 'DEBUG', 'ERROR'];
  const what = ['richiesta servita', 'cache aggiornata', 'backup completato', 'connessione lenta', 'utente collegato', 'timeout del disco'];
  const out: string[] = [];
  let t = Date.UTC(2024, 4, 12, 9, 30, 0);
  for (let i = 1; i <= 16_000; i++) {
    t += 1000 + ((i * 7919) % 5000);
    const k = (i * 31) % lvl.length;
    out.push(`${new Date(t).toISOString()} ${lvl[k].padEnd(5)} [worker-${i % 8}] riga ${i}: ${what[(i * 13) % what.length]} (${(i * 2654435761) % 100000} µs)\n`);
  }
  return out.join('');
}
let logBytes: Uint8Array | null = null;
const logText = () => (logBytes ??= new TextEncoder().encode(mockLog()));
export const MOCK_LOG_SIZE = 1_325_779;
const urls = new Map<string, string>();
export function generatedAsset(key: string): string | null {
  if (typeof URL === 'undefined' || typeof Blob === 'undefined' || !URL.createObjectURL) return null;
  let u = urls.get(key);
  if (!u && key === 'gen:log') urls.set(key, (u = URL.createObjectURL(new Blob([logText() as Uint8Array<ArrayBuffer>], { type: 'text/plain' }))));
  return u ?? null;
}
export const mockLogBytes = logText;
