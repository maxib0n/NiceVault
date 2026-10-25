// Applies URL switches (mock mode only) so the designer can land directly on
// any screen/state: see switches.ts for the list.
import { m } from '../paraglide/messages.js';
import * as api from './api';
import { fileType } from './filetype';
import { MOCK_PASSWORD, STICK } from './mock';
import { switches } from './switches';
import { app } from './stores/app.svelte';
import { idle } from './stores/idle.svelte';
import { jobs } from './stores/jobs.svelte';
import { look } from './stores/look.svelte';
import { prefs } from './stores/prefs.svelte';
import { parseFilterSwitch } from './stores/search.svelte';
import { toasts } from './stores/toasts.svelte';
import { vaults } from './stores/vaults.svelte';
import type { RecentEntry } from './types';

const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms));

function firstRecent(): RecentEntry {
  return (
    app.recents.find((r) => r.exists) ?? {
      path: `${STICK}/Documenti.nvault`,
      name: 'Documenti',
      size_bytes: 0,
      last_opened: Date.now(),
      exists: true,
      unlock_ms: 700,
    }
  );
}

export async function applySwitches(): Promise<void> {
  look.start(); // per-volume accent / theme / auto-lock (both modes)
  if (api.isTauri) return;
  const s = switches;
  if (s.view) prefs.view = s.view; // not persisted
  if (s.autolock) idle.timeoutMs = s.autolock * 1000;
  if (s.error === 'toast') toasts.push(m.toast_damaged_demo(), 'error', 0);

  switch (s.screen) {
    case 'settings':
      app.openSettings();
      return;
    case 'create':
      app.goto('create');
      return;
    case 'device':
      app.goto('device'); // DeviceWizard.svelte reads device_step
      return;
    case 'unlock': {
      const target = firstRecent();
      app.showUnlock(target, s.locked);
      if (s.unlocking) {
        vaults.unlock = { ...vaults.unlock, phase: 'unlocking', startedAt: performance.now(), expectedMs: target.unlock_ms ?? 700 };
      } else if (s.error === 'wrong-password') {
        await vaults.unlockVault(target, 'password-sbagliata');
      }
      return;
    }
    case 'vault':
    case 'details':
    case 'passwords': {
      if (s.empty) {
        // empty=1 with vault: open a fresh, empty volume
        await vaults.createVault(`${STICK}/Vuoto.nvault`, MOCK_PASSWORD).catch(() => undefined);
      } else {
        const ok = await vaults.unlockVault(firstRecent(), MOCK_PASSWORD);
        if (!ok) return;
      }
      app.goto(s.screen);
      if (s.screen === 'passwords') return; // Passwords.svelte reads entry / q / dialog
      const b = vaults.browser;
      if (!b) return;
      if (s.dir) await b.gotoPath(s.dir);
      else await b.load();
      if (s.jobs && vaults.activeId) {
        for (const j of api.mock().seedJobs(vaults.activeId)) jobs.track(j.job, j.kind, vaults.activeId, j.label, j.kind === 'export' ? '/home/utente/Scrivania' : b.vault.name);
        jobs.open = true;
      }
      // the volume's search: filters (chips), a query typed, the index filling
      if (s.screen === 'vault' && (s.filters || s.q)) {
        if (s.filters) {
          const p = parseFilterSwitch(s.filters);
          b.filters = p.filters;
          if (p.scope) b.scope = p.scope;
        }
        if (s.q) {
          b.openSearch();
          b.query = s.q;
          await b.runSearch();
        } else await b.load();
      }
      if (s.screen === 'vault' && s.indexing) {
        const [done, total] = s.indexing.split('/').map(Number);
        b.openSearch();
        b.indexing = { done, total };
      }
      if (s.select === 'all') b.selectAll();
      else if (s.select && Number(s.select) > 0) {
        b.click(b.order[0], {});
        b.moveTo(Number(s.select) - 1, { shift: true });
      }
      if (s.preview) {
        const want = s.preview.toLowerCase();
        const n = b.sorted.find((x) => x.name.toLowerCase() === want) ?? b.sorted.find((x) => x.kind === 'file' && fileType(x) === want);
        if (n) {
          b.click(n.id, {});
          b.quickLook = n.id;
        }
      }
      if (s.drop) b.dragOver = true;
      await tick(50);
      if (s.menu && b.order.length) {
        const first = document.querySelector<HTMLElement>(`#item-${b.order[0]}`)?.getBoundingClientRect();
        b.openMenu((first?.left ?? 300) + 40, (first?.top ?? 120) + 40, b.order[0]);
      }
      if (s.screen === 'vault' && s.tagger) {
        if (!b.selectedIds.length) {
          const files = b.sorted.filter((n) => n.kind === 'file').slice(0, 2);
          if (files.length) b.sel = { selected: new Set(files.map((n) => n.id)), anchor: files[0].id, focus: files[files.length - 1].id };
        }
        const at = document.querySelector<HTMLElement>(`#item-${b.selectedIds[0]}`)?.getBoundingClientRect();
        b.openTagger({ x: (at?.left ?? 300) + 60, y: (at?.bottom ?? 160) + 4 });
      }
      if (s.screen === 'vault' && s.tags === 'manage') b.tagManager = { x: window.innerWidth - 372, y: 132 };
      if (s.screen === 'vault' && s.dialog) {
        const files = b.sorted.filter((n) => n.kind === 'file');
        if (s.dialog === 'delete') b.requestDelete(b.selectedIds.length ? b.selectedIds : b.order.slice(0, 3));
        else if (s.dialog === 'rename' && files[0]) {
          b.click(files[0].id, {});
          b.startRename(files[0].id);
        } else if (s.dialog === 'new-folder') await b.newFolder();
      }
      return;
    }
    default:
      return;
  }
}
