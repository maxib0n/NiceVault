// URL query switches. They exist so every screen and state can be captured
// directly in a plain browser (`pnpm dev`) without clicking through the app.
// Screen/state switches are honoured only in mock mode; `theme`, `accent` and `lang` work everywhere.
import { ACCENTS, type AccentId } from './accents';

export type ScreenSwitch =
  | 'welcome' | 'create' | 'unlock' | 'vault' | 'details' | 'passwords' | 'settings' | 'device';

export interface Switches {
  screen: ScreenSwitch | null;
  /** create wizard step 1..3 */
  step: 1 | 2 | 3 | null;
  /** no recent volumes on Benvenuto; opened vault has an empty root */
  empty: boolean;
  /** wrong-password: Sblocca shows the error · toast: an error toast is shown · import: import jobs fail ·
   *  delete: delete jobs stop half way, the volume full (no_room_to_delete) */
  error: 'wrong-password' | 'toast' | 'import' | 'delete' | null;
  /** unavailable: no mount backend works · mounted: vault opens already mounted ·
   *  win_missing / win_old / win_broken / win_ok: as on Windows without WinFsp, with one too old,
   *  one that does not load, with WinFsp · mac_ok / mac_nonfs: as on macOS (NFS client missing) */
  mount: 'unavailable' | 'mounted' | 'win_missing' | 'win_old' | 'win_broken' | 'win_ok' | 'mac_ok' | 'mac_nonfs' | null;
  /** seed the job tray with running, finished and failed jobs */
  jobs: boolean;
  theme: 'light' | 'dark' | null;
  /** Sblocca frozen in the unlocking state */
  unlocking: boolean;
  /** Sblocca shows why the volume was locked */
  locked: 'idle' | 'removed' | 'sleep' | 'screen' | 'user' | null;
  view: 'grid' | 'list' | null;
  /** folder path inside the vault, e.g. "Foto/Vacanze 2024" */
  dir: string | null;
  /** open quick look on a file kind (first match in the folder) or exact name */
  preview: string | null;
  /** select: "all" or a count of leading items */
  select: string | null;
  /** open the context menu on the first item */
  menu: boolean;
  /** open a dialog: delete | rename | new-folder (vault) · add-password | change-password | remove-slot |
   *  add-duress | add-duress-2 | remove-duress | emblem (details) · pm-edit | pm-new | pm-import | pm-import-2 |
   *  pm-generator | pm-history | pm-delete (passwords) · licences (settings) */
  dialog: string | null;
  /** password manager: the entry shown (id or exact title) */
  entry: string | null;
  /** a search already typed: password manager (screen=passwords) or the volume's search (screen=vault) */
  q: string | null;
  /** volume: filter chips set, "kinds:photos,videos;size:small|medium|large;date:today|week|month|year;tags:1,2;any:1;scope:folder" */
  filters: string | null;
  /** volume: the tag picker open on the selection (select=N, else the first two files) */
  tagger: boolean;
  /** volume: manage = the tag manager open */
  tags: 'manage' | null;
  /** volume: the search index filling, "done/total" (the quiet "Indicizzazione…" line) */
  indexing: string | null;
  /** UI language (it | en); like theme it works outside the mock too (lib/i18n.svelte.ts reads it) */
  lang: 'it' | 'en' | null;
  /** only one keyslot (shows the "can't remove the last one" state) */
  slots1: boolean;
  /** idle auto-lock after N seconds (to watch the vault-locked flow) */
  autolock: number | null;
  /** drag-over overlay visible */
  drop: boolean;
  /** accent preset id (accents.ts); works outside the mock too, like theme */
  accent: AccentId | null;
  /** mock: the app's own look (Impostazioni > Aspetto dell'app) the session starts with */
  app_theme: 'system' | 'light' | 'dark' | null;
  app_accent: AccentId | null;
  /** mock: what ←/→ do in a full-screen video the session starts with */
  video_arrows: 'seek' | 'step' | null;
  /** mock: interface size, percent (90 | 100 | 110 | 125 | 150) */
  scale: number | null;
  /** force every mock volume's emblem; hidden = opted out of showing it while locked */
  emblem: 'initial' | 'icon' | 'image' | 'hidden' | null;
  /** force every mock volume's appearance visibility (encrypted = like emblem=hidden: neutral locked screens) */
  appearance: 'visible' | 'encrypted' | null;
  /** force every mock volume's emblem frame (ornament/frames.ts) */
  frame: string | null;
  /** available: the start-up check finds the mock's newer version · downloads: and installing saves it to Downloads */
  update: 'available' | 'downloads' | null;
  /** password manager: N extra generated entries in every seeded volume (perf runs, e.g. ?pm=2000) */
  pm: number | null;
  /** mock: run as installed (settings "persisted", Avvia all'accesso / Avvia ridotto shown) instead of portable */
  installed: boolean;
  /** Crea dispositivo (screen=device): the wizard step shown (disk | options | confirm | type | progress | done | error) */
  device_step: 'disk' | 'options' | 'confirm' | 'type' | 'progress' | 'done' | 'error' | null;
  /** mock drives: empty = none plugged in · incomplete = the SD card is an interrupted device ("Completa") */
  device: 'empty' | 'incomplete' | null;
  /** mock: the device job fails with this code, optionally at a step (elevation_denied, device_gone@copy) */
  device_fail: string | null;
  /** mock: the builds cannot be downloaded (only the local Linux one ends up on the device) */
  device_offline: boolean;
  /** mock: that device command never answers (the busy states), or =auth: the job waits at the system's password window */
  device_slow: 'candidates' | 'options' | 'plan' | 'create' | 'auth' | null;
  /** mock: the SanDisk holds NiceVault volumes (the erase warning on the confirm step) */
  device_volumes: boolean;
  /** Crea dispositivo: ms before a pending step offers Annulla / Riprova (default 30 s; tests, screenshots) */
  device_stuck_ms: number | null;
  /** mock: a NiceVault device holds the volumes (Aggiorna i programmi sulla chiavetta, stickMock.ts):
   *  old = every program older · missing = no macOS one · current = up to date · unmounted = NiceVault partition not mounted */
  stick: 'old' | 'missing' | 'current' | 'unmounted' | 'tight' | null;
  /** mock: the other OSes' programs cannot be downloaded · stick_fail: the update fails with this code */
  stick_offline: boolean;
  stick_fail: string | null;
  /** mock: seeded volumes are fixed-size (Estendi / Riduci) · resize_fail: vault_resize fails with this code */
  fixed: boolean;
  resize_fail: string | null;
  /** mock (with ?fixed): the seeded fixed-size volumes are too full for Aggiorna formato until extended */
  fmt_full: boolean;
  /** mock: delete jobs take this many ms per committed batch (bare ?slow_delete = 700) */
  slow_delete: number | null;
  /** mock: English sample content (volumes, files, tags, passwords) for the README screenshots (mockDemo.ts) */
  demo: boolean;
  /** mock Benvenuto: long = more passports with 40+ character names and long places (the fit checks) */
  recents: 'long' | null;
}

