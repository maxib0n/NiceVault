import { expect, test } from '@playwright/test';
import { boot, STARTS } from './scimmia';

const where = (page: import('@playwright/test').Page) =>
  page.evaluate(() => {
    const a = document.activeElement as HTMLElement | null;
    if (!a || a === document.body || a === document.documentElement) return { at: 'BODY', visible: true, edge: false };
    const all = [...document.querySelectorAll<HTMLElement>('a[href], button, input, select, textarea, [tabindex]')].filter(
      (e) => e.tabIndex >= 0 && !e.matches(':disabled') && e.getClientRects().length && !e.closest('[inert]'),
    );
    const edge = a === all[0] || a === all.at(-1);
    const r = a.getBoundingClientRect();
    const s = getComputedStyle(a);
    const visible = r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && !a.closest('[aria-hidden="true"], [inert]');
    const name = (a.getAttribute('aria-label') ?? a.innerText ?? '').trim().replace(/\s+/g, ' ').slice(0, 30);
    return { at: `${a.tagName.toLowerCase()}${a.dataset.testid ? '#' + a.dataset.testid : ''} «${name}»`, visible, edge };
  });

for (const start of [...STARTS, 'screen=settings', 'screen=unlock']) {
  test(`Tab never loses the focus: ${start}`, async ({ page }, info) => {
    const phone = info.project.name.includes('phone');
    const w = await boot(page, `${start}${phone ? '&platform=mobile' : ''}`);
    const lost: string[] = [];
    let prev = 'start';
    let prevEdge = true;
    for (let i = 0; i < 90; i++) {
      await page.keyboard.press(i % 9 === 8 ? 'Shift+Tab' : 'Tab');
      const now = await where(page);
      if (now.at === 'BODY' && !prevEdge) lost.push(`body after ${prev}`);
      else if (!now.visible) lost.push(`invisible ${now.at} after ${prev}`);
      prev = now.at;
      prevEdge = now.at === 'BODY' || now.edge;
    }
    expect(lost, lost.join('\n')).toEqual([]);
    expect(w.errors, w.errors.join('\n')).toEqual([]);
  });
}
