import { localize, t } from './lib/i18n.js';

const api = globalThis.browser ?? globalThis.chrome;
const $ = (id) => document.getElementById(id);
const DEFAULTS = { launch: 'auto', fieldIcon: true, siteMemory: true, copyTotp: false };

async function opts() {
  return { ...DEFAULTS, ...((await api.storage.local.get('opts')).opts ?? {}) };
}

async function save(patch) {
  await api.storage.local.set({ opts: { ...(await opts()), ...patch } });
}

async function refresh() {
  const o = await opts();
  for (const r of document.querySelectorAll('input[name=launch]')) r.checked = r.value === o.launch;
  for (const k of ['fieldIcon', 'siteMemory', 'copyTotp']) $(k).checked = o[k];
  const s = await api.runtime.sendMessage({ kind: 'get_state' });
  const st = s?.state;
  $('link-state').textContent = st === 'ready' ? t('opt_linked') : st === 'unpaired' ? t('st_unpaired') : st === 'missing' ? t('st_missing') : st === 'not_running' ? t('st_closed') : '';
  $('pair').hidden = st === 'ready';
  $('unpair').hidden = st !== 'ready';
  if (api.permissions?.contains) {
    const has = await api.permissions.contains({ origins: ['<all_urls>'] }).catch(() => true);
    $('perm-box').hidden = has;
  }
}

for (const r of document.querySelectorAll('input[name=launch]')) r.addEventListener('change', () => save({ launch: r.value }));
for (const k of ['fieldIcon', 'siteMemory', 'copyTotp']) $(k).addEventListener('change', () => save({ [k]: $(k).checked }));
$('forget').addEventListener('click', async () => {
  await api.runtime.sendMessage({ kind: 'forget_memory' });
  $('forgotten').hidden = false;
});
$('pair').addEventListener('click', () => void api.tabs.create({ url: api.runtime.getURL('pair.html') }));
$('unpair').addEventListener('click', async () => {
  await api.runtime.sendMessage({ kind: 'unpair_local' });
  await refresh();
});
$('perm').addEventListener('click', async () => {
  await api.permissions.request({ origins: ['<all_urls>'] }).catch(() => false);
  await refresh();
});

localize();
void refresh();
