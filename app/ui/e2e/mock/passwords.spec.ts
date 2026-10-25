// The password manager screen against the mock (pmMock.ts): list, search, CRUD, reveal, copy
// with the clipboard countdown, live TOTP, generator, import, keyboard shortcuts.
import type { Page } from '@playwright/test';
import { boot, expect, expectScreen, test } from './helpers';

const entries = (page: Page) => page.getByTestId('pm-entry');
const titles = async (page: Page) => (await entries(page).evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.title)));
const clipboard = (page: Page) => page.evaluate(() => (window as unknown as { __nv: { mock: { pm: { clipboard: string | null } } } }).__nv.mock.pm.clipboard);

test('sidebar entry, list with folders and favourites, search as you type', async ({ page }) => {
  await boot(page, 'screen=vault');
  await page.getByTestId('nav-passwords').click();
  await expectScreen(page, 'passwords');
  await expect(entries(page)).toHaveCount(6);
  await page.locator('[data-filter="favorites"]').click();
  expect(await titles(page)).toEqual(['Banca del Borgo', 'Posta personale']);
  await page.locator('[data-folder="Personale"]').click();
  expect(await titles(page)).toEqual(['Banca del Borgo', 'Posta personale', 'SPID']);
  await page.locator('[data-filter="all"]').click();
  await page.getByTestId('pm-search').fill('example.it mario');
  await expect(entries(page)).toHaveCount(2);
  await page.getByTestId('pm-search').fill('');
  await expect(entries(page)).toHaveCount(6);
  // Ctrl+F focuses the search
  await page.getByTestId('pm-list').focus();
  await page.keyboard.press('Control+f');
  await expect(page.getByTestId('pm-search')).toBeFocused();
});

test('detail: reveal on request, copy with the clipboard countdown, live TOTP', async ({ page }) => {
  await boot(page, 'screen=passwords&entry=Posta personale');
  const d = page.getByTestId('pm-detail');
  await expect(d.getByTestId('pm-password')).toHaveAttribute('data-shown', 'false');
  await d.getByTestId('pm-reveal').click();
  await expect(d.getByTestId('pm-password')).toHaveText('Gufo-Lampada-Remo-81');
  await d.getByTestId('pm-copy-password').click();
  await expect(page.getByTestId('pm-copied')).toContainText('Gli appunti si svuotano tra');
  expect(await clipboard(page)).toBe('Gufo-Lampada-Remo-81');
  const code = d.getByTestId('totp-code');
  await expect(code).toHaveText(/^\d{3} \d{3}$/);
  await d.getByTestId('pm-copy-totp').click();
  expect(await clipboard(page)).toBe((await code.textContent())!.replace(' ', ''));
  const left = Number(await d.locator('[data-remaining]').getAttribute('data-remaining'));
  expect(left).toBeGreaterThan(0);
  expect(left).toBeLessThanOrEqual(30);
});

test('keyboard: arrows move, Ctrl+C / Ctrl+B / Ctrl+T copy password, username, 2FA', async ({ page }) => {
  await boot(page, 'screen=passwords&entry=Banca del Borgo');
  await page.getByTestId('pm-list').focus();
  await page.keyboard.press('Control+c');
  await expect.poll(() => clipboard(page)).toBe('x7#Lq9!vR2@pZ4');
  await page.keyboard.press('Control+b');
  await expect.poll(() => clipboard(page)).toBe('00412789');
  await page.keyboard.press('Control+t');
  await expect.poll(() => clipboard(page)).toMatch(/^\d{8}$/);
  await page.keyboard.press('ArrowDown');
  await expect(page.getByTestId('pm-detail')).toContainText('Film in streaming');
});

