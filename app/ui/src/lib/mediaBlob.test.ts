import { afterEach, describe, expect, it, vi } from 'vitest';
import { fetchAsBlob } from './mediaBlob';

const MB = 1024 * 1024;

/** A fake nvault:// endpoint over `size` bytes honouring Range like preview.rs. */
function server(size: number, wholeLimit = 8 * MB) {
  const ranges: string[] = [];
  vi.stubGlobal('fetch', async (_url: string, init?: { headers?: Record<string, string> }) => {
    const range = init?.headers?.Range;
    if (!range) {
      const n = Math.min(size, wholeLimit);
      return new Response(new Uint8Array(n), { status: n === size ? 200 : 206, headers: { 'content-type': 'video/webm' } });
    }
    ranges.push(range);
    const [a, b] = range.replace('bytes=', '').split('-').map(Number);
    return new Response(new Uint8Array(b - a + 1), { status: 206, headers: { 'content-type': 'video/webm', 'content-range': `bytes ${a}-${b}/${size}` } });
  });
  return ranges;
}

afterEach(() => vi.unstubAllGlobals());

describe('fetchAsBlob', () => {
  it('pulls a large file in ≤32 MiB ranged slices and keeps the type', async () => {
    const size = 70 * MB + 123;
    const ranges = server(size);
    const blob = await fetchAsBlob('nvault://v/7', size);
    expect(blob.size).toBe(size);
    expect(blob.type).toBe('video/webm');
    expect(ranges).toEqual([`bytes=0-${32 * MB - 1}`, `bytes=${32 * MB}-${64 * MB - 1}`, `bytes=${64 * MB}-${size - 1}`]);
  });

  it('a small file is one request', async () => {
    const ranges = server(1000);
    const blob = await fetchAsBlob('nvault://v/7', 1000);
    expect(blob.size).toBe(1000);
    expect(ranges).toEqual(['bytes=0-999']);
  });

  it('an empty file does not send an invalid range', async () => {
    const ranges = server(0);
    const blob = await fetchAsBlob('nvault://v/7', 0);
    expect(blob.size).toBe(0);
    expect(ranges).toEqual([]);
  });

  it('fails on an HTTP error', async () => {
    vi.stubGlobal('fetch', async () => new Response('', { status: 404 }));
    await expect(fetchAsBlob('nvault://v/7', 10)).rejects.toThrow('404');
  });
});
