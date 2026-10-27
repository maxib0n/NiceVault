import * as api from '../lib/api';
import { toasts } from '../lib/stores/toasts.svelte';
import type { NodeId, VaultId } from '../lib/types';

export async function openMounted(vault: VaultId, node: NodeId): Promise<void> {
  try {
    await api.fsOpenMounted(vault, node);
  } catch (e) {
    toasts.error(e);
  }
}

export async function openLink(url: string): Promise<boolean> {
  try {
    await api.openLink(url);
    return true;
  } catch (e) {
    toasts.error(e);
    return false;
  }
}
