import { describe, expect, it, vi } from 'vitest';
import { moovFirst } from './videoThumb';

function box(type: string, len: number): number[] {
  const b = new Array(len).fill(0);
  b[0] = (len >>> 24) & 255;
  b[1] = (len >>> 16) & 255;
  b[2] = (len >>> 8) & 255;
  b[3] = len & 255;
  for (let i = 0; i < 4; i++) b[4 + i] = type.charCodeAt(i);
  return b;
}

describe('moovFirst', () => {
  it('finds the index before the media data', () => {
    expect(moovFirst(new Uint8Array([...box('ftyp', 24), ...box('moov', 16), ...box('mdat', 16)]))).toBe(true);
  });
  it('finds the index at the end', () => {
    expect(moovFirst(new Uint8Array([...box('ftyp', 24), ...box('mdat', 16), ...box('moov', 16)]))).toBe(false);
  });
  it('does not know other files', () => {
    expect(moovFirst(new Uint8Array([0x1a, 0x45, 0xdf, 0xa3, 0, 0, 0, 0, 1, 2]))).toBe(null);
  });
});

describe('videoThumb fallback', () => {
  it('falls back to the Blob path when the canvas is tainted', async () => {
    vi.resetModules();
    const mediaRelease = vi.fn();
    vi.doMock('./api', () => ({
      isTauri: true,
      mediaUrl: async () => 'http://127.0.0.1:1/m/x',
      mediaRelease,
      onVaultLocked: () => undefined,
      previewUrl: () => 'nvault://x',
    }));
    vi.doMock('./mediaBlob', () => ({ fetchAsBlob: async () => new Blob(['x']) }));
    vi.stubGlobal('fetch', async () => new Response(new Uint8Array(8), { status: 200, headers: { 'content-type': 'video/mp4' } }));
    const srcs: string[] = [];
    let cross: string | null | undefined;
    vi.stubGlobal('document', { createElement: ((tag: string) => {
      if (tag === 'video') {
        const v: Record<string, unknown> = {
          videoWidth: 100, videoHeight: 50, duration: 0, muted: false, playsInline: false, preload: '',
          setAttribute() {}, removeAttribute() {}, load() {},
          addEventListener(ev: string, cb: () => void) { if (ev === 'loadeddata') setTimeout(cb, 0); },
          removeEventListener() {},
        };
        Object.defineProperty(v, 'src', { set: (x: string) => srcs.push(x), get: () => '' });
        Object.defineProperty(v, 'crossOrigin', { set: (x: string) => (cross = x), get: () => cross });
        return v as unknown as HTMLVideoElement;
      }
      if (tag === 'canvas') {
        const c = {
          width: 0, height: 0,
          getContext: () => ({ drawImage() { if (srcs.length === 1) throw new DOMException('tainted', 'SecurityError'); } }),
          toBlob: (cb: (b: Blob) => void) => cb(new Blob(['j'])),
        };
        return c as unknown as HTMLCanvasElement;
      }
      throw new Error(tag);
    }) });
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:thumb');
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined);
    const { videoThumb } = await import('./videoThumb');
    const u = await videoThumb('v', { id: 1, mtime: 1, size: 10 } as never, 128);
    expect(u).toBe('blob:thumb');
    expect(srcs[0]).toBe('http://127.0.0.1:1/m/x');
    expect(srcs).toHaveLength(2);
  });
});