test('create, edit (password history, TOTP checked live), favourite, delete', async ({ page }) => {
  await boot(page, 'screen=passwords');
  await page.getByTestId('pm-new').click();
  const ed = page.getByTestId('pm-editor');
  await expect(page.getByTestId('pm-save')).toBeEnabled();
  await ed.locator('#pm-title').fill('');
  await page.getByTestId('pm-save').click();
  await expect(ed).toContainText('Serve un titolo.');
  await ed.locator('#pm-title').fill('Forum');
  await ed.locator('#pm-user').fill('mario');
  await ed.getByLabel('Password', { exact: true }).fill('prima-password-1');
  await ed.getByTestId('totp-input').fill('not base32!');
  await expect(ed.getByTestId('totp-check')).toHaveAttribute('data-state', 'error');
  await expect(page.getByTestId('pm-save')).toBeDisabled();
  await ed.getByTestId('totp-input').fill('JBSWY3DPEHPK3PXP');
  await expect(ed.getByTestId('totp-check')).toHaveAttribute('data-state', 'ok');
  await page.getByTestId('pm-save').click();
  await expect(ed).toHaveCount(0);
  await expect(page.getByTestId('pm-detail')).toContainText('Forum');
  await expect(entries(page)).toHaveCount(7);

  await page.getByTestId('pm-edit').click();
  await ed.getByLabel('Password', { exact: true }).fill('seconda-password-2');
  await page.getByTestId('pm-save').click();
  await expect(ed).toHaveCount(0);
  await page.getByTestId('pm-history').click();
  const hist = page.getByTestId('pm-history-list');
  await expect(hist.getByRole('listitem')).toHaveCount(1);
  await hist.getByRole('button', { name: 'Mostra' }).click();
  await expect(hist).toContainText('prima-password-1');
  await page.getByRole('dialog').getByRole('button', { name: 'Chiudi' }).click();

  await page.getByTestId('pm-favorite').click();
  await expect(page.getByTestId('pm-favorite')).toHaveAttribute('aria-pressed', 'true');
  await page.getByTestId('pm-delete').click();
  await page.getByTestId('pm-delete-confirm').click();
  await expect(entries(page)).toHaveCount(6);
  await expect(page.getByTestId('pm-detail')).toHaveCount(0);
});

test('generator: random and passphrase, entropy, use in the editor', async ({ page }) => {
  await boot(page, 'screen=passwords&dialog=pm-new');
  await page.getByTestId('pm-generate').click();
  const gen = page.getByTestId('pm-generator');
  await gen.getByTestId('gen-kind').getByRole('radio', { name: 'Casuale' }).click();
  await gen.getByTestId('gen-length').fill('32');
  await expect(gen.getByTestId('gen-value')).toHaveText(/^\S{32}$/);
  await expect(gen.getByTestId('gen-bits')).toContainText(/2\d\d bit/);
  await gen.getByTestId('gen-kind').getByRole('radio', { name: 'Frase' }).click();
  await expect(gen.getByTestId('gen-value')).toHaveText(/^\p{L}+(-\p{L}+\d?){2,}/u);
  await expect(gen.getByTestId('gen-bits')).toContainText(/\d+ bit/);
  const value = (await gen.getByTestId('gen-value').textContent())!.trim();
  await page.getByTestId('gen-use').click();
  await expect(page.getByTestId('pm-editor').getByLabel('Password', { exact: true })).toHaveValue(value);
});

test('import: CSV and KeePass (wrong then right master password) with progress and result', async ({ page }) => {
  await boot(page, 'screen=passwords');
  await page.getByTestId('pm-open-import').click();
  const imp = page.getByTestId('pm-import');
  await imp.locator('[data-kind="csv"]').click();
  await page.getByTestId('imp-next').click();
  await page.getByTestId('imp-choose').click();
  await page.getByTestId('imp-start').click();
  await expect(page.getByTestId('pm-import-result')).toHaveAttribute('data-imported', '1');
  await page.getByRole('dialog').getByRole('button', { name: 'Chiudi' }).click();
  await expect(entries(page)).toHaveCount(7);

  await page.getByTestId('pm-open-import').click();
  await imp.locator('[data-kind="keepass"]').click();
  await page.getByTestId('imp-next').click();
  await page.getByTestId('imp-choose').click();
  await imp.getByLabel('Password del database KeePass').fill('sbagliata');
  await page.getByTestId('imp-start').click();
  await expect(imp).toContainText('Importazione non riuscita: Password errata.');
  await page.getByRole('dialog').getByRole('button', { name: 'Chiudi' }).click();
  await page.getByTestId('pm-open-import').click();
  await imp.locator('[data-kind="keepass"]').click();
  await page.getByTestId('imp-next').click();
  await page.getByTestId('imp-choose').click();
  await imp.getByLabel('Password del database KeePass').fill('demo');
  await page.getByTestId('imp-start').click();
  await expect(page.getByTestId('pm-import-result')).toHaveAttribute('data-imported', '3');
  await page.getByRole('dialog').getByRole('button', { name: 'Chiudi' }).click();
  await expect(entries(page)).toHaveCount(10);
  await expect(page.locator('[data-folder="Casa/Rete"]')).toBeVisible();
});

test('English: the same screen, labels translated', async ({ page }) => {
  await boot(page, 'screen=passwords&lang=en&entry=SPID');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Passwords');
  await expect(page.getByTestId('pm-new')).toHaveText(/New entry/);
  await expect(page.getByTestId('pm-detail')).toContainText('Username');
});
