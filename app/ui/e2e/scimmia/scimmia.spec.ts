import { test } from '@playwright/test';
import { boot, monkey, STARTS } from './scimmia';

const seeds = (process.env.SCIMMIA_SEEDS ?? '1,2,3,4,5,6,7,8').split(',').map(Number);
const steps = Number(process.env.SCIMMIA_STEPS ?? 250);

for (const seed of seeds) {
  test(`seed ${seed}`, async ({ page }, info) => {
    const phone = info.project.name.includes('phone');
    const n = info.project.name.startsWith('webkit') ? Math.min(steps, 80) : steps;
    test.setTimeout(n * 3000 + 60_000);
    const start = STARTS[(seed - 1) % STARTS.length];
    const extra = (info.project.metadata as { query?: string }).query ?? '';
    const w = await boot(page, `${start}${phone ? '&platform=mobile' : ''}${extra}`);
    await monkey(page, w, info, { seed, steps: n, phone });
  });
}
