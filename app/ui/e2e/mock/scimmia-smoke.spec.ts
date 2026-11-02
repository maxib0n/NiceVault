import { test } from '@playwright/test';
import { boot, monkey } from '../scimmia/scimmia';

for (const [seed, query] of [
  [101, 'screen=vault'],
  [102, 'screen=welcome&app_theme=dark'],
] as const) {
  test(`la scimmia, short run: seed ${seed}`, async ({ page }, info) => {
    test.setTimeout(90_000);
    const w = await boot(page, query);
    await monkey(page, w, info, { seed, steps: 40 });
  });
}
