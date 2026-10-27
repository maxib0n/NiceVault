import { BrowserLinkMock } from './browserLinkMock';
import type {
  ApiError,
  AppearanceState,
  AutoLockStatus,
  Backend,
  Drive,
  Emblem,
  EmblemState,
  EventMap,
  EventName,
  FsNode,
  JobId,
  LockReason,
  NodeId,
  NodeProps,
  RecentEntry,
  Settings,
  ThemeId,
  SlotInfo,
  VaultId,
  VaultInfo,
  VaultStats,
  Visibility,
} from './types';
import { ROOT } from './types';
import type { OsDragEvent } from './api';
import { DEFAULT_ACCENT, isAccent } from './accents';
import { LOCKED_IMAGE_BYTES, MAX_IMAGE_BYTES } from './emblem';
import { MOCK_EMBLEM_WEBP } from './mockEmblem';
import { PmMock } from './pmMock';
import { SearchMock } from './searchMock';
import { matchesFilters } from './search';
import { DeviceMock } from './deviceMock';
import { StickMock, type StickScenario } from './stickMock';
import { ResizeMock } from './resizeMock';
import { MOCK_LOG_SIZE, ViewersMock, generatedAsset, runsCode } from '../viewers/viewersMock';
import { switches } from './switches';
import { DEMO_DOCUMENTS, DEMO_HOME, DEMO_JOURNAL, DEMO_NAMES, DEMO_SLOTS, DEMO_STICK, DEMO_STICK_LABEL, DEMO_TAX } from './mockDemo';
import { isGalleryMedia } from './gallery';

export const MOCK_PASSWORD = 'demo';
export const STICK = switches.demo ? DEMO_STICK : '/run/media/utente/CHIAVETTA';

export interface MockOptions {
  speed: number;
  empty: boolean;
  mountUnavailable: boolean;
  mounted: boolean;
  mountOs: string | null;
  importFails: boolean;
  deleteFails?: boolean;
  slowDelete?: number | null;
  singleSlot: boolean;
  unlockMs: number;
  emblem: 'initial' | 'icon' | 'image' | 'hidden' | null;
  appearance: Visibility | null;
  frame: string | null;
  updateOnStart: boolean;
  updateMode: 'replaced' | 'downloaded';
  pmEntries: number;
  device: 'empty' | 'incomplete' | null;
  deviceFail: string | null;
  deviceOffline: boolean;
  deviceSlow: 'candidates' | 'options' | 'plan' | 'create' | 'auth' | null;
  deviceVolumes: boolean;
  stick: StickScenario;
  stickOffline: boolean;
  stickFail: string | null;
  fixed: boolean;
  resizeFail: string | null;
  fmtFull: boolean;
}

const DEFAULTS: MockOptions = {
  speed: 1,
  empty: false,
  mountUnavailable: false,
  mounted: false,
  mountOs: null,
  importFails: false,
  singleSlot: false,
  unlockMs: 700,
  device: null,
  deviceFail: null,
  deviceOffline: false,
  deviceSlow: null,
  deviceVolumes: false,
  stick: null,
  stickOffline: false,
  stickFail: null,
  fixed: false,
  resizeFail: null,
  fmtFull: false,
  emblem: null,
  appearance: null,
  frame: null,
  updateOnStart: false,
  updateMode: 'replaced',
  pmEntries: 0,
};

export const MOCK_UPDATE = {
  current: '0.1.0',
  version: '0.2.0',
  notes: {
    it: 'Sblocco più veloce sulle chiavette lente.\n- Corretto l’ordine dei file per data.',
    en: 'Faster unlock on slow sticks.\n- Fixed sorting files by date.',
  },
  pub_date: '2026-10-24T12:00:00Z',
  size: 18_400_000,
};

const THEMES = ['system', 'light', 'dark'] as const;
const LOCK_SCOPES = ['volume', 'all'] as const;
const GRID_SIZES = ['s', 'm', 'l', 'xl'] as const;
const LIST_DENSITIES = ['compact', 'normal', 'comfortable'] as const;
const DEFAULT_SETTINGS: Settings = {
  auto_lock_minutes: 15,
  accent: DEFAULT_ACCENT,
  theme: 'dark',
  lock_on_sleep: true,
  lock_on_screen_lock: true,
  lock_scope: 'volume',
  grid_size: 'm',
  list_density: 'normal',
  browser_enabled: true,
};

export const MOCK_DURESS_PASSWORD = 'panico';
const DURESS_CONFLICT = 'A self-destruct password must differ from every other password of the volume.';

function applySettings(base: Settings, patch: Partial<Settings> | undefined): Settings {
  const s = { ...DEFAULT_SETTINGS, ...base };
  if (!patch) return s;
  if (typeof patch.auto_lock_minutes === 'number') s.auto_lock_minutes = Math.min(24 * 60, Math.max(0, Math.round(patch.auto_lock_minutes)));
  if (isAccent(patch.accent)) s.accent = patch.accent;
  if ((THEMES as readonly string[]).includes(patch.theme as string)) s.theme = patch.theme as Settings['theme'];
  if (typeof patch.lock_on_sleep === 'boolean') s.lock_on_sleep = patch.lock_on_sleep;
  if (typeof patch.lock_on_screen_lock === 'boolean') s.lock_on_screen_lock = patch.lock_on_screen_lock;
  if ((LOCK_SCOPES as readonly string[]).includes(patch.lock_scope as string)) s.lock_scope = patch.lock_scope as Settings['lock_scope'];
  if ((GRID_SIZES as readonly string[]).includes(patch.grid_size as string)) s.grid_size = patch.grid_size as Settings['grid_size'];
  if ((LIST_DENSITIES as readonly string[]).includes(patch.list_density as string)) s.list_density = patch.list_density as Settings['list_density'];
  if (typeof patch.browser_enabled === 'boolean') s.browser_enabled = patch.browser_enabled;
  return s;
}

const SETTINGS_STORE = 'nv.mock.volume-settings';

function loadStoredSettings(): Record<string, Settings> {
  try {
    return JSON.parse(localStorage.getItem(SETTINGS_STORE) ?? '{}') as Record<string, Settings>;
  } catch {
    return {};
  }
}

function storeSettings(path: string, s: Settings): void {
  try {
    localStorage.setItem(SETTINGS_STORE, JSON.stringify({ ...loadStoredSettings(), [path]: s }));
  } catch {
  }
}

interface MockNode extends FsNode {
  asset?: string;
  created?: number | null;
  imported?: number | null;
  origin?: string | null;
  sha256?: string | null;
}

const ASSET_INFO: Record<string, { mime: string; width?: number; height?: number; duration_ms?: number }> = {
  '/mock/photo-01.jpg': { mime: 'image/jpeg', width: 960, height: 659 },
  '/mock/photo-02.jpg': { mime: 'image/jpeg', width: 960, height: 638 },
  '/mock/photo-03.jpg': { mime: 'image/jpeg', width: 853, height: 1280 },
  '/mock/photo-08.jpg': { mime: 'image/jpeg', width: 960, height: 960 },
  '/mock/scan.jpg': { mime: 'image/jpeg', width: 320, height: 440 },
  '/mock/card.png': { mime: 'image/png', width: 428, height: 270 },
  '/mock/clip.webm': { mime: 'video/webm', duration_ms: 4000 },
  '/mock/tone.ogg': { mime: 'audio/opus', duration_ms: 3007 },
  '/mock/document.pdf': { mime: 'application/pdf' },
  '/mock/notes.md': { mime: 'text/plain' },
  '/mock/readme.txt': { mime: 'text/plain' },
  '/mock/recipe.txt': { mime: 'text/plain' },
  '/mock/key.pub': { mime: 'text/plain' },
};

function fakeSha(seed: string): string {
  let h = 2166136261;
  let out = '';
  for (let i = 0; out.length < 64; i++) {
    h ^= seed.charCodeAt(i % Math.max(1, seed.length)) + i;
    h = Math.imul(h, 16777619) >>> 0;
    out += (h >>> 0).toString(16).padStart(8, '0');
  }
  return out.slice(0, 64);
}

interface MockVolume {
  path: string;
  name: string;
  password: string;
  nodes: Map<NodeId, MockNode>;
  nextId: number;
  slots: SlotInfo[];
  emblem: EmblemState;
  settings: Settings;
  lockedImage: string | null;
  duress: Map<number, string>;
  destroyed: boolean;
  singleFile?: boolean;
  fixed?: boolean;
  rolledBack?: { seen: number; now: number } | null;
}

interface OpenVault {
  id: VaultId;
  path: string;
  mountpoint: string | null;
  seq: number;
  lastActivity: number;
}

type Handler<E extends EventName> = (payload: EventMap[E]) => void;

const MB = 1024 * 1024;
const NS = 1_000_000;
const DAY = 86_400_000;

function err(code: string, message: string): ApiError {
  return { code, message };
}

const ASSET_BY_EXT: Record<string, string> = {
  pdf: '/mock/document.pdf',
  webm: '/mock/clip.webm',
  mp4: '/mock/clip.webm',
  mov: '/mock/clip.webm',
  mkv: '/mock/clip.webm',
  ogg: '/mock/tone.ogg',
  m4a: '/mock/tone.ogg',
  mp3: '/mock/tone.ogg',
  wav: '/mock/tone.ogg',
  md: '/mock/notes.md',
  txt: '/mock/readme.txt',
  pub: '/mock/key.pub',
  png: '/mock/card.png',
  zip: '/mock/v/archivio.zip',
  gz: '/mock/v/backup-tar-gz.bin',
  docx: '/mock/v/verbale.docx',
  xlsx: '/mock/v/bilancio.xlsx',
  pptx: '/mock/v/presentazione.pptx',
  odt: '/mock/v/lettera.odt',
  jpg: '/mock/photo-01.jpg',
  jpeg: '/mock/photo-01.jpg',
};

