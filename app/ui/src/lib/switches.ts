import { ACCENTS, type AccentId } from './accents';

export type ScreenSwitch =
  | 'welcome' | 'create' | 'unlock' | 'vault' | 'details' | 'passwords' | 'settings' | 'device';

export interface Switches {
  screen: ScreenSwitch | null;
  step: 1 | 2 | 3 | null;
  empty: boolean;
  error: 'wrong-password' | 'toast' | 'import' | 'delete' | null;
  mount: 'unavailable' | 'mounted' | 'win_missing' | 'win_old' | 'win_broken' | 'win_ok' | 'mac_ok' | 'mac_nonfs' | null;
  jobs: boolean;
  theme: 'light' | 'dark' | null;
  unlocking: boolean;
  locked: 'idle' | 'removed' | 'sleep' | 'screen' | 'user' | null;
  view: 'grid' | 'list' | null;
  dir: string | null;
  preview: string | null;
  select: string | null;
  menu: boolean;
  dialog: string | null;
  entry: string | null;
  q: string | null;
  filters: string | null;
  tagger: boolean;
  tags: 'manage' | null;
  indexing: string | null;
  lang: 'it' | 'en' | null;
  slots1: boolean;
  autolock: number | null;
  drop: boolean;
  accent: AccentId | null;
  app_theme: 'system' | 'light' | 'dark' | null;
  app_accent: AccentId | null;
  video_arrows: 'seek' | 'step' | null;
  scale: number | null;
  emblem: 'initial' | 'icon' | 'image' | 'hidden' | null;
  appearance: 'visible' | 'encrypted' | null;
  frame: string | null;
  update: 'available' | 'downloads' | null;
  pm: number | null;
  installed: boolean;
  device_step: 'disk' | 'options' | 'confirm' | 'type' | 'progress' | 'done' | 'error' | null;
  device: 'empty' | 'incomplete' | null;
  device_fail: string | null;
  device_offline: boolean;
  device_slow: 'candidates' | 'options' | 'plan' | 'create' | 'auth' | null;
  device_volumes: boolean;
  device_stuck_ms: number | null;
  stick: 'old' | 'missing' | 'current' | 'unmounted' | 'tight' | null;
  stick_offline: boolean;
  stick_fail: string | null;
  fixed: boolean;
  resize_fail: string | null;
  fmt_full: boolean;
  slow_delete: number | null;
  demo: boolean;
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
