import { boot, expect, expectScreen, test } from './helpers';

test('installed: the three switches toggle and keep their state', async ({ page }) => {
  await boot(page, 'screen=settings&installed=1');
  const section = page.getByTestId('settings-startup');
  await expect(section.getByRole('heading', { name: 'Avvio' })).toBeVisible();
  const login = section.getByRole('switch', { name: 'Avvia all’accesso' });
  const minimized = section.getByRole('switch', { name: 'Avvia ridotto' });
  const tray = section.getByRole('switch', { name: 'Resta nel vassoio quando chiudi la finestra' });
  await expect(login).toHaveAttribute('aria-checked', 'false');
  await expect(minimized).toHaveAttribute('aria-checked', 'false');
  await expect(tray).toHaveAttribute('aria-checked', 'true');
  await expect(section.getByTestId('startup-portable')).toHaveCount(0);
  await expect(minimized).toHaveAccessibleDescription(/area di notifica/);
  await expect(tray).toHaveAccessibleDescription(/Esci dal menu dell’icona/);

  await login.click();
  await expect(login).toHaveAttribute('aria-checked', 'true');
  await minimized.click();
  await expect(minimized).toHaveAttribute('aria-checked', 'true');
  await tray.click();
  await expect(tray).toHaveAttribute('aria-checked', 'false');

  await page.getByTestId('settings-back').click();
  await expectScreen(page, 'welcome');
  await page.getByTestId('open-settings').click();
  await expectScreen(page, 'settings');
  await expect(login).toHaveAttribute('aria-checked', 'true');
  await expect(minimized).toHaveAttribute('aria-checked', 'true');
  await expect(tray).toHaveAttribute('aria-checked', 'false');

  await login.click();
  await expect(login).toHaveAttribute('aria-checked', 'false');
  await expect(minimized).toHaveAttribute('aria-checked', 'true');
});

test('the Avvio card comes right after the app look, before the language', async ({ page }) => {
  await boot(page, 'screen=settings&installed=1');
  const order = await page.locator('section.block').evaluateAll((s) => s.map((e) => e.getAttribute('data-testid')));
  expect(order.indexOf('settings-startup')).toBe(order.indexOf('settings-app') + 1);
  expect(order.indexOf('settings-startup')).toBeLessThan(order.indexOf('settings-language'));
});

test('portable: no switches, the reason instead', async ({ page }) => {
  await boot(page, 'screen=settings');
  const section = page.getByTestId('settings-startup');
  await expect(section.getByTestId('startup-portable')).toHaveText(/Da chiavetta .* non può avviarsi all’accesso/);
  await expect(section.getByTestId('start-at-login')).toHaveCount(0);
  await expect(section.getByTestId('start-minimized')).toHaveCount(0);
  await expect(section.getByTestId('close-to-tray')).toHaveCount(0);
});

test('english labels', async ({ page }) => {
  await boot(page, 'screen=settings&installed=1&lang=en');
  const section = page.getByTestId('settings-startup');
  await expect(section.getByRole('switch', { name: 'Start at login' })).toBeVisible();
  await expect(section.getByRole('switch', { name: 'Start minimised' })).toBeVisible();
  await expect(section.getByRole('switch', { name: 'Stay in the tray when the window is closed' })).toBeVisible();
});

test('interface size: every key the same height, letter and value on one baseline', async ({ page }) => {
  await boot(page, 'screen=settings&installed=1&lang=en');
  const keys = page.getByTestId('app-scale').getByRole('radio');
  await expect(keys).toHaveCount(5);
  const m = await keys.evaluateAll((ks) =>
    ks.map((k) => {
      const probe = document.createElement('span');
      probe.style.cssText = 'display:inline-block;width:0;height:0;vertical-align:baseline';
      k.appendChild(probe);
      const b = probe.getBoundingClientRect().bottom - k.getBoundingClientRect().top;
      probe.remove();
      return { h: k.getBoundingClientRect().height, top: k.getBoundingClientRect().top, base: b };
    }),
  );
  for (const k of m) {
    expect(k.h).toBeCloseTo(m[0].h, 1);
    expect(k.top).toBeCloseTo(m[0].top, 1);
    expect(Math.abs(k.base - m[0].base)).toBeLessThanOrEqual(1);
  }
});