function extOf(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

function basename(p: string): string {
  return p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? p;
}

function volumeTag(path: string): string {
  const A = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
  let h = 2166136261 >>> 0;
  let g = 0x9e3779b9 >>> 0;
  for (let i = 0; i < path.length; i++) {
    h = Math.imul(h ^ path.charCodeAt(i), 16777619) >>> 0;
    g = Math.imul(g ^ path.charCodeAt(i), 2246822519) >>> 0;
  }
  let out = '';
  for (let i = 0; i < 9; i++) out += A[((i < 6 ? h >>> (i * 5) : g >>> ((i - 6) * 5)) & 31) >>> 0];
  return out;
}

function stem(p: string): string {
  return basename(p).replace(/\.nvault$/i, '');
}

function validName(name: string): boolean {
  return (
    name.length > 0 &&
    new TextEncoder().encode(name).length <= 255 &&
    name !== '.' &&
    name !== '..' &&
    !name.includes('/') &&
    !name.includes('\0')
  );
}

type Seed = [name: string, sizeOrChildren: number | Seed[], asset?: string];

const photo = (n: number) => `/mock/photo-${String(n).padStart(2, '0')}.jpg`;

const dirs = (names: string[]): Seed[] => names.map((n) => [n, [['dentro.bin', 1_000]]]);
const GAME_FILES: Seed[] = [
  ['sh2pc.exe', 6.2 * MB],
  ['binkw32.dll', 180_000],
  ['d3d8.dll', 1.1 * MB],
  ['local.fsb', 92 * MB],
  ['save.dat', 42_000],
  ['config.ini', 1_400],
  ['Leggimi.txt', 3_000],
];
const BUSTE: Seed[] = [
  ['Solo file', [...GAME_FILES, ['bgm.afs', 310 * MB], ['voice.afs', 120 * MB], ['movie.bik', 64 * MB], ['font.tex', 900_000], ['core.pak', 12 * MB]]],
  ['Solo cartelle', dirs(['bg', 'chr', 'etc', 'menu', 'music', 'pic', 'sound', 'movie', 'eff', 'item', 'map', 'demo'])],
  ['Misto', [...GAME_FILES.slice(0, 4), ...dirs(['data', 'sh2e', 'saves', 'mods', 'tools', 'docs', 'cache', 'logs', 'backup'])]],
  ['Un file', [['note.bin', 4_000]]],
  ['Due file', [['a.bin', 4_000], ['b.bin', 8_000]]],
  ['Una cartella', dirs(['dentro'])],
  ['Due cartelle', dirs(['prima', 'seconda'])],
  ['Un file e cartelle', [['solo.dat', 9_000], ...dirs(['uno', 'due', 'tre'])]],
  ['Foto e cartelle', [['Ritratto.jpg', 1.2 * MB, photo(11)], ...dirs(['uno', 'due'])]],
  [
    'Molti elementi',
    [
      ...Array.from({ length: 60 }, (_, i): Seed => [`file ${String(i + 1).padStart(3, '0')}.dat`, 1_000 + i * 97]),
      ...dirs(Array.from({ length: 20 }, (_, i) => `cartella ${i + 1}`)),
    ],
  ],
  [
    'Una cartella dal nome lunghissimo che non sta in nessuna riga della griglia e va troncata',
    [['un file dal nome altrettanto lungo, giusto per vedere cosa succede alla busta.bin', 2_000], ...dirs(['sottocartella dal nome molto lungo anche lei'])],
  ],
  ['Vuota', []],
];

const DOCUMENTI: Seed[] = [
  [
    'Foto',
    [
      [
        'Vacanze 2024',
        [
          ['Dolomiti 01.jpg', 4.2 * MB, photo(1)],
          ['Dolomiti 02.jpg', 3.9 * MB, photo(2)],
          ['Dolomiti 03.jpg', 4.6 * MB, photo(3)],
          ['Lago di Braies.jpg', 5.1 * MB, photo(4)],
          ['Rifugio.jpg', 3.3 * MB, photo(5)],
        ],
      ],
      [
        'Compleanno Giulia',
        [
          ['IMG_2041.jpg', 2.8 * MB, photo(6)],
          ['IMG_2042.jpg', 2.9 * MB, photo(7)],
          ['IMG_2043.jpg', 3.1 * MB, photo(8)],
          ['IMG_2044.jpg', 2.7 * MB, photo(9)],
        ],
      ],
      ['Tramonto sul lago.jpg', 3.6 * MB, photo(10)],
      ['Nonna e nonno.jpg', 1.9 * MB, photo(11)],
      ['Micio.jpg', 2.2 * MB, photo(12)],
    ],
  ],
  [
    'Lavoro',
    [
      ['Archivio 2023', []],
      ['Contratto affitto.pdf', 412_000],
      ['Fattura 2024-031.pdf', 88_000],
      ['Preventivo cucina.pdf', 1.4 * MB],
      ['Budget 2025.xlsx', 61_000],
      ['Note riunione.md', 1_200],
      ['Presentazione Q3.pptx', 7.8 * MB],
    ],
  ],
  [
    'Personale',
    [
      [
        'Ricette',
        [
          ['Ragù della nonna.txt', 420, '/mock/recipe.txt'],
          ['Tiramisù.txt', 380, '/mock/recipe.txt'],
          ['Pane in cassetta.txt', 510, '/mock/recipe.txt'],
        ],
      ],
      ['Buste', BUSTE],
      [
        'Anteprime',
        [
          ['main.rs', 610, '/mock/v/main.rs'],
          ['budget.ts', 480, '/mock/v/budget.ts'],
          ['backup.py', 533, '/mock/v/backup.py'],
          ['impostazioni.json', 152, '/mock/v/impostazioni.json'],
          ['stile.css', 232, '/mock/v/stile.css'],
          ['config.toml', 139, '/mock/v/config.toml'],
          ['Lettera (Windows-1252).txt', 164, '/mock/v/lettera-1252.txt'],
          ['Lettera (UTF-16).txt', 330, '/mock/v/lettera-utf16.txt'],
          ['Spese maggio.csv', 230, '/mock/v/spese.csv'],
          ['Guida al volume.md', 953, '/mock/v/guida.md'],
          ['Taccuino di casa.md', 2_483, '/mock/v/taccuino.md'],
          ['Nota sospetta.md', 1_126, '/mock/v/sospetto.md'],
          ['Vecchio PC.zip', 845, '/mock/v/archivio.zip'],
          ['Backup contatti.tar', 10_240, '/mock/v/backup.tar'],
          ['Backup contatti.tar.gz', 237, '/mock/v/backup-tar-gz.bin'],
          ['Verbale riunione.docx', 1_775, '/mock/v/verbale.docx'],
          ['Bilancio casa.xlsx', 2_002, '/mock/v/bilancio.xlsx'],
          ['Risultati Q3.pptx', 2_222, '/mock/v/presentazione.pptx'],
          ['Lettera condominio.odt', 911, '/mock/v/lettera.odt'],
          ['dati.bin', 3_000, '/mock/v/dati.bin'],
          ['Scatto.heic', 1_048, '/mock/v/scatto.heic'],
          ['Foto ruotata.jpg', 30_725, '/mock/v/ruotata.jpg'],
          ['Lago.webp', 41_928, '/mock/v/lago.webp'],
          ['server.log', MOCK_LOG_SIZE, 'gen:log'],
          ['Panorama enorme.tif', 260 * MB, '/mock/photo-02.jpg'],
          ['LEGGIMI', 152, '/mock/v/impostazioni.json'],
        ],
      ],
      ['Passaporto (scansione).jpg', 1.1 * MB, '/mock/scan.jpg'],
      ['Tessera sanitaria.png', 640_000, '/mock/card.png'],
      ['Codice fiscale.png', 590_000, '/mock/card.png'],
      ['Analisi sangue 2024.pdf', 230_000],
    ],
  ],
  [
    'Video',
    [
      ['Compleanno Giulia.webm', 148 * MB],
      ['Tramonto timelapse.mp4', 86 * MB],
      ['Drone vacanze.mov', 412 * MB],
    ],
  ],
  [
    'Audio',
    [
      ['Nota vocale 01.ogg', 410_000],
      ['Nota vocale 02.ogg', 1.2 * MB],
      ['Messaggio papà.m4a', 2.4 * MB],
    ],
  ],
  [
    'Chiavi',
    [
      ['id_ed25519.pub', 98],
      ['id_ed25519', 411, '/mock/key.pub'],
      ['Password wifi.txt', 64, '/mock/readme.txt'],
    ],
  ],
  ['Leggimi.txt', 2_100],
  ['Archivio 2019.zip', 312 * MB],
  ['Backup telefono.tar.gz', 1.7 * 1024 * MB],
  ['Foto profilo.jpg', 840_000, photo(13)],
  ['Copertina.jpg', 1.3 * MB, photo(14)],
];

const ARCHIVIO: Seed[] = [
  [
    'Ricevute',
    [
      ['Farmacia marzo.pdf', 120_000],
      ['Dentista.pdf', 98_000],
      ['Ottico.pdf', 77_000],
    ],
  ],
  ['730 2023.pdf', 940_000],
  ['730 2024.pdf', 1.02 * MB],
  ['CU 2024.pdf', 310_000],
];

const DIARIO: Seed[] = [
  ['2024', [['Gennaio.md', 4_200, '/mock/notes.md'], ['Febbraio.md', 3_100, '/mock/notes.md']]],
  ['Lettere', [['A Giulia.txt', 1_900, '/mock/readme.txt']]],
  ['Pensieri.md', 2_600, '/mock/notes.md'],
];

function defaultSlots(single: boolean): SlotInfo[] {
  const kdf = { m_kib: 524_288, t: 3, p: 4 };
  const slots: SlotInfo[] = [{ index: 0, kind: 'password', label: switches.demo ? DEMO_SLOTS.main : 'Principale', kdf, in_use: false }];
  if (!single) slots.push({ index: 1, kind: 'password', label: switches.demo ? DEMO_SLOTS.spare : 'Busta in cassaforte', kdf, in_use: false });
  return slots;
}

const GB = 1024 * 1024 * 1024;

export const MOCK_DRIVES: Drive[] = [
  { id: '/dev/sdb1', label: switches.demo ? DEMO_STICK_LABEL : 'CHIAVETTA', mount_path: STICK, folder: STICK, fs_type: 'exfat', total_bytes: 32 * GB, free_bytes: 21.7 * GB, removable: true, system: false, app_here: true, is_nicevault_device: false },
  { id: '/dev/nvme0n1p2', label: '', mount_path: '/', folder: switches.demo ? DEMO_HOME : '/home/utente/Documenti', fs_type: 'ext4', total_bytes: 512 * GB, free_bytes: 188 * GB, removable: false, system: true, app_here: false, is_nicevault_device: false },
];

export const MOCK_BACKUP_DRIVE: Drive = {
  id: '/dev/sdc1', label: 'BACKUP', mount_path: '/run/media/utente/BACKUP', folder: '/run/media/utente/BACKUP', fs_type: 'ntfs', total_bytes: 1000 * GB, free_bytes: 612 * GB, removable: true, system: false, app_here: false, is_nicevault_device: false,
};

const FOTOGRAFIE: Seed[] = [
  ['Mare 2025', [['Spiaggia.jpg', 2_400_000, '/mock/photo-01.jpg'], ['Tramonto.jpg', 3_100_000, '/mock/photo-02.jpg']]],
  ['Nonna.png', 860_000, '/mock/card.png'],
];

function mockPathSize(path: string): number {
  const name = basename(path);
  if (/^huge/i.test(name)) return 6 * 1024 ** 3;
  if (/slow/i.test(name)) return 80_000_000;
  return 200_000 + ((name.length * 7919) % 40) * 310_000;
}

export class MockBackend {
  readonly opts: MockOptions;
  private volumes = new Map<string, MockVolume>();
  private open = new Map<VaultId, OpenVault>();
  private recents: RecentEntry[] = [];
  private drives: Drive[] = [...MOCK_DRIVES.map((d) => ({ ...d }))];
  private defaults: Settings = { ...DEFAULT_SETTINGS };
  private listeners = new Map<EventName, Set<Handler<EventName>>>();
  private jobSeq = 0;
  private vaultSeq = 0;
  private cancelled = new Set<JobId>();
  private seededPaths = new Set<string>();
  private update = { checkOnStart: true, lastCheck: null as number | null, found: false, downloaded: false, installed: false };
  readonly pm = new PmMock({
    pathOf: (vault) => this.vault(vault).ov.path,
    seeded: (path) => this.seededPaths.has(path),
    wait: (ms) => this.wait(ms),
    emit: (event, payload) => this.emit(event, payload),
    speed: () => this.opts.speed,
    extra: () => this.opts.pmEntries,
    siteIcons: () => this.siteIcons,
  });

  readonly search = new SearchMock({
    volume: (vault) => {
      const { vol } = this.vault(vault);
      return { path: vol.path, nodes: vol.nodes };
    },
    seeded: (path) => this.seededPaths.has(path),
    wait: (ms) => this.wait(ms),
    emit: (event, payload) => this.emit(event, payload),
  });
  device!: DeviceMock;
  stick!: StickMock;
  private fmtFull = new Set<string>();
  resize!: ResizeMock;
  viewers!: ViewersMock;
  browserLink!: BrowserLinkMock;
  lastCreate: Record<string, unknown> | null = null;

  constructor(opts: Partial<MockOptions> = {}) {
    this.opts = { ...DEFAULTS, ...opts };
    this.device = new DeviceMock(
      { wait: (ms) => this.wait(ms), emit: (event, payload) => this.emit(event, payload), speed: () => this.opts.speed },
      { scenario: this.opts.device, fail: this.opts.deviceFail, offline: this.opts.deviceOffline, slow: this.opts.deviceSlow, volumes: this.opts.deviceVolumes },
    );
    this.stick = new StickMock(
      { wait: (ms) => this.wait(ms), emit: (event, payload) => this.emit(event, payload), speed: () => this.opts.speed },
      { scenario: this.opts.stick, offline: this.opts.stickOffline, fail: this.opts.stickFail, dataMount: STICK, running: '0.1.0', latest: MOCK_UPDATE.version },
    );
    this.resize = new ResizeMock(
      {
        volume: (vault) => {
          const { vol } = this.vault(vault);
          return { path: vol.path, used: this.used(vol) };
        },
        wait: (ms) => this.wait(ms),
        emit: (event, payload) => this.emit(event, payload),
      },
      { fixed: this.opts.fixed, fail: this.opts.resizeFail },
    );
    this.browserLink = new BrowserLinkMock(
      {
        emit: (event, payload) => this.emit(event, payload),
        installed: () => this.installed,
        volumes: () => this.recentEntries().map((r) => ({ path: r.path, name: r.name })),
      },
      typeof location === 'undefined' ? null : new URLSearchParams(location.search).get('browser'),
    );
    this.viewers = new ViewersMock({
      file: (vault, node) => {
        const { ov, vol } = this.vault(vault);
        const n = this.node(vol, node);
        return { name: n.name, kind: n.kind, path: this.pathOf(vol, node), mountpoint: ov.mountpoint };
      },
      wait: (ms) => this.wait(ms),
    });
    const now = Date.now();
    const D = switches.demo;
    const docName = D ? DEMO_NAMES.documents : 'Documenti';
    const arcName = D ? DEMO_NAMES.tax : 'Archivio fiscale';
    const diaName = D ? DEMO_NAMES.journal : 'Diario';
    const doc = this.addVolume(`${STICK}/${docName}.nvault`, this.opts.empty ? [] : D ? DEMO_DOCUMENTS : DOCUMENTI, now - 2 * DAY);
    const arc = this.addVolume(`${STICK}/${arcName}.nvault`, this.opts.empty ? [] : D ? DEMO_TAX : ARCHIVIO, now - 40 * DAY);
    const dia = this.addVolume(`${STICK}/${diaName}.nvault`, this.opts.empty ? [] : D ? DEMO_JOURNAL : DIARIO, now - 20 * DAY);
    const forced = this.opts.emblem;
    const seed = (vol: MockVolume, own: Emblem, icon: string, accent: string, visibility: Visibility, frame = 'guilloche') => {
      const emblem: Emblem =
        forced === 'initial' ? { kind: 'initial' } : forced === 'icon' ? { kind: 'icon', id: icon } : forced === 'image' ? { kind: 'image', data_base64: MOCK_EMBLEM_WEBP } : own;
      const vis = forced === 'hidden' ? 'encrypted' : (this.opts.appearance ?? visibility);
      vol.emblem = { emblem, show_when_locked: vis === 'visible', emblem_frame: this.opts.frame ?? frame };
      vol.settings = applySettings(applySettings(DEFAULT_SETTINGS, { accent }), loadStoredSettings()[vol.path]);
    };
    seed(doc, { kind: 'image', data_base64: MOCK_EMBLEM_WEBP }, 'house', 'velluto', 'visible');
    seed(arc, { kind: 'icon', id: 'receipt' }, 'receipt', 'prussia', 'visible', 'starburst');
    seed(dia, { kind: 'icon', id: 'book-open' }, 'book-open', 'bottiglia', 'encrypted');
    if (!this.opts.empty) for (const v of [doc, arc, dia]) this.seededPaths.add(v.path);
    for (const v of [doc, arc, dia]) this.resize.seed(v.path);
    if (this.opts.fixed && this.opts.fmtFull) for (const v of [doc, arc, dia]) this.fmtFull.add(v.path);
    if (!this.opts.singleSlot) {
      doc.slots.push({ index: 2, kind: 'duress', label: switches.demo ? DEMO_SLOTS.duress : 'Autodistruzione', kdf: { m_kib: 524_288, t: 3, p: 4 }, in_use: false });
      doc.duress.set(2, MOCK_DURESS_PASSWORD);
    }
    if (!this.opts.empty) {
      const oldShown = forced !== 'hidden' && this.opts.appearance !== 'encrypted';
      this.recents = [
        { path: doc.path, name: docName, size_bytes: 2.61 * 1024 * MB, last_opened: now - 2 * 3600_000, exists: true, unlock_ms: this.opts.unlockMs, source: 'here' },
        { path: arc.path, name: arcName, size_bytes: 3.4 * MB, last_opened: now - 9 * DAY, exists: true, unlock_ms: this.opts.unlockMs, source: 'here' },
        { path: dia.path, name: diaName, size_bytes: 18.2 * MB, last_opened: now - 20 * DAY, exists: true, unlock_ms: this.opts.unlockMs, source: 'removable' },
        {
          path: D ? '/run/media/alex/OLD LAPTOP/Old laptop.nvault' : '/media/utente/VECCHIA/Vecchio.nvault',
          name: D ? DEMO_NAMES.old : 'Vecchio',
          size_bytes: 512 * MB,
          last_opened: now - 120 * DAY,
          exists: false,
          emblem: !oldShown ? null : forced === 'icon' ? { kind: 'icon', id: 'key-round' } : forced === 'image' ? { kind: 'image', data_base64: MOCK_EMBLEM_WEBP } : { kind: 'initial' },
          show_when_locked: oldShown,
          accent: oldShown ? 'seppia' : null,
          source: 'known',
        },
      ];
      if (switches.recents === 'long') {
        const long = (name: string, path: string, days: number, source: RecentEntry['source'], size: number): RecentEntry => ({
          path, name, size_bytes: size, last_opened: now - days * DAY, exists: true, unlock_ms: this.opts.unlockMs, source,
        });
        this.recents.push(
          long('Dichiarazioni dei redditi e ricevute mediche 2019–2026', '/media/utente/CHIAVETTA_LAVORO_ARCHIVIO_STORICO/Dichiarazioni.nvault', 3, 'removable', 118.4 * 1024 * MB),
          long('Fotografie di famiglia — matrimonio di Giulia e Alessandro', '/home/utente/Documenti/Volumi cifrati personali e di famiglia/Foto.nvault', 31, 'known', 1.2 * 1024 * 1024 * MB),
          long('Wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww', '/home/utente/WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW/W.nvault', 400, 'session', 999.9 * MB),
        );
      }
    }
  }

  private look(vol: MockVolume): Pick<RecentEntry, 'emblem' | 'show_when_locked' | 'accent' | 'emblem_frame' | 'volume_tag' | 'key_slots' | 'kdf'> {
    const head = { volume_tag: volumeTag(vol.path), key_slots: vol.slots.length, kdf: vol.slots.find((s) => s.kind === 'password')?.kdf ?? null };
    if (!vol.emblem.show_when_locked) return { emblem: null, show_when_locked: false, accent: null, emblem_frame: null, ...head };
    const e = vol.emblem.emblem;
    const emblem: Emblem = e.kind === 'image' ? { kind: 'image', data_base64: vol.lockedImage ?? e.data_base64 } : structuredClone(e);
    const frame = vol.emblem.emblem_frame;
    return { emblem, show_when_locked: true, accent: vol.settings.accent, emblem_frame: frame && frame !== 'guilloche' ? frame : null, ...head };
  }

  private appearance(vol: MockVolume): AppearanceState {
    return {
      accent: vol.settings.accent,
      emblem: structuredClone(vol.emblem.emblem),
      emblem_frame: vol.emblem.emblem_frame ?? 'guilloche',
      visibility: vol.emblem.show_when_locked ? 'visible' : 'encrypted',
      tampered: false,
    };
  }

  private settingsTarget(vault: unknown): MockVolume | null {
    if (vault !== undefined && vault !== null) return this.vault(String(vault)).vol;
    const last = [...this.open.values()].sort((a, b) => b.seq - a.seq)[0];
    return last ? this.volumes.get(last.path)! : null;
  }

  private setSettings(vol: MockVolume, s: Settings) {
    vol.settings = s;
    storeSettings(vol.path, s);
  }

  private wait(ms: number): Promise<void> {
    const t = ms * this.opts.speed;
    return t > 0 ? new Promise((r) => setTimeout(r, t)) : Promise.resolve();
  }

  listen<E extends EventName>(event: E, handler: Handler<E>): () => void {
    let set = this.listeners.get(event);
    if (!set) this.listeners.set(event, (set = new Set()));
    set.add(handler as Handler<EventName>);
    return () => set.delete(handler as Handler<EventName>);
  }

  emit<E extends EventName>(event: E, payload: EventMap[E]): void {
    for (const h of this.listeners.get(event) ?? []) (h as Handler<E>)(payload);
  }

  private addVolume(path: string, seed: Seed[], mtimeMs: number): MockVolume {
    const vol: MockVolume = {
      path,
      name: stem(path),
      password: MOCK_PASSWORD,
      nodes: new Map(),
      nextId: 2,
      slots: defaultSlots(this.opts.singleSlot),
      emblem: { emblem: { kind: 'initial' }, show_when_locked: true, emblem_frame: 'guilloche' },
      settings: { ...this.defaults },
      lockedImage: null,
      duress: new Map(),
      destroyed: false,
    };
    vol.nodes.set(ROOT, { id: ROOT, parent: ROOT, name: '', kind: 'dir', size: 0, mtime: mtimeMs * NS, created: (mtimeMs - 90 * DAY) * NS });
    let k = 0;
    const hostPath = (id: NodeId): string => {
      const n = vol.nodes.get(id);
      return !n || id === ROOT ? '' : `${hostPath(n.parent)}/${n.name}`;
    };
    const walk = (parent: NodeId, items: Seed[]) => {
      for (const [name, v, asset] of items) {
        const id = vol.nextId++;
        const mtime = (mtimeMs - (k++ % 37) * 3.7 * DAY) * NS;
        const origin = `${switches.demo ? DEMO_HOME : '/home/utente/Documenti'}${hostPath(parent)}/${name}`;
        const meta = { created: mtime - 30 * DAY * NS, imported: mtime + 2 * DAY * NS, origin };
        if (Array.isArray(v)) {
          vol.nodes.set(id, { id, parent, name, kind: 'dir', size: 0, mtime, ...meta });
          walk(id, v);
        } else {
          vol.nodes.set(id, {
            id,
            parent,
            name,
            kind: 'file',
            size: Math.round(v),
            mtime,
            asset: asset ?? ASSET_BY_EXT[extOf(name)],
            ...meta,
          });
        }
      }
    };
    walk(ROOT, seed);
    this.volumes.set(path, vol);
    return vol;
  }

  private vault(id: VaultId): { ov: OpenVault; vol: MockVolume } {
    const ov = this.open.get(id);
    if (!ov) throw err('locked', 'The volume is locked.');
    ov.lastActivity = Date.now();
    return { ov, vol: this.volumes.get(ov.path)! };
  }

  private node(vol: MockVolume, id: NodeId): MockNode {
    const n = vol.nodes.get(id);
    if (!n) throw err('not_found', 'Item not found.');
    return n;
  }

  private children(vol: MockVolume, dir: NodeId): MockNode[] {
    return [...vol.nodes.values()].filter((n) => n.parent === dir && n.id !== ROOT);
  }

  private info(ov: OpenVault): VaultInfo {
    const vol = this.volumes.get(ov.path)!;
    const e = vol.emblem;
    return {
      id: ov.id,
      path: ov.path,
      name: stem(ov.path),
      mountpoint: ov.mountpoint,
      kdf: { m_kib: 524_288, t: 3, p: 4 },
      emblem: structuredClone(e.emblem),
      emblem_frame: e.emblem_frame ?? 'guilloche',
      show_when_locked: e.show_when_locked,
      visibility: e.show_when_locked ? 'visible' : 'encrypted',
      settings: { ...vol.settings },
      appearance_tampered: false,
      volume_tag: volumeTag(ov.path),
      single_file: !!vol.singleFile,
      can_upgrade_format: !vol.singleFile && !this.fmtFull.has(ov.path),
      ...(vol.rolledBack ? { rolled_back: { ...vol.rolledBack } } : {}),
    };
  }

  private validLockedImage(b64: unknown): string | null {
    if (b64 === undefined || b64 === null || b64 === '') return null;
    const img = this.validEmblem({ kind: 'image', data_base64: b64 });
    if (img.kind !== 'image' || atob(img.data_base64).length > LOCKED_IMAGE_BYTES) throw err('invalid_emblem', 'Invalid thumbnail.');
    return img.data_base64;
  }

  private validEmblem(e: unknown): Emblem {
    const bad = (m: string) => err('invalid_emblem', m);
    const x = e as Record<string, unknown> | null;
    if (x?.kind === 'initial') return { kind: 'initial' };
    if (x?.kind === 'icon') {
      const id = String(x.id ?? '');
      if (!/^[a-z0-9-]{1,40}$/.test(id)) throw bad('Icona non valida.');
      return { kind: 'icon', id };
    }
    if (x?.kind === 'image') {
      let b64 = String(x.data_base64 ?? '');
      if (b64.startsWith('data:')) b64 = b64.slice(b64.indexOf(',') + 1);
      let bin: string;
      try {
        bin = atob(b64.trim());
      } catch {
        throw bad('Immagine non leggibile.');
      }
      if (bin.length > MAX_IMAGE_BYTES) throw bad('Immagine troppo grande (massimo 64 KB).');
      const png = bin.startsWith('\x89PNG\r\n\x1a\n');
      const webp = bin.length > 12 && bin.startsWith('RIFF') && bin.slice(8, 12) === 'WEBP';
      if (!png && !webp) throw bad('L’immagine deve essere PNG o WebP.');
      return { kind: 'image', data_base64: btoa(bin) };
    }
    throw bad('Emblema non valido.');
  }

  private touchRecent(path: string) {
    const vol = this.volumes.get(path);
    this.recents = this.recents.filter((r) => r.path !== path);
    this.recents.unshift({
      path,
      name: stem(path),
      size_bytes: vol ? Math.max(1.2 * MB, this.used(vol) * 1.02) : 0,
      last_opened: Date.now(),
      exists: true,
      unlock_ms: this.opts.unlockMs,
      source: 'session',
    });
  }

  private recentEntries(): RecentEntry[] {
    return this.recents.map((r) => {
      const vol = this.volumes.get(r.path);
      return vol ? { ...r, ...this.look(vol) } : { ...r };
    });
  }

  private setAppearance(vol: MockVolume, p: { accent?: unknown; emblem?: unknown; visibility?: unknown; locked_image_base64?: unknown; emblem_frame?: unknown }) {
    const emblem = p.emblem !== undefined && p.emblem !== null ? this.validEmblem(p.emblem) : undefined;
    if (p.emblem_frame !== undefined && p.emblem_frame !== null) {
      if (typeof p.emblem_frame !== 'string' || !/^[a-z0-9-]{1,32}$/.test(p.emblem_frame)) throw err('invalid_emblem', 'Invalid frame.');
      vol.emblem = { ...vol.emblem, emblem_frame: p.emblem_frame };
    }
    const thumb = this.validLockedImage(p.locked_image_base64);
    if (p.visibility !== undefined && p.visibility !== null && p.visibility !== 'visible' && p.visibility !== 'encrypted') {
      throw err('invalid_appearance', 'Invalid visibility.');
    }
    if (typeof p.accent === 'string') this.setSettings(vol, applySettings(vol.settings, { accent: p.accent }));
    if (emblem) {
      if (emblem.kind !== 'image') vol.lockedImage = null;
      else if (thumb) vol.lockedImage = thumb;
      else if (JSON.stringify(emblem) !== JSON.stringify(vol.emblem.emblem)) vol.lockedImage = null;
      vol.emblem = { ...vol.emblem, emblem };
    }
    if (p.visibility === 'visible' || p.visibility === 'encrypted') vol.emblem = { ...vol.emblem, show_when_locked: p.visibility === 'visible' };
  }

  private used(vol: MockVolume): number {
    let s = 0;
    for (const n of vol.nodes.values()) s += n.size;
    return s;
  }

  private uniqueName(vol: MockVolume, parent: NodeId, name: string): string {
    const taken = new Set(this.children(vol, parent).map((n) => n.name));
    if (!taken.has(name)) return name;
    const i = name.lastIndexOf('.');
    const [base, ext] = i > 0 ? [name.slice(0, i), name.slice(i)] : [name, ''];
    for (let k = 2; ; k++) {
      const c = `${base} (${k})${ext}`;
      if (!taken.has(c)) return c;
    }
  }

  simulateRemoval(reason: LockReason = 'removed'): void {
    for (const id of [...this.open.keys()]) {
      this.open.delete(id);
      this.emit('vault-locked', { vault: id, reason });
    }
  }

  private pathOf(vol: MockVolume, id: NodeId): string {
    const parts: string[] = [];
    for (let cur = id; cur !== ROOT; ) {
      const n = vol.nodes.get(cur);
      if (!n) break;
      parts.unshift(n.name);
      cur = n.parent;
    }
    return `/${parts.join('/')}`;
  }

  private props(vol: MockVolume, n: MockNode): NodeProps {
    const info = n.kind === 'file' ? (n.asset ? ASSET_INFO[n.asset] : undefined) : undefined;
    return {
      id: n.id,
      name: n.name,
      kind: n.kind,
      path: this.pathOf(vol, n.id),
      size: n.size,
      mtime: n.mtime,
      created: n.created ?? null,
      imported: n.imported ?? null,
      origin: n.origin ?? null,
      mime: n.kind === 'file' ? (info?.mime ?? null) : null,
      width: info?.width ?? null,
      height: info?.height ?? null,
      duration_ms: info?.duration_ms ?? null,
      sha256: n.sha256 ?? null,
      children: n.kind === 'dir' ? this.children(vol, n.id).length : null,
    };
  }

  private propsJob(vault: VaultId, steps: number, step: (i: number) => Record<string, unknown>, done: () => Record<string, unknown>): JobId {
    const job = `props-${++this.jobSeq}`;
    let i = 0;
    const tick = () => {
      if (this.cancelled.has(job)) return;
      if (!this.open.has(vault)) {
        this.emit('props-done', { job, error: err('locked', 'The volume was locked.') });
        return;
      }
      if (i < steps) {
        this.emit('props-progress', { job, ...step(i++) });
        setTimeout(tick, 60 * this.opts.speed);
      } else this.emit('props-done', { job, error: null, ...done() });
    };
    setTimeout(tick, 0);
    return job;
  }

  private propsTree(vault: VaultId, vol: MockVolume, ids: NodeId[]): JobId {
    let files = 0;
    let dirs = 0;
    let bytes = 0;
    const walk = (id: NodeId) => {
      for (const c of this.children(vol, id)) {
        if (c.kind === 'dir') {
          dirs++;
          walk(c.id);
        } else {
          files++;
          bytes += c.size;
        }
      }
    };
    for (const id of ids) {
      const n = this.node(vol, id);
      if (n.kind === 'dir') walk(id);
      else {
        files++;
        bytes += n.size;
      }
    }
    const total = { files, dirs, bytes };
    const steps = 4;
    const part = (i: number) => ({ files: Math.round((files * (i + 1)) / (steps + 1)), dirs: Math.round((dirs * (i + 1)) / (steps + 1)), bytes: Math.round((bytes * (i + 1)) / (steps + 1)) });
    return this.propsJob(vault, steps, part, () => total);
  }

  private hashJob(vault: VaultId, n: MockNode): JobId {
    const steps = 8;
    return this.propsJob(
      vault,
      steps,
      (i) => ({ done_bytes: Math.round((n.size * (i + 1)) / steps), total_bytes: n.size }),
      () => {
        n.sha256 = fakeSha(`${n.name}:${n.size}`);
        return { sha256: n.sha256, done_bytes: n.size, total_bytes: n.size };
      },
    );
  }

  simulateDrive(action: 'add' | 'remove', drive?: Partial<Drive> | string): Drive | null {
    if (action === 'add') {
      const d: Drive = { ...MOCK_BACKUP_DRIVE, ...(typeof drive === 'object' ? drive : {}) };
      if (!d.folder) d.folder = d.mount_path;
      this.drives = this.drives.filter((x) => x.id !== d.id);
      this.drives.push(d);
      if (d.id === MOCK_BACKUP_DRIVE.id) {
        const path = `${d.mount_path}/Fotografie.nvault`;
        const vol = this.volumes.get(path) ?? this.addVolume(path, this.opts.empty ? [] : FOTOGRAFIE, Date.now() - 5 * DAY);
        vol.emblem = { emblem: { kind: 'icon', id: 'camera' }, show_when_locked: true };
        vol.settings = applySettings(vol.settings, { accent: 'ottanio' });
        this.recents = this.recents.filter((r) => r.path !== path);
        this.recents.push({ path, name: 'Fotografie', size_bytes: 1.8 * 1024 * MB, last_opened: Date.now() - 5 * DAY, exists: true, unlock_ms: this.opts.unlockMs, source: 'removable' });
      }
      this.emit('drives-changed', { drives: this.drives.map((x) => ({ ...x })) });
      return d;
    }
    const id = typeof drive === 'string' ? drive : (drive?.id ?? [...this.drives].reverse().find((x) => x.removable)?.id);
    const d = this.drives.find((x) => x.id === id);
    if (!d) return null;
    this.drives = this.drives.filter((x) => x !== d);
    const on = (p: string) => p === d.mount_path || p.startsWith(`${d.mount_path}/`);
    this.recents = this.recents.filter((r) => !(on(r.path) && r.source === 'removable')).map((r) => (on(r.path) ? { ...r, exists: false } : r));
    for (const [vid, ov] of [...this.open]) {
      if (!on(ov.path)) continue;
      this.open.delete(vid);
      this.emit('vault-locked', { vault: vid, reason: 'removed' });
    }
    this.emit('drives-changed', { drives: this.drives.map((x) => ({ ...x })) });
    return d;
  }

  autolockStatus(now = Date.now()): AutoLockStatus[] {
    return [...this.open.values()]
      .map((ov) => {
        const min = this.volumes.get(ov.path)!.settings.auto_lock_minutes;
        return { vault: ov.id, auto_lock_minutes: min, remaining_ms: min > 0 ? Math.max(0, min * 60_000 - (now - ov.lastActivity)) : null };
      })
      .sort((x, y) => x.vault.localeCompare(y.vault));
  }

  private autoLock(due: OpenVault[], reason: LockReason): VaultId[] {
    if (due.length === 0) return [];
    const all = due.some((ov) => this.volumes.get(ov.path)!.settings.lock_scope === 'all');
    const ids = (all ? [...this.open.keys()] : due.map((ov) => ov.id)).sort();
    for (const id of ids) {
      this.open.delete(id);
      this.emit('vault-locked', { vault: id, reason });
    }
    return ids;
  }

  idleTick(now = Date.now()): VaultId[] {
    const due = [...this.open.values()].filter((ov) => {
      const min = this.volumes.get(ov.path)!.settings.auto_lock_minutes;
      return min > 0 && now - ov.lastActivity >= min * 60_000;
    });
    return this.autoLock(due, 'idle');
  }

  simulateLockEvent(reason: 'sleep' | 'screen'): VaultId[] {
    const due = [...this.open.values()].filter((ov) => {
      const s = this.volumes.get(ov.path)!.settings;
      return reason === 'sleep' ? s.lock_on_sleep : s.lock_on_screen_lock;
    });
    return this.autoLock(due, reason);
  }

  private osDragHandlers = new Set<(e: OsDragEvent) => void>();

  onOsDrag(handler: (e: OsDragEvent) => void): () => void {
    this.osDragHandlers.add(handler);
    return () => this.osDragHandlers.delete(handler);
  }

  simulateOsDrag(e: OsDragEvent): void {
    for (const h of this.osDragHandlers) h(e);
  }

  private externalSeq = 0;

  simulateExternalChange(
    dir: NodeId = ROOT,
    opts: { vault?: VaultId; add?: string[]; remove?: string[]; emit?: boolean } = {},
  ): NodeId[] {
    const ov = opts.vault ? this.open.get(opts.vault) : [...this.open.values()].sort((a, b) => b.seq - a.seq)[0];
    if (!ov) throw err('locked', 'Il volume è bloccato.');
    const vol = this.volumes.get(ov.path)!;
    if (this.node(vol, dir).kind !== 'dir') throw err('not_dir', 'Non è una cartella.');
    const drop = (id: NodeId) => {
      for (const c of this.children(vol, id)) drop(c.id);
      vol.nodes.delete(id);
    };
    for (const name of opts.remove ?? []) {
      const c = this.children(vol, dir).find((n) => n.name === name);
      if (c) drop(c.id);
    }
    const added: NodeId[] = [];
    for (const name of opts.add ?? [`Copiato da fuori ${++this.externalSeq}.txt`]) {
      if (this.children(vol, dir).some((n) => n.name === name)) continue;
      const id = vol.nextId++;
      const kind = extOf(name) ? 'file' : 'dir';
      vol.nodes.set(id, { id, parent: dir, name, kind, size: kind === 'file' ? 1_200 : 0, mtime: Date.now() * NS, asset: kind === 'file' ? ASSET_BY_EXT[extOf(name)] : undefined });
      added.push(id);
    }
    if (opts.emit !== false) this.emit('fs-changed', { vault: ov.id, dirs: [dir] });
    return added;
  }

  setMountAvailable(available: boolean): void {
    this.opts.mountUnavailable = !available;
  }

  seedJobs(vault: VaultId): { job: JobId; kind: 'import' | 'export'; label: string }[] {
    const a = this.runJob(vault, 'Importazione', [
      ['DSC_0192.jpg', 6.1 * MB],
      ['DSC_0193.jpg', 5.8 * MB],
      ['Video matrimonio.mp4', 820 * MB],
    ], { durationMs: 90_000, startFraction: 0.38 });
    const b = this.runJob(vault, 'Esportazione', [['Lavoro', 9.8 * MB]], { durationMs: 60_000, startFraction: 0.72 });
    const c = this.runJob(vault, 'Importazione', [['Scansione.pdf', 1.2 * MB]], { durationMs: 0 });
    const d = this.runJob(vault, 'Importazione', [['Film.mkv', 4.3 * 1024 * MB]], { durationMs: 0, fail: err('io', 'No space left on the drive.') });
    return [
      { job: a, kind: 'import', label: '3 elementi' },
      { job: b, kind: 'export', label: 'Lavoro' },
      { job: c, kind: 'import', label: 'Scansione.pdf' },
      { job: d, kind: 'import', label: 'Film.mkv' },
    ];
  }

  assetUrl(vault: VaultId, node: NodeId): string | null {
    const ov = this.open.get(vault);
    if (!ov) return null;
    const a = this.volumes.get(ov.path)?.nodes.get(node)?.asset ?? null;
    return a?.startsWith('gen:') ? generatedAsset(a) : a;
  }

  private deleteJob(vault: VaultId, vol: MockVolume, ids: NodeId[]): JobId {
    const job = `job-${++this.jobSeq}`;
    const order: MockNode[] = [];
    const seen = new Set<NodeId>();
    const walk = (id: NodeId) => {
      if (seen.has(id) || !vol.nodes.has(id)) return;
      seen.add(id);
      for (const c of this.children(vol, id)) walk(c.id);
      order.push(vol.nodes.get(id)!);
    };
    for (const id of ids) walk(id);
    const total_items = order.length;
    const total_bytes = order.reduce((s, n) => s + (n.kind === 'file' ? n.size : 0), 0);
    const out = { deleted: 0, total: total_items, freed_bytes: 0, errors: [] as { node: NodeId; name: string; code: string; message: string }[] };
    const perBatch = (this.opts.slowDelete ?? 40) * this.opts.speed;
    const failAt = this.opts.deleteFails ? Math.max(1, Math.floor(total_items / 2)) : Infinity;
    let k = 0;
    const progress = (current: string) =>
      this.emit('job-progress', { job, done_bytes: out.freed_bytes, total_bytes, current, done_items: out.deleted, total_items });
    const step = () => {
      if (this.cancelled.has(job)) return;
      if (!this.open.has(vault)) {
        this.emit('job-done', { job, error: err('locked', 'The volume was locked.'), outcome: out });
        return;
      }
      if (k >= failAt) {
        this.emit('job-done', {
          job,
          error: err('no_room_to_delete', 'There is not enough room to finish deleting: extend the volume (fixed-size) or free space on the drive.'),
          outcome: out,
        });
        return;
      }
      const dirs = new Set<NodeId>();
      let items = 0;
      let bytes = 0;
      let current = '';
      while (k < order.length && k < failAt && items < 128 && bytes < 16 * MB) {
        const n = order[k++];
        if (vol.nodes.has(n.id)) {
          vol.nodes.delete(n.id);
          dirs.add(n.parent);
        }
        items++;
        bytes += n.kind === 'file' ? n.size : 0;
        current = n.name;
      }
      out.deleted += items;
      out.freed_bytes += bytes;
      if (dirs.size) this.emit('fs-changed', { vault, dirs: [...dirs] });
      progress(current);
      if (k >= order.length) this.emit('job-done', { job, error: null, outcome: out });
      else setTimeout(step, perBatch);
    };
    setTimeout(() => {
      progress('');
      setTimeout(step, perBatch);
    }, 0);
    return job;
  }

  private runJob(
    vault: VaultId,
    _what: string,
    items: [name: string, size: number][],
    o: { durationMs?: number; startFraction?: number; fail?: ApiError; onDone?: () => void },
  ): JobId {
    const job = `job-${++this.jobSeq}`;
    const total = Math.round(items.reduce((s, [, n]) => s + n, 0));
    const duration = (o.durationMs ?? Math.min(4000, 900 + total / (60 * MB) * 1000)) * this.opts.speed;
    const tick = 80;
    const steps = Math.max(1, Math.round(duration / tick));
    let step = Math.round((o.startFraction ?? 0) * steps);
    const current = (done: number) => {
      let acc = 0;
      for (const [name, size] of items) {
        acc += size;
        if (done < acc) return name;
      }
      return items[items.length - 1]?.[0] ?? '';
    };
    const finish = () => {
      if (o.fail) {
        this.emit('job-done', { job, error: o.fail });
        return;
      }
      if (this.open.has(vault)) o.onDone?.();
      this.emit('job-done', { job, error: this.open.has(vault) ? null : err('locked', 'The volume was locked.') });
    };
    const run = () => {
      if (this.cancelled.has(job)) return;
      if (!this.open.has(vault)) {
        this.emit('job-done', { job, error: err('locked', 'The volume was locked.') });
        return;
      }
      step++;
      const failAt = o.fail ? Math.floor(steps * 0.55) : Infinity;
      const done = Math.min(total, Math.round((total * Math.min(step, failAt)) / steps));
      this.emit('job-progress', { job, done_bytes: done, total_bytes: total, current: current(done) });
      if (step >= steps || step >= failAt) finish();
      else setTimeout(run, tick);
    };
    if (duration <= 0) {
      setTimeout(() => {
        if (!o.fail) this.emit('job-progress', { job, done_bytes: total, total_bytes: total, current: current(total) });
        finish();
      }, 0);
    } else {
      setTimeout(run, 0);
    }
    return job;
  }

  private pick: string[] = [];
  private pickTried = new Set<number>();

  private importPhotos(vault: VaultId, parent: number, retry: number[] | null): { job: JobId; total: number } | null {
    const q = typeof location === 'undefined' ? null : new URLSearchParams(location.search);
    const { vol } = this.vault(vault);
    if (!retry) {
      const n = Math.max(0, Number(q?.get('photos') ?? 3));
      const base = ['IMG_0412.HEIC', 'IMG_0413.MOV', 'IMG_0415.JPG'];
      this.pick = Array.from({ length: n }, (_, i) => base[i] ?? `IMG_${String(1000 + i).padStart(4, '0')}.HEIC`);
      this.pickTried.clear();
      if (n === 0) return null;
    }
    const failFirst = Number(q?.get('pickfail') ?? 0);
    const items = retry ?? this.pick.map((_, i) => i);
    if (!items.length) return null;
    const job = `job-${++this.jobSeq}`;
    const total = items.length;
    let k = 0;
    let imported = 0;
    const failures: { index: number; name: string; reason: 'offline'; message: string }[] = [];
    const tick = 140 * this.opts.speed;
    const progress = (finished: boolean, cancelled = false) =>
      this.emit('mobile-pick-progress', {
        job, total, done: k, imported, failed: failures.length, fetching: finished ? 0 : Math.min(4, total - k),
        fraction: finished ? 1 : k / total, finished, cancelled, failures: finished ? failures : null,
      });
    const step = () => {
      if (!this.open.has(vault)) {
        this.emit('job-done', { job, error: err('locked', 'The volume was locked.') });
        return;
      }
      if (this.cancelled.has(job)) {
        for (const i of items.slice(k)) failures.push({ index: i, name: this.pick[i], reason: 'offline', message: 'Cancelled.' });
        failures.forEach((f) => ((f as { reason: string }).reason = f.message === 'Cancelled.' ? 'cancelled' : f.reason));
        progress(true, true);
        return;
      }
      if (k < total) {
        const i = items[k];
        const first = !this.pickTried.has(i);
        this.pickTried.add(i);
        if (first && i < failFirst) failures.push({ index: i, name: this.pick[i], reason: 'offline', message: 'The Internet connection appears to be offline.' });
        else {
          const name = this.pick[i];
          const id = vol.nextId++;
          const now = Date.now() * NS;
          vol.nodes.set(id, { id, parent, name: this.uniqueName(vol, parent, name), kind: 'file', size: 2_400_000, mtime: now, asset: ASSET_BY_EXT[extOf(name)], created: now, imported: now, origin: name });
          imported++;
        }
        k++;
        this.emit('job-progress', { job, done_bytes: imported * 2_400_000, total_bytes: total * 2_400_000, current: this.pick[i] });
        progress(false);
        setTimeout(step, tick);
        return;
      }
      progress(true);
      this.emit('job-done', { job, error: imported === 0 ? err('photos_failed', 'None of the photos could be imported.') : null });
    };
    setTimeout(step, 0);
    return { job, total };
  }

  language = 'system';
  siteIcons = true;
  sounds = true;
  videoArrows: 'seek' | 'step' = switches.video_arrows ?? 'seek';
  sidebarCollapsed = false;
  appTheme: ThemeId = switches.app_theme ?? 'dark';
  appAccent: string = switches.app_accent ?? DEFAULT_ACCENT;
  uiScale = switches.scale ?? 100;
  newLikeApp = true;
  installed = switches.installed;
  startAtLogin = false;
  startMinimized = false;
  closeToTray = true;
  gallerySave: boolean | null = null;
  gallerySaved: NodeId[] = [];

  private newVolumeSettings(): Settings {
    return this.newLikeApp ? { ...this.defaults, theme: this.appTheme, accent: this.appAccent } : { ...this.defaults };
  }

  private global(cmd: string, a: Record<string, unknown>): unknown {
    const view = () => ({
      language: this.language,
      defaults: { ...this.defaults },
      persisted: this.installed,
      app_theme: this.appTheme,
      app_accent: this.appAccent,
      ui_scale: this.uiScale,
      new_like_app: this.newLikeApp,
      site_icons: this.siteIcons,
      sounds: this.sounds,
      video_arrows: this.videoArrows,
      sidebar_collapsed: this.sidebarCollapsed,
      start_at_login: this.startAtLogin,
      start_minimized: this.startMinimized,
      close_to_tray: this.installed && this.closeToTray,
      gallery_save: this.gallerySave,
    });
    if (cmd === 'app_settings_get') return view();
    if (cmd === 'app_settings_set') {
      const p = (a.patch ?? {}) as {
        language?: string;
        defaults?: Partial<Settings>;
        app_theme?: string;
        app_accent?: string;
        ui_scale?: number;
        new_like_app?: boolean;
        site_icons?: boolean;
        sounds?: boolean;
        video_arrows?: string;
        sidebar_collapsed?: boolean;
        start_at_login?: boolean;
        start_minimized?: boolean;
        close_to_tray?: boolean;
        gallery_save?: boolean;
      };
      if (p.language && ['system', 'it', 'en'].includes(p.language)) this.language = p.language;
      if (typeof p.site_icons === 'boolean') this.siteIcons = p.site_icons;
      if (typeof p.sounds === 'boolean') this.sounds = p.sounds;
      if (p.video_arrows === 'seek' || p.video_arrows === 'step') this.videoArrows = p.video_arrows;
      if (typeof p.sidebar_collapsed === 'boolean') this.sidebarCollapsed = p.sidebar_collapsed;
      if (p.defaults) this.defaults = applySettings(this.defaults, p.defaults);
      if (p.app_theme && ['system', 'light', 'dark'].includes(p.app_theme)) this.appTheme = p.app_theme as ThemeId;
      if (isAccent(p.app_accent)) this.appAccent = p.app_accent;
      if (typeof p.ui_scale === 'number' && [90, 100, 110, 125, 150].includes(p.ui_scale)) this.uiScale = p.ui_scale;
      if (typeof p.new_like_app === 'boolean') this.newLikeApp = p.new_like_app;
      if (this.installed && typeof p.start_at_login === 'boolean') this.startAtLogin = p.start_at_login;
      if (this.installed && typeof p.start_minimized === 'boolean') this.startMinimized = p.start_minimized;
      if (this.installed && typeof p.close_to_tray === 'boolean') this.closeToTray = p.close_to_tray;
      if (typeof p.gallery_save === 'boolean') this.gallerySave = p.gallery_save;
      return view();
    }
    if (cmd === 'app_info')
      return { version: '0.1.0', portable: !this.installed, config_dir: this.installed ? '/home/utente/.config/NiceVault' : null };
    throw err('unknown_command', `Unknown command: ${cmd}`);
  }

  async invoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
    const a = args as Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    switch (cmd) {
      case 'launch_path_take':
        return null;
      case 'recent_list':
        await this.wait(30);
        return this.recentEntries();
      case 'recent_forget':
        this.recents = this.recents.filter((r) => r.path !== a.path);
        return this.recentEntries();
      case 'default_volume_dir':
        return STICK;
      case 'fs_props': {
        await this.wait(30);
        const { vol } = this.vault(String(a.vault));
        return this.props(vol, this.node(vol, Number(a.node)));
      }
      case 'fs_props_tree': {
        const vault = String(a.vault);
        const { vol } = this.vault(vault);
        const ids = (a.nodes as NodeId[]).map((id) => this.node(vol, id).id);
        return this.propsTree(vault, vol, ids);
      }
      case 'fs_hash': {
        const vault = String(a.vault);
        const { vol } = this.vault(vault);
        const n = this.node(vol, Number(a.node));
        if (n.kind !== 'file') throw err('is_dir', 'It is a folder.');
        return this.hashJob(vault, n);
      }
      case 'drives_list':
        await this.wait(20);
        return this.drives.map((d) => ({ ...d }));
      case 'settings_get': {
        const vol = this.settingsTarget(a.vault);
        return { ...(vol ? vol.settings : this.defaults) };
      }
      case 'settings_set': {
        const vol = this.settingsTarget(a.vault);
        if (!vol) return { ...(this.defaults = applySettings(this.defaults, a.settings)) };
        this.setSettings(vol, applySettings(vol.settings, a.settings));
        return { ...vol.settings };
      }

      case 'vault_emblem_get': {
        const { vol } = this.vault(String(a.vault));
        return structuredClone(vol.emblem);
      }
      case 'vault_emblem_set': {
        const { vol } = this.vault(String(a.vault));
        await this.wait(40);
        this.setAppearance(vol, { emblem: a.emblem, visibility: a.show_when_locked !== false ? 'visible' : 'encrypted', locked_image_base64: a.locked_image_base64, emblem_frame: a.emblem_frame });
        return structuredClone(vol.emblem);
      }
      case 'vault_appearance_get': {
        const { vol } = this.vault(String(a.vault));
        return this.appearance(vol);
      }
      case 'vault_appearance_set': {
        const { vol } = this.vault(String(a.vault));
        await this.wait(40);
        this.setAppearance(vol, a);
        return this.appearance(vol);
      }

      case 'vault_create': {
        this.lastCreate = { ...a, password: undefined };
        await this.wait(350);
        const path = String(a.path);
        if (!path.toLowerCase().endsWith('.nvault')) throw err('invalid_name', 'The file must end in .nvault.');
        if (this.volumes.has(path)) throw err('exists', 'A volume with this name already exists.');
        if (!a.password) throw err('invalid_password', 'A password is needed.');
        const vol = this.addVolume(path, [], Date.now());
        this.resize.created(path, a.fixed_size);
        vol.password = String(a.password);
        vol.settings = this.newVolumeSettings();
        vol.fixed = !!a.fixed_size;
        vol.singleFile = true;
        vol.slots = [{ index: 0, kind: 'password', label: 'Principale', kdf: { m_kib: 524_288, t: 3, p: 4 }, in_use: true }];
        const ov: OpenVault = { id: `v${++this.vaultSeq}`, path, mountpoint: null, seq: this.vaultSeq, lastActivity: Date.now() };
        this.open.set(ov.id, ov);
        this.touchRecent(path);
        return this.info(ov);
      }
      case 'vault_open': {
        const path = String(a.path);
        const vol = this.volumes.get(path);
        await this.wait(this.opts.unlockMs);
        if (!vol) throw err('not_found', 'The volume file is not reachable.');
        if (vol.destroyed || a.password !== vol.password) {
          if (!vol.destroyed && [...vol.duress.values()].includes(String(a.password))) vol.destroyed = true;
          throw err('wrong_password', 'Wrong password.');
        }
        const already = [...this.open.values()].find((o) => o.path === path);
        if (already) return this.info(already);
        const n = ++this.vaultSeq;
        const ov: OpenVault = {
          id: `v${n}`,
          path,
          mountpoint: this.opts.mounted && !this.opts.mountUnavailable ? `/run/user/1000/nicevault/${vol.name}` : null,
          seq: n,
          lastActivity: Date.now(),
        };
        this.open.set(ov.id, ov);
        vol.slots.forEach((s, i) => (s.in_use = i === 0 && s.kind !== 'duress'));
        this.touchRecent(path);
        if (vol.rolledBack === undefined && typeof location !== 'undefined' && new URLSearchParams(location.search).has('rolledback'))
          vol.rolledBack = { seen: 42, now: 37 };
        return this.info(ov);
      }
      case 'vault_lock': {
        const id = String(a.vault);
        if (!this.open.has(id)) return null;
        this.open.delete(id);
        await this.wait(20);
        this.emit('vault-locked', { vault: id, reason: (a.reason as LockReason) ?? 'user' });
        return null;
      }
      case 'vault_accept_rollback': {
        const { ov, vol } = this.vault(String(a.vault));
        vol.rolledBack = null;
        return this.info(ov);
      }
      case 'vault_upgrade_format': {
        await this.wait(120);
        const { ov, vol } = this.vault(String(a.vault));
        if (!vol.singleFile && this.fmtFull.has(ov.path)) throw err('volume_full', 'The fixed-size volume is too full for the new format: extend it first.');
        vol.singleFile = true;
        return this.info(ov);
      }
      case 'vault_list_open':
        return [...this.open.values()].map((o) => this.info(o));
      case 'vault_stats': {
        const { vol } = this.vault(String(a.vault));
        const files = [...vol.nodes.values()].filter((n) => n.kind === 'file');
        const used = this.used(vol);
        const st: VaultStats = {
          used_bytes: used,
          file_count: files.length,
          file_size_bytes: Math.round(Math.max(1.2 * MB, used * 1.02 + 0.4 * MB)),
          free_bytes: 21.7 * 1024 * MB,
          modified_ms: Math.max(...[...vol.nodes.values()].map((n) => n.mtime / NS)),
        };
        const cap = this.resize.capacity(vol.path);
        if (cap !== null) Object.assign(st, { file_size_bytes: cap, free_bytes: Math.max(0, cap - st.file_size_bytes) });
        return st;
      }

      case 'fs_list': {
        await this.wait(25);
        const { vol } = this.vault(String(a.vault));
        const dir = this.node(vol, Number(a.dir));
        if (dir.kind !== 'dir') throw err('not_dir', 'Not a folder.');
        const vault = String(a.vault);
        return this.children(vol, dir.id)
          .map(({ asset: _asset, created: _c, imported: _i, origin: _o, sha256: _s, ...n }): FsNode => {
            const tags = this.search.tagsOf(vault, n.id);
            return tags.length ? { ...n, tags } : n;
          })
          .filter((n) => matchesFilters(n, n.tags ?? [], a.filters));
      }
      case 'fs_mkdir': {
        await this.wait(20);
        const { vol } = this.vault(String(a.vault));
        const name = String(a.name).trim();
        if (!validName(name)) throw err('invalid_name', 'Invalid name.');
        if (this.children(vol, Number(a.parent)).some((n) => n.name === name)) throw err('exists', 'Something with this name already exists.');
        const n: MockNode = { id: vol.nextId++, parent: Number(a.parent), name, kind: 'dir', size: 0, mtime: Date.now() * NS, created: Date.now() * NS, imported: null, origin: null };
        vol.nodes.set(n.id, n);
        const { created: _c, imported: _i, origin: _o, ...out } = n;
        return out;
      }
      case 'fs_rename': {
        await this.wait(20);
        const { vol } = this.vault(String(a.vault));
        const n = this.node(vol, Number(a.node));
        const name = String(a.new_name).trim();
        if (!validName(name)) throw err('invalid_name', 'Invalid name.');
        const clash = this.children(vol, Number(a.new_parent)).find((c) => c.name === name && c.id !== n.id);
        if (clash) throw err('exists', 'Something with this name already exists.');
        for (let p = Number(a.new_parent); p !== ROOT; p = this.node(vol, p).parent) {
          if (p === n.id) throw err('invalid_name', 'A folder cannot go inside itself.');
        }
        n.parent = Number(a.new_parent);
        n.name = name;
        n.mtime = Date.now() * NS;
        return null;
      }
      case 'fs_delete': {
        await this.wait(10);
        const vault = String(a.vault);
        const { vol } = this.vault(vault);
        const ids = a.nodes as NodeId[];
        if (ids.includes(ROOT)) throw err('invalid_name', 'The root cannot be deleted.');
        return this.deleteJob(vault, vol, ids);
      }
      case 'import_size':
        return (a.paths as string[]).reduce((n, p) => n + mockPathSize(p), 0);
      case 'fs_import': {
        const vault = String(a.vault);
        const { vol } = this.vault(vault);
        const parent = Number(a.parent);
        const sources = (a.host_paths as (string | File)[]) ?? [];
        const items: [string, number, string | undefined, string][] = sources.map((s) => {
          if (typeof s === 'string') {
            const name = basename(s);
            return [name, mockPathSize(s), ASSET_BY_EXT[extOf(name)], s];
          }
          let url: string | undefined;
          try {
            url = URL.createObjectURL(s);
          } catch {
            url = undefined;
          }
          return [s.name, s.size, url, s.name];
        });
        return this.runJob(vault, 'Importazione', items.map(([n, s]) => [n, s]), {
          durationMs: items.some(([n]) => /slow/i.test(n)) ? 8000 : undefined,
          fail: this.opts.importFails ? err('io', 'No space left on the drive.') : undefined,
          onDone: () => {
            for (const [name, size, asset, origin] of items) {
              const id = vol.nextId++;
              const now = Date.now() * NS;
              vol.nodes.set(id, {
                id,
                parent,
                name: this.uniqueName(vol, parent, name),
                kind: 'file',
                size,
                mtime: now - 10 * DAY * NS,
                asset,
                created: now - 40 * DAY * NS,
                imported: now,
                origin,
              });
            }
          },
        });
      }
      case 'fs_export': {
        const vault = String(a.vault);
        const { vol } = this.vault(vault);
        const sizeOf = (id: NodeId): number => {
          const n = this.node(vol, id);
          return n.kind === 'file' ? n.size : this.children(vol, id).reduce((s, c) => s + sizeOf(c.id), 0);
        };
        const items = (a.nodes as NodeId[]).map((id) => [this.node(vol, id).name, sizeOf(id)] as [string, number]);
        return this.runJob(vault, 'Esportazione', items, {});
      }
      case 'job_cancel': {
        if (String(a.job).startsWith('props-')) {
          if (!this.cancelled.has(String(a.job))) {
            this.cancelled.add(String(a.job));
            this.emit('props-done', { job: String(a.job), error: err('cancelled', 'Cancelled.') });
          }
          return null;
        }
        this.cancelled.add(String(a.job));
        this.device.cancel(String(a.job));
        this.stick.cancel(String(a.job));
        this.emit('job-done', { job: String(a.job), error: err('cancelled', 'Cancelled.') });
        return null;
      }
      case 'mobile_volume_in':
        return String(a.uri);
      case 'mobile_import_photos':
        return this.importPhotos(String(a.vault), Number(a.parent), (a.retry as number[] | null) ?? null);
      case 'mobile_pick_files':
        await this.wait(80);
        return a.multiple ? ['/var/mobile/tmp/nv-import/2/0/Bolletta luce.pdf'] : ['/var/mobile/tmp/nv-import/2/0/Archivio fiscale.nvault'];
      case 'mobile_save_gallery': {
        await this.wait(120);
        const { vol } = this.vault(String(a.vault));
        const saved: NodeId[] = [];
        const others: NodeId[] = [];
        for (const id of a.nodes as NodeId[]) {
          const n = this.node(vol, Number(id));
          (n.kind === 'file' && isGalleryMedia(n) ? saved : others).push(n.id);
        }
        this.gallerySaved.push(...saved);
        return { saved, others };
      }
      case 'mobile_docs_busy':
        return false;
      case 'mobile_export_dir':
        return '/storage/emulated/0/Download/NiceVault';
      case 'mobile_share': {
        await this.wait(80);
        const { vol } = this.vault(String(a.vault));
        for (const id of a.nodes as number[]) this.node(vol, Number(id));
        return null;
      }
      case 'fs_open_external': {
        await this.wait(120);
        const { vol } = this.vault(String(a.vault));
        const n = this.node(vol, Number(a.node));
        if (n.kind !== 'file') throw err('is_dir', 'It is a folder.');
        if (runsCode(n.name)) throw err('unsafe_type', 'Programs and scripts are not opened from a volume: export the file first.');
        if (typeof window !== 'undefined' && n.asset) window.open(n.asset, '_blank', 'noopener');
        return { host_disk_notice: false };
      }

      case 'mount_available': {
        return mockBackends(this.opts.mountOs, this.opts.mountUnavailable);
      }
      case 'mount': {
        await this.wait(300);
        const { ov, vol } = this.vault(String(a.vault));
        if (this.opts.mountUnavailable || !mockBackends(this.opts.mountOs, false).some((b) => b.available)) throw err('mount_unavailable', 'This computer cannot mount it as a drive.');
        ov.mountpoint = `/run/user/1000/nicevault/${vol.name}`;
        return { mountpoint: ov.mountpoint };
      }
      case 'unmount': {
        await this.wait(150);
        const { ov } = this.vault(String(a.vault));
        ov.mountpoint = null;
        return null;
      }

      case 'slots_list': {
        const { vol } = this.vault(String(a.vault));
        return vol.slots.map((s) => ({ ...s }));
      }
      case 'slot_add_password': {
        await this.wait(this.opts.unlockMs);
        const { vol } = this.vault(String(a.vault));
        if (vol.slots.length >= 8) throw err('slots_full', 'There are already 8 keys.');
        if ([...vol.duress.values()].includes(String(a.password))) throw err('duress_conflict', DURESS_CONFLICT);
        const used = new Set(vol.slots.map((s) => s.index));
        let index = 0;
        while (used.has(index)) index++;
        vol.slots.push({ index, kind: 'password', label: (a.label as string) || null, kdf: { m_kib: 524_288, t: 3, p: 4 }, in_use: false });
        vol.slots.sort((x, y) => x.index - y.index);
        return vol.slots.map((s) => ({ ...s }));
      }
      case 'slot_add_duress': {
        await this.wait(this.opts.unlockMs);
        const { vol } = this.vault(String(a.vault));
        if (!a.password) throw err('invalid_password', 'A password is needed.');
        if (vol.slots.length >= 8) throw err('slots_full', 'There are already 8 keys.');
        const pw = String(a.password);
        if (pw === vol.password || [...vol.duress.values()].includes(pw)) throw err('duress_conflict', DURESS_CONFLICT);
        const used = new Set(vol.slots.map((s) => s.index));
        let index = 0;
        while (used.has(index)) index++;
        const kdf = vol.slots.find((s) => s.kind === 'password')?.kdf ?? { m_kib: 524_288, t: 3, p: 4 };
        vol.slots.push({ index, kind: 'duress', label: (a.label as string) || null, kdf: { ...kdf }, in_use: false });
        vol.slots.sort((x, y) => x.index - y.index);
        vol.duress.set(index, pw);
        return vol.slots.map((s) => ({ ...s }));
      }
      case 'vault_touch': {
        this.vault(String(a.vault));
        return null;
      }
      case 'autolock_status':
        return this.autolockStatus();
      case 'slot_change_password': {
        await this.wait(this.opts.unlockMs);
        const { vol } = this.vault(String(a.vault));
        const s = vol.slots.find((x) => x.index === Number(a.slot));
        if (!s) throw err('not_found', 'Key not found.');
        if (s.kind !== 'password') throw err('invalid_slot', 'This key is not a password.');
        if (a.old_password !== vol.password) throw err('wrong_password', 'Wrong current password.');
        if ([...vol.duress.values()].includes(String(a.new_password))) throw err('duress_conflict', DURESS_CONFLICT);
        vol.password = String(a.new_password);
        return vol.slots.map((x) => ({ ...x }));
      }
      case 'slot_rotate_key': {
        const { vol } = this.vault(String(a.vault));
        const slot = Number(a.slot);
        const s = vol.slots.find((x) => x.index === slot);
        if (!s) throw err('not_found', 'Key not found.');
        if (s.kind !== 'password') throw err('invalid_slot', 'This key is not a password.');
        await this.wait(Math.max(this.opts.unlockMs, 2600));
        if (a.password !== vol.password) throw err('wrong_password', 'Wrong password.');
        vol.slots = [{ ...s, in_use: true }];
        vol.duress.clear();
        return vol.slots.map((x) => ({ ...x }));
      }
      case 'slot_remove': {
        await this.wait(60);
        const { vol } = this.vault(String(a.vault));
        const slot = Number(a.slot);
        if (!vol.slots.some((x) => x.index === slot)) throw err('not_found', 'Key not found.');
        if (!vol.slots.some((x) => x.index !== slot && x.kind !== 'duress')) throw err('last_slot', 'It is the only key: it cannot be removed.');
        vol.slots = vol.slots.filter((x) => x.index !== slot);
        vol.duress.delete(slot);
        return vol.slots.map((s) => ({ ...s }));
      }
      case 'update_check': {
        await this.wait(250);
        const u = this.update;
        if (a.auto && (!u.checkOnStart || !this.opts.updateOnStart)) return { status: 'skipped' };
        u.lastCheck = Date.now();
        u.found = true;
        const { current, version, notes, pub_date } = MOCK_UPDATE;
        return { status: 'available', current, version, notes, pub_date };
      }
      case 'update_download': {
        if (!this.update.found) throw err('update_none', 'No update to download.');
        const total = MOCK_UPDATE.size;
        for (let i = 1; i <= 10; i++) {
          await this.wait(120);
          this.emit('update-progress', { downloaded: Math.round((total * i) / 10), total });
        }
        this.update.downloaded = true;
        return null;
      }
      case 'update_install': {
        if (!this.update.downloaded) throw err('update_none', 'Download the update first.');
        await this.wait(300);
        this.update.downloaded = false;
        if (this.opts.updateMode === 'downloaded') {
          return { mode: 'downloaded', path: `/home/utente/Scaricati/NiceVault-linux-x86_64-${MOCK_UPDATE.version}` };
        }
        this.update.installed = true;
        return { mode: 'replaced', path: `${STICK}/NiceVault-linux-x86_64` };
      }
      case 'update_restart': {
        if (!this.update.installed) throw err('update_none', 'No update installed.');
        if (typeof location !== 'undefined' && this.opts.speed > 0) location.reload();
        return null;
      }
      case 'update_prefs_get':
        return { check_on_start: this.update.checkOnStart, last_check: this.update.lastCheck, portable: true };
      case 'update_prefs_set':
        this.update.checkOnStart = Boolean(a.check_on_start);
        return { check_on_start: this.update.checkOnStart, last_check: this.update.lastCheck, portable: true };
      default:
        if (cmd.startsWith('pm_')) return this.pm.invoke(cmd, a);
        if (cmd.startsWith('tags_') || cmd === 'fs_search') return this.search.invoke(cmd, a);
        if (cmd === 'device_sticks' || cmd.startsWith('device_stick_')) return this.stick.invoke(cmd, a);
        if (cmd.startsWith('device_')) return this.device.invoke(cmd, a);
        if (cmd === 'vault_resize_info') return this.resize.invoke(cmd, a);
        if (cmd === 'vault_resize') {
          const path = this.vault(String(a.vault)).ov.path;
          const before = this.resize.capacity(path) ?? 0;
          const r = await this.resize.invoke(cmd, a);
          if ((this.resize.capacity(path) ?? 0) > before) this.fmtFull.delete(path);
          return r;
        }
        if (cmd === 'media_url' || cmd === 'media_release' || cmd === 'fs_open_mounted' || cmd === 'open_link') return this.viewers.invoke(cmd, a);
        if (cmd.startsWith('app_')) return this.global(cmd, a);
        if (cmd.startsWith('browser_')) return this.browserLink.invoke(cmd, a);
        throw err('unknown_command', `Unknown command: ${cmd}`);
    }
  }
}

