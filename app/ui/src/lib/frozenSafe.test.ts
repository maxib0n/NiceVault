import { spawnSync } from 'node:child_process';
import { describe, expect, it } from 'vitest';

const script = (useFilter: boolean) => `
Object.freeze(Object.prototype);
const { frozenSafe } = await import('./src/lib/frozenSafe.ts');
const { ZxcvbnFactory } = await import('@zxcvbn-ts/core');
const common = await import('@zxcvbn-ts/language-common');
const en = await import('@zxcvbn-ts/language-en');
const it = await import('@zxcvbn-ts/language-it');
const d = { ...common.dictionary, ...en.dictionary, ...it.dictionary };
const z = new ZxcvbnFactory({ dictionary: ${useFilter ? 'frozenSafe(d)' : 'd'}, graphs: common.adjacencyGraphs, translations: it.translations });
console.log(['correct horse battery staple', 'constructor', 'toString1', 'P@ssw0rd'].map((p) => z.check(p).score).join(','));
`;

const run = (useFilter: boolean) =>
  spawnSync(process.execPath, ['--experimental-strip-types', '--no-warnings', '--input-type=module', '-e', script(useFilter)], {
    cwd: new URL('../..', import.meta.url).pathname,
    encoding: 'utf8',
    timeout: 60_000,
  });

describe('zxcvbn under a frozen Object.prototype', () => {
  it('fails without the filter (the bug) and works with it', () => {
    const bad = run(false);
    expect(bad.status).not.toBe(0);
    expect(bad.stderr).toMatch(/read only property 'constructor'/);
    const good = run(true);
    expect(good.stderr).toBe('');
    expect(good.stdout.trim()).toMatch(/^4,\d,\d,0$/);
  }, 120_000);
});
