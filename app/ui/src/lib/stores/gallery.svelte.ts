import * as api from '../api';
import type { NodeId, VaultId } from '../types';
import { m } from '../../paraglide/messages.js';
import { toasts } from './toasts.svelte';

class Gallery {
  asking = $state(false);
  private answer: ((yes: boolean | null) => void) | null = null;

  async enabled(): Promise<boolean | null> {
    const s = await api.appSettingsGet().catch(() => null);
    if (s && typeof s.gallery_save === 'boolean') return s.gallery_save;
    const yes = await new Promise<boolean | null>((res) => {
      this.answer = res;
      this.asking = true;
    });
    if (yes !== null) await api.appSettingsSet({ gallery_save: yes }).catch(() => undefined);
    return yes;
  }

  reply(yes: boolean | null) {
    this.asking = false;
    const a = this.answer;
    this.answer = null;
    a?.(yes);
  }

  async save(vault: VaultId, ids: NodeId[]): Promise<NodeId[] | null> {
    try {
      const r = await api.mobileSaveGallery(vault, ids);
      if (r.saved.length) toasts.push(m.gallery_saved({ n: String(r.saved.length) }), 'success');
      return r.others;
    } catch (e) {
      const code = (e as { code?: string })?.code;
      if (code === 'unsupported') return ids;
      toasts.push(code === 'gallery_denied' ? m.gallery_denied() : m.gallery_failed(), 'error');
      return null;
    }
  }
}

export const gallery = new Gallery();
