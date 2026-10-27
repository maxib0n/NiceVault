import { join } from 'node:path';
import type { Page } from '@playwright/test';
import { boot, expect, test } from './helpers';

const WINDOWS = [
  [900, 600],
  [1200, 780],
  [1680, 1050],
  [2560, 1440],
] as const;
const SCALES = [100, 125, 150] as const;
const SHOTS = process.env.NV_E2E_SHOTS;

interface Fault {
  passport: string;
  what: string;
}

async function faults(page: Page): Promise<Fault[]> {
  return page.evaluate(() => {
    const out: { passport: string; what: string }[] = [];
    const r = (el: Element) => el.getBoundingClientRect();
    const px = (n: number) => Math.round(n * 10) / 10;
    const books = [...document.querySelectorAll<HTMLElement>('[data-testid=passport]')];
    const boxes = books.map((p) => r(p));
    books.forEach((p, i) => {
      const name = p.dataset.name ?? `#${i}`;
      const cover = p.querySelector('.cover');
      if (!cover) return;
      const c = r(cover);
      const safe = { left: c.left + 20, right: c.right - 6, top: c.top + 6, bottom: c.bottom - 6 };
      for (const el of cover.querySelectorAll<HTMLElement>('.issuer, .name, .place')) {
        const b = r(el);
        if (!b.width) continue;
        const tag = el.classList[0];
        if (b.left < safe.left - 0.5 || b.right > safe.right + 0.5 || b.top < safe.top - 0.5 || b.bottom > safe.bottom + 0.5)
          out.push({ passport: name, what: `${tag} [${px(b.left)},${px(b.top)} → ${px(b.right)},${px(b.bottom)}] outside the stitching [${px(safe.left)},${px(safe.top)} → ${px(safe.right)},${px(safe.bottom)}]` });
        if (getComputedStyle(el).textOverflow !== 'ellipsis') {
          const range = document.createRange();
          range.selectNodeContents(el);
          for (const line of range.getClientRects()) {
            if (line.top >= b.bottom - 1) continue;
            if (line.left < b.left - 0.5 || line.right > b.right + 0.5)
              out.push({ passport: name, what: `${tag} line [${px(line.left)} → ${px(line.right)}] wider than its box [${px(b.left)} → ${px(b.right)}]` });
          }
        }
        const lh = parseFloat(getComputedStyle(el).lineHeight) || parseFloat(getComputedStyle(el).fontSize) * 1.2;
        const lines = Math.round((el.clientHeight - parseFloat(getComputedStyle(el).paddingTop) - parseFloat(getComputedStyle(el).paddingBottom)) / lh);
        if (lines > (tag === 'name' ? 2 : 1)) out.push({ passport: name, what: `${tag} on ${lines} lines` });
      }
      const mark = cover.querySelector('.mark');
      if (mark) {
        const b = r(mark);
        if (b.left < c.left + 6 || b.right > c.right - 6 || b.top < safe.top || b.bottom > safe.bottom)
          out.push({ passport: name, what: `the rosette [${px(b.left)},${px(b.top)} → ${px(b.right)},${px(b.bottom)}] outside the cover` });
      }
      const book = p.querySelector('.book');
      if (!book?.getAttribute('title')?.includes(name)) out.push({ passport: name, what: 'no full name in the title' });
      for (const el of p.querySelectorAll<HTMLElement>('.caption, .where')) {
        const b = r(el);
        if (b.width && (b.left < boxes[i].left - 0.5 || b.right > boxes[i].right + 0.5)) out.push({ passport: name, what: `${el.classList[0]} wider than the passport` });
        if (el.scrollWidth > el.clientWidth + 1) out.push({ passport: name, what: `${el.classList[0]} text overflows` });
      }
      for (let j = i + 1; j < boxes.length; j++) {
        const a = boxes[i];
        const b = boxes[j];
        if (a.left < b.right - 0.5 && b.left < a.right - 0.5 && a.top < b.bottom - 0.5 && b.top < a.bottom - 0.5)
          out.push({ passport: name, what: `overlaps ${books[j].dataset.name}` });
      }
    });
    const shelf = document.querySelector('.passports');
    const deck = shelf && getComputedStyle(shelf).flexWrap === 'nowrap';
    if (!deck && document.documentElement.scrollWidth > document.documentElement.clientWidth + 1) out.push({ passport: '-', what: 'the page scrolls sideways' });
    return out;
  });
}

async function check(page: Page, query: string, label: string) {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await boot(page, `screen=welcome&recents=long&${query}`);
  await expect(page.getByTestId('passport')).toHaveCount(7);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(150);
  const found = await faults(page);
  if (SHOTS) {
    await page.evaluate(() => document.querySelector<HTMLElement>('.welcome')?.style.setProperty('height', 'auto'));
    await page.screenshot({ path: join(SHOTS, `${label}.png`), fullPage: true });
  }
  expect(found, label).toEqual([]);
}

for (const lang of ['it', 'en'] as const) {
  for (const [w, h] of WINDOWS) {
    test(`passports fit: ${w}x${h}, ${lang}, 100/125/150 %`, async ({ page }) => {
      test.slow();
      for (const s of SCALES) {
        await page.setViewportSize({ width: Math.round(w / (s / 100)), height: Math.round(h / (s / 100)) });
        await check(page, `lang=${lang}`, `${w}x${h}-${s}-${lang}`);
      }
    });
  }
}

test('passports fit in every ink, dark and light', async ({ page }, info) => {
  test.slow();
  test.skip(info.project.name !== 'chromium', 'geometry only: one engine is enough for the inks');
  await page.setViewportSize({ width: 900, height: 600 });
  for (const theme of ['dark', 'light'] as const) {
    for (const ink of ['velluto', 'prugna', 'prussia', 'bottiglia', 'seppia', 'ardesia', 'ottanio']) {
      await check(page, `app_theme=${theme}&app_accent=${ink}&theme=${theme}&accent=${ink}`, `ink-${ink}-${theme}`);
    }
  }
});
