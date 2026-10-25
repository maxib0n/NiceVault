// Phones: Esporta puts pictures and videos in the system gallery ("Salva foto e video nella
// Galleria", a global setting; asked once the first time a photo is exported), and the item
// sheet's "Salva nella Galleria" (lib/gallery.ts, src-tauri mobile.rs mobile_save_gallery).
import * as api from '../api';
import type { NodeId, VaultId } from '../types';
import { m } from '../../paraglide/messages.js';
import { toasts } from './toasts.svelte';

class Gallery {
  /** the "Salvare nella Galleria?" question is up (GalleryAsk.svelte answers it) */
  asking = $state(false);
  private answer: ((yes: boolean | null) => void) | null = null;

  /** The setting; null (never asked): asks now and remembers the answer. null = cancelled. */
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

  /** GalleryAsk's keys: true / false remember, null (Esc, scrim) cancels this export only. */
  reply(yes: boolean | null) {
    this.asking = false;
    const a = this.answer;
    this.answer = null;
    a?.(yes);
  }

  /** Saves into the gallery; returns what is left for Files, or null when it failed (toasted). */
  async save(vault: VaultId, ids: NodeId[]): Promise<NodeId[] | null> {
    try {
      const r = await api.mobileSaveGallery(vault, ids);
      if (r.saved.length) toasts.push(m.gallery_saved({ n: String(r.saved.length) }), 'success');
      return r.others;
    } catch (e) {
      const code = (e as { code?: string })?.code;
      // Android before 10: no gallery without a storage permission: Files, as before
      if (code === 'unsupported') return ids;
      toasts.push(code === 'gallery_denied' ? m.gallery_denied() : m.gallery_failed(), 'error');
      return null;
    }
  }
}

export const gallery = new Gallery();
