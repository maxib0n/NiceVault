import type { ApiError, EventMap, EventName, ResizeInfo, ResizePhase, VaultId } from './types';
import { FIXED_MAX, FIXED_MIN, GIB, MIB, down4k, limits } from './resize';

export interface ResizeMockHost {
  volume(vault: VaultId): { path: string; used: number };
  wait(ms: number): Promise<void>;
  emit<E extends EventName>(event: E, payload: EventMap[E]): void;
}

export interface ResizeMockOptions {
  fixed: boolean;
  fail: string | null;
}

interface Fixed {
  capacity: number;
  slack: number;
}

const DRIVE_FREE = 40 * GIB;

function err(code: string, message: string): ApiError {
  return { code, message };
}

export class ResizeMock {
  private fixed = new Map<string, Fixed>();
  private free = DRIVE_FREE;

  constructor(
    private host: ResizeMockHost,
    private opts: ResizeMockOptions,
  ) {}

  seed(path: string): void {
    if (this.opts.fixed) this.fixed.set(path, { capacity: 4 * GIB, slack: 300 * MIB });
  }

  created(path: string, fixedSize: unknown): void {
    if (typeof fixedSize === 'number' && fixedSize > 0) this.fixed.set(path, { capacity: down4k(fixedSize), slack: 0 });
  }

  capacity(path: string): number | null {
    return this.fixed.get(path)?.capacity ?? null;
  }

  info(vault: VaultId): ResizeInfo {
    const { path, used } = this.host.volume(vault);
    const store = Math.round(used * 1.02 + 0.4 * MIB);
    const f = this.fixed.get(path);
    if (!f) {
      return { capacity: null, file_bytes: store, used_bytes: store, min_capacity: store, min_in_place: store, max_capacity: store, drive_free: this.free };
    }
    const { min, inPlace, max } = limits(f.capacity, store + f.slack, store, this.free);
    return { capacity: f.capacity, file_bytes: f.capacity, used_bytes: store, min_capacity: min, min_in_place: inPlace, max_capacity: max, drive_free: this.free };
  }

  async invoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
    const vault = String(a.vault);
    if (cmd === 'vault_resize_info') {
      await this.host.wait(30);
      return this.info(vault);
    }
    if (cmd !== 'vault_resize') throw err('error', `mock: unknown command ${cmd}`);
    const info = this.info(vault);
    const f = this.fixed.get(this.host.volume(vault).path);
    if (!f || info.capacity === null) throw err('not_fixed', 'This volume grows by itself: there is no size to change.');
    const to = down4k(Number(a.size));
    if (!(to >= FIXED_MIN && to <= FIXED_MAX)) throw err('out_of_range', `The size must be between ${FIXED_MIN} and ${FIXED_MAX} bytes.`);
    if (to === f.capacity) return info;
    const emit = (phase: ResizePhase, done: number, total: number) => this.host.emit('vault-resize', { vault, phase, done, total });
    const failNow = () => {
      if (this.opts.fail) throw err(this.opts.fail, 'Read or write error: mock failure.');
    };
    if (to > f.capacity) {
      if (to - f.capacity > this.free) throw err('no_space', `Not enough free space on the drive: ${to - f.capacity} bytes needed, ${this.free} free.`);
      emit('extend', 0, to - f.capacity);
      await this.host.wait(500);
      failNow();
      emit('commit', 0, 1);
      await this.host.wait(150);
      this.free -= to - f.capacity;
    } else {
      if (to < info.min_capacity) throw err('too_small', `The data in the volume needs at least ${info.min_capacity} bytes.`);
      if (to < info.min_in_place) {
        if (to > this.free) throw err('no_space', `Not enough free space on the drive: ${to} bytes needed, ${this.free} free.`);
        const total = info.used_bytes;
        for (let i = 1; i <= 10; i++) {
          emit('compact', Math.round((total * i) / 10), total);
          await this.host.wait(120);
          if (i === 3) failNow();
        }
        f.slack = 0;
      }
      emit('commit', 0, 1);
      await this.host.wait(150);
      failNow();
      emit('truncate', 0, 1);
      await this.host.wait(100);
      this.free += f.capacity - to;
    }
    f.capacity = to;
    return this.info(vault);
  }
}
