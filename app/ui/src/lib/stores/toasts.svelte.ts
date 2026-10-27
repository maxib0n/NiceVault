import { errorText } from '../errors';
import { play } from '../sound';

export type ToastKind = 'error' | 'info' | 'success';

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

class Toasts {
  items = $state<Toast[]>([]);
  private seq = 0;

  push(text: string, kind: ToastKind = 'info', timeoutMs = kind === 'error' ? 6000 : 3500): number {
    const id = ++this.seq;
    if (kind === 'error') play('error');
    this.items.push({ id, kind, text });
    if (this.items.length > 4) this.items.shift();
    if (timeoutMs > 0) setTimeout(() => this.dismiss(id), timeoutMs);
    return id;
  }

  error(e: unknown): number {
    return this.push(errorText(e), 'error');
  }

  clear(): void {
    this.items = [];
  }

  dismiss(id: number): void {
    this.items = this.items.filter((t) => t.id !== id);
  }
}

export const toasts = new Toasts();