function pick<T extends string>(v: string | null, allowed: readonly T[]): T | null {
  return v !== null && (allowed as readonly string[]).includes(v) ? (v as T) : null;
}

export function parseSwitches(search: string): Switches {
  const q = new URLSearchParams(search);
  const flag = (k: string) => q.has(k) && q.get(k) !== '0' && q.get(k) !== 'false';
  const step = Number(q.get('step'));
  const autolock = Number(q.get('autolock'));
  return {
    screen: pick(q.get('screen'), [
      'welcome', 'create', 'unlock', 'vault', 'details', 'passwords', 'settings', 'device',
    ] as const),
    step: step === 1 || step === 2 || step === 3 ? step : null,
    empty: flag('empty'),
    error: pick(q.get('error'), ['wrong-password', 'toast', 'import', 'delete'] as const),
    mount: pick(q.get('mount'), ['unavailable', 'mounted', 'win_missing', 'win_old', 'win_broken', 'win_ok', 'mac_ok', 'mac_nonfs'] as const),
    jobs: flag('jobs'),
    theme: pick(q.get('theme'), ['light', 'dark'] as const),
    unlocking: flag('unlocking'),
    locked: pick(q.get('locked'), ['idle', 'removed', 'sleep', 'screen', 'user'] as const),
    view: pick(q.get('view'), ['grid', 'list'] as const),
    dir: q.get('dir'),
    preview: q.get('preview'),
    select: q.get('select'),
    menu: flag('menu'),
    dialog: q.get('dialog'),
    entry: q.get('entry'),
    q: q.get('q'),
    filters: q.get('filters'),
    tagger: flag('tagger'),
    tags: pick(q.get('tags'), ['manage'] as const),
    indexing: /^\d+\/\d+$/.test(q.get('indexing') ?? '') ? q.get('indexing') : null,
    lang: pick(q.get('lang'), ['it', 'en'] as const),
    slots1: q.get('slots') === '1',
    autolock: Number.isFinite(autolock) && autolock > 0 ? autolock : null,
    drop: flag('drop'),
    accent: pick(q.get('accent'), ACCENTS.map((a) => a.id)),
    app_theme: pick(q.get('app_theme'), ['system', 'light', 'dark'] as const),
    app_accent: pick(q.get('app_accent'), ACCENTS.map((a) => a.id)),
    video_arrows: pick(q.get('video_arrows'), ['seek', 'step'] as const),
    scale: [90, 100, 110, 125, 150].includes(Number(q.get('scale'))) ? Number(q.get('scale')) : null,
    emblem: pick(q.get('emblem'), ['initial', 'icon', 'image', 'hidden'] as const),
    appearance: pick(q.get('appearance'), ['visible', 'encrypted'] as const),
    frame: pick(q.get('frame'), ['guilloche', 'laurel', 'rope', 'meander', 'starburst', 'hairline'] as const),
    update: pick(q.get('update'), ['available', 'downloads'] as const),
    installed: flag('installed'),
    device_step: pick(q.get('device_step'), ['disk', 'options', 'confirm', 'type', 'progress', 'done', 'error'] as const),
    device: pick(q.get('device'), ['empty', 'incomplete'] as const),
    device_fail: q.get('device_fail'),
    device_offline: flag('device_offline'),
    device_slow: pick(q.get('device_slow'), ['candidates', 'options', 'plan', 'create', 'auth'] as const),
    device_volumes: flag('device_volumes'),
    device_stuck_ms: Number(q.get('device_stuck_ms')) > 0 ? Number(q.get('device_stuck_ms')) : null,
    stick: pick(q.get('stick'), ['old', 'missing', 'current', 'unmounted', 'tight'] as const),
    stick_offline: flag('stick_offline'),
    stick_fail: q.get('stick_fail'),
    fixed: flag('fixed'),
    resize_fail: q.get('resize_fail'),
    fmt_full: flag('fmt_full'),
    slow_delete: q.has('slow_delete') ? Number(q.get('slow_delete')) > 0 ? Number(q.get('slow_delete')) : 700 : null,
    demo: flag('demo'),
    recents: pick(q.get('recents'), ['long'] as const),
    pm: Number.isInteger(Number(q.get('pm'))) && Number(q.get('pm')) > 0 ? Math.min(Number(q.get('pm')), 20000) : null,
  };
}

export const switches: Switches = parseSwitches(
  typeof location !== 'undefined' ? location.search : '',
);
