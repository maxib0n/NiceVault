import { localize, t } from './lib/i18n.js';

const api = globalThis.browser ?? globalThis.chrome;
const $ = (id) => document.getElementById(id);

const RESULT = {
  paired: 'pair_done',
  pair_refused: 'pair_refused',
  pair_busy: 'pair_busy',
  pair_later: 'pair_later',
  pair_blocked: 'pair_blocked',
  not_running: 'st_launch_failed',
  missing: 'st_missing',
  unsupported: 'st_unsupported',
};

api.runtime.onMessage.addListener((m) => {
  if (m?.kind === 'pair_code') {
    $('code').textContent = `${m.code.slice(0, 3)} ${m.code.slice(3)}`;
    $('code-box').hidden = false;
    $('status').textContent = t('pair_confirm_in_app');
  }
});

$('start').addEventListener('click', async (e) => {
  if (!e.isTrusted) return;
  $('start').disabled = true;
  $('status').textContent = t('st_starting');
  const r = await api.runtime.sendMessage({ kind: 'pair_start' });
  const res = r?.result ?? 'error';
  $('status').textContent = t(RESULT[res] ?? 'st_error');
  $('status').dataset.result = res;
  document.body.dataset.result = res;
  $('code-box').hidden = res !== 'paired' && $('code-box').hidden;
  $('start').disabled = res === 'paired';
});

async function perm() {
  if (!api.permissions?.contains) return;
  $('perm-box').hidden = await api.permissions.contains({ origins: ['<all_urls>'] }).catch(() => true);
}
$('perm').addEventListener('click', async () => {
  await api.permissions.request({ origins: ['<all_urls>'] }).catch(() => false);
  await perm();
});

localize();
void perm();
