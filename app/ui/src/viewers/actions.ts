// Viewer actions that leave the app: both go through the backend (viewers.rs), which checks
// them again. Errors become toasts.
import * as api from '../lib/api';
import { toasts } from '../lib/stores/toasts.svelte';
import type { NodeId, VaultId } from '../lib/types';

/** "Apri con app esterna": the file on the mounted drive, opened by the system (no copy). */
export async function openMounted(vault: VaultId, node: NodeId): Promise<void> {
  try {
    await api.fsOpenMounted(vault, node);
  } catch (e) {
    toasts.error(e);
  }
}

/** a link of a document, after the reader confirmed it */
export async function openLink(url: string): Promise<boolean> {
  try {
    await api.openLink(url);
    return true;
  } catch (e) {
    toasts.error(e);
    return false;
  }
}