export function mockBackends(os: string | null, fuseMissing: boolean): Backend[] {
  const off = (id: Backend['id'], label: string, code: string, reason: string): Backend => ({ id, label, available: false, reason, reason_code: code, native: false, link: null, min_version: null });
  const win = os?.startsWith('win_') ?? false;
  const mac = os?.startsWith('mac_') ?? false;
  const fuse: Backend = win || mac
    ? off('fuse', 'FUSE', 'linux_only', 'Linux only.')
    : fuseMissing
      ? { ...off('fuse', 'FUSE', 'fuse_missing', 'fusermount3 or /dev/fuse is missing on this computer.'), native: true }
      : { id: 'fuse', label: 'FUSE', available: true, reason: null, reason_code: null, native: true, link: null, min_version: null };
  const nfs: Backend = !mac
    ? off('nfs', 'NFS', 'macos_only', 'macOS only.')
    : os === 'mac_nonfs'
      ? { ...off('nfs', 'NFS', 'nfs_missing', "macOS's NFS client (/sbin/mount_nfs) is missing on this Mac."), native: true }
      : { id: 'nfs', label: 'NFS', available: true, reason: null, reason_code: null, native: true, link: null, min_version: null };
  const why: Record<string, [string, string]> = {
    win_missing: ['winfsp_missing', 'WinFsp is not installed (winfsp.dev).'],
    win_old: ['winfsp_unsupported', 'This WinFsp (1.10.22006.0) is too old: WinFsp 2.1 or newer is needed.'],
    win_broken: ['winfsp_load_failed', 'Cannot load C:\\Program Files (x86)\\WinFsp\\bin\\winfsp-x64.dll'],
  };
  const winfsp: Backend = !win
    ? off('winfsp', 'WinFsp', 'windows_only', 'Windows only.')
    : os === 'win_ok'
      ? { id: 'winfsp', label: 'WinFsp', available: true, reason: null, reason_code: null, native: true, link: null, min_version: null }
      : { ...off('winfsp', 'WinFsp', why[os!][0], why[os!][1]), native: true, link: 'https://winfsp.dev/rel/', min_version: '2.1' };
  return [fuse, nfs, winfsp];
}
