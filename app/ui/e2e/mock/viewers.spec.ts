import { test, expect, boot } from './helpers';
import type { Page } from '@playwright/test';

const open = async (page: Page, dir: string, name: string, extra = '') => {
  await boot(page, `screen=vault&dir=${encodeURIComponent(dir)}&preview=${encodeURIComponent(name.toLowerCase())}${extra}`);
  const ql = page.getByRole('dialog', { name: `Anteprima di ${name}` });
  await expect(ql).toBeVisible();
  return ql;
};

test.describe('in-app viewers', () => {
  test('code: highlighted, numbered, encoding and language in the bar', async ({ page }) => {
    const ql = await open(page, 'Personale/Anteprime', 'main.rs');
    const v = ql.getByTestId('viewer-text');
    await expect(v).toHaveAttribute('data-lang', 'rust');
    const code = ql.getByTestId('viewer-code');
    await expect(code).toContainText('fn top_words');
    await expect(code.locator('.t-kw').first()).toHaveText('use');
    await expect(ql.getByTestId('viewer-encoding')).toHaveText('UTF-8');
    await expect(ql.getByRole('toolbar', { name: 'Strumenti testo' })).toContainText('Rust');
    const wrap = ql.getByRole('button', { name: 'A capo automatico' });
    await expect(wrap).toHaveAttribute('aria-pressed', 'true');
    await wrap.click();
    await expect(wrap).toHaveAttribute('aria-pressed', 'false');
  });

  test('encodings: Windows-1252 and UTF-16 decoded right', async ({ page }) => {
    let ql = await open(page, 'Personale/Anteprime', 'Lettera (Windows-1252).txt');
    await expect(ql.getByTestId('viewer-encoding')).toHaveText('Windows-1252');
    await expect(ql.getByTestId('viewer-code')).toContainText('Città di Castello: qui è già autunno, e il caffè è più buono.');
    ql = await open(page, 'Personale/Anteprime', 'Lettera (UTF-16).txt');
    await expect(ql.getByTestId('viewer-encoding')).toHaveText('UTF-16 LE');
    await expect(ql.getByTestId('viewer-code')).toContainText('zia Lucìa');
  });

  test('a big log is read in pages', async ({ page }) => {
    const ql = await open(page, 'Personale/Anteprime', 'server.log');
    const v = ql.getByTestId('viewer-text');
    await expect(v).toHaveAttribute('data-pages', '6');
    await expect(ql.getByTestId('viewer-text-page')).toHaveText('Parte 1 di 6');
    await expect(ql.getByTestId('viewer-code')).toContainText('riga 1: ');
    await ql.getByRole('button', { name: 'Ultima pagina' }).click();
    await expect(ql.getByTestId('viewer-text-page')).toHaveText('Parte 6 di 6');
    await expect(ql.getByTestId('viewer-code')).toContainText('riga 16000: ');
    const first = await ql.getByTestId('viewer-code').locator('div').first().textContent();
    expect(first).toMatch(/^2024-\d\d-\d\dT/);
  });

  test('CSV as a table, and as text', async ({ page }) => {
    const ql = await open(page, 'Personale/Anteprime', 'Spese maggio.csv');
    const t = ql.getByTestId('viewer-csv');
    await expect(t.getByRole('columnheader')).toHaveText(['Data', 'Descrizione', 'Categoria', 'Importo']);
    await expect(t.getByRole('row').nth(4)).toContainText('Cena da "Gigi"');
    await ql.getByRole('button', { name: 'Mostra come tabella' }).click();
    await expect(ql.getByTestId('viewer-code')).toContainText('2024-05-02;Bolletta luce');
  });

  test('Markdown: typeset; raw HTML is text; images not fetched; links ask, then open outside', async ({ page }) => {
    const remote: string[] = [];
    page.on('request', (r) => {
      if (!r.url().startsWith('http://127.0.0.1') && !r.url().startsWith('blob:') && !r.url().startsWith('data:')) remote.push(r.url());
    });
    page.on('dialog', () => {
      throw new Error('a script ran');
    });
    const ql = await open(page, 'Personale/Anteprime', 'Guida al volume.md');
    const md = ql.getByTestId('viewer-markdown');
    await expect(md.getByRole('heading', { level: 1 })).toHaveText('Guida al volume');
    await expect(md.getByRole('table')).toBeVisible();
    await expect(md.locator('script')).toHaveCount(0);
    await expect(md.locator('b')).toHaveCount(0);
    await expect(md).toContainText('<script>alert("questo non deve girare")</script>');
    await expect(md.locator('img')).toHaveCount(0);
    await expect(md).toContainText('Una foto remota');
    await expect(md.getByRole('link', { name: 'link pericoloso' })).toHaveCount(0);
    await md.getByRole('link', { name: 'la pagina di aiuto' }).click();
    const ask = ql.getByTestId('link-confirm');
    await expect(ask).toContainText('https://example.org/aiuto');
    await ask.getByRole('button', { name: 'Annulla' }).click();
    await expect(ask).toHaveCount(0);
    await md.getByRole('link', { name: 'la pagina di aiuto' }).click();
    await ql.getByRole('button', { name: 'Apri nel browser' }).click();
    await expect.poll(() => page.evaluate(() => (window as unknown as { __nv: { mock: { viewers: { opened: { links: string[] } } } } }).__nv.mock.viewers.opened.links)).toEqual(['https://example.org/aiuto']);
    expect(remote).toEqual([]);
    await ql.getByRole('button', { name: 'Sorgente' }).click();
    await expect(ql.getByTestId('viewer-code')).toContainText('# Guida al volume');
    await ql.getByRole('button', { name: 'Impaginato' }).click();
    await expect(md.getByRole('heading', { level: 1 })).toBeVisible();
  });

  test('Markdown, every feature: front matter, outline, tables, tasks, footnotes, math, diagram, a picture of the volume', async ({ page }) => {
    const remote: string[] = [];
    page.on('request', (r) => {
      if (!r.url().startsWith('http://127.0.0.1') && !r.url().startsWith('blob:') && !r.url().startsWith('data:')) remote.push(r.url());
    });
    await page.addInitScript(() => Object.freeze(Object.prototype));
    const ql = await open(page, 'Personale/Anteprime', 'Taccuino di casa.md');
    const md = ql.getByTestId('viewer-markdown');
    await expect(md.getByRole('heading', { level: 1 })).toHaveText('Taccuino di casa');
    await expect(ql.getByTestId('md-front')).toContainText('Anna Rossi');
    await expect(md.getByText('author: Anna Rossi')).toHaveCount(0);
    const toc = ql.getByTestId('md-toc');
    await expect(toc.getByRole('link')).toHaveCount(8);
    await toc.getByRole('link', { name: 'Glossario' }).click();
    await expect(md.getByRole('heading', { name: 'Glossario' })).toBeInViewport();
    await expect(toc.getByRole('link', { name: 'Glossario' })).toHaveAttribute('aria-current', 'location');
    await expect(md.getByRole('table')).toContainText('813,50 €');
    await expect(md.getByRole('img', { name: 'Fatto' })).toHaveCount(2);
    await expect(md.getByRole('img', { name: 'Da fare' })).toHaveCount(2);
    await expect(md.getByRole('term').filter({ hasText: 'Chiave di riserva' })).toHaveCount(1);
    await expect(md.locator('mark')).toHaveText('promemoria evidenziati');
    await expect(md.locator('abbr[title="Central Processing Unit"]')).toHaveText('CPU');
    await expect(md).toContainText('🎉');
    await expect(md).toContainText('Suggerimento');
    await md.getByRole('link', { name: '[2]' }).click();
    await expect(md.getByText('Anche con formattazione')).toBeInViewport();
    await md.getByRole('link', { name: 'spese', exact: true }).click();
    await expect(md.getByRole('heading', { name: 'Spese del trimestre' })).toBeInViewport();
    await expect(md).toHaveAttribute('data-math', 'ready');
    await expect(md.locator('math').first()).toBeAttached();
    await expect(md).toHaveAttribute('data-mermaid', 'ready');
    const diagram = md.locator('svg[aria-roledescription]');
    await expect(diagram).toHaveCount(1);
    await expect(diagram).toContainText('Copia cifrata');
    const fill = () => diagram.locator('.node rect, .node polygon').first().evaluate((e) => getComputedStyle(e).fill);
    const before = await fill();
    await page.evaluate(() => (document.documentElement.dataset.accent = document.documentElement.dataset.accent === 'prussia' ? 'bottiglia' : 'prussia'));
    await expect.poll(fill).not.toBe(before);
    await expect(md).toHaveAttribute('data-pictures', '1');
    await expect(md.getByRole('img', { name: "Il lago d'estate" })).toBeVisible();
    await expect(md).toContainText('Logo remoto');
    expect(remote).toEqual([]);
  });

  test('Markdown, a hostile file: nothing runs, loads or navigates', async ({ page }) => {
    const remote: string[] = [];
    page.on('request', (r) => {
      if (!r.url().startsWith('http://127.0.0.1') && !r.url().startsWith('blob:') && !r.url().startsWith('data:')) remote.push(r.url());
    });
    page.on('dialog', () => {
      throw new Error('a script ran');
    });
    await page.addInitScript(() => Object.freeze(Object.prototype));
    const ql = await open(page, 'Personale/Anteprime', 'Nota sospetta.md');
    const url = page.url();
    const md = ql.getByTestId('viewer-markdown');
    await expect(md.getByRole('heading', { level: 1 })).toHaveText('Nota sospetta');
    await expect(md).toHaveAttribute('data-mermaid', /ready|error/);
    for (const tag of ['script', 'iframe', 'img', 'form', 'input', 'object', 'embed', 'style:not(svg style)', 'foreignObject'])
      await expect(md.locator(tag), tag).toHaveCount(0);
    await expect(md).toContainText('<script>alert("questo non deve girare")</script>');
    await expect(md).toContainText('<img src=x onerror=alert(1)>');
    for (const name of ['link pericoloso', 'vbscript', 'file', 'dati', 'credenziali', 'riferimento']) await expect(md.getByRole('link', { name, exact: true })).toHaveCount(0);
    const hrefs = await md.locator('a[href]').evaluateAll((as) => as.map((a) => a.getAttribute('href')));
    for (const h of hrefs) expect(h).toMatch(/^(https?:\/\/|mailto:|#)/);
    const handlers = await md.locator('*').evaluateAll((els) => els.flatMap((e) => e.getAttributeNames().filter((n) => n.startsWith('on'))));
    expect(handlers).toEqual([]);
    await md.getByRole('link', { name: 'mailto con allegato' }).click();
    await expect(ql.getByTestId('link-confirm')).toContainText('mailto:a@b.it');
    await ql.getByRole('button', { name: 'Annulla' }).click();
    await md.getByRole('link', { name: 'https://evil.example/' }).first().click();
    await expect(ql.getByTestId('link-confirm')).toContainText('https://evil.example/');
    await page.keyboard.press('Escape');
    expect(page.url()).toBe(url);
    expect(remote).toEqual([]);
  });

  test('archives: zip and tar listed; tar.gz facts only', async ({ page }) => {
    let ql = await open(page, 'Personale/Anteprime', 'Vecchio PC.zip');
    let a = ql.getByTestId('viewer-archive');
    await expect(a).toHaveAttribute('data-entries', '5');
    await expect(ql.getByTestId('viewer-archive-summary')).toContainText('5 file, 3 cartelle');
    await expect(a.getByRole('row').filter({ hasText: 'maggio.txt' })).toContainText('160 B');
    await a.getByRole('searchbox', { name: 'Filtra per nome' }).fill('accent');
    await expect(a.getByRole('row')).toHaveCount(2);
    ql = await open(page, 'Personale/Anteprime', 'Backup contatti.tar');
    a = ql.getByTestId('viewer-archive');
    await expect(a).toHaveAttribute('data-entries', '3');
    await expect(a).toContainText('contatti.vcf');
    ql = await open(page, 'Personale/Anteprime', 'Backup contatti.tar.gz');
    await expect(ql.getByTestId('viewer-archive')).toContainText('Archivio gzip');
  });

  test('office: docx, xlsx, pptx, odt as text', async ({ page }) => {
    let ql = await open(page, 'Personale/Anteprime', 'Verbale riunione.docx');
    let o = ql.getByTestId('viewer-office');
    await expect(o).toHaveAttribute('data-kind', 'text');
    await expect(o.getByRole('heading', { name: 'Decisioni' })).toBeVisible();
    await expect(o).toContainText('Si approva il preventivo per il tetto');
    await expect(o).toContainText('Anna Rossi');
    ql = await open(page, 'Personale/Anteprime', 'Bilancio casa.xlsx');
    o = ql.getByTestId('viewer-office');
    await expect(o.getByTestId('viewer-sheet').getByRole('row').nth(4)).toContainText('Marzo');
    await expect(o).toContainText('Primo trimestre');
    ql = await open(page, 'Personale/Anteprime', 'Risultati Q3.pptx');
    await expect(ql.getByTestId('viewer-slide')).toHaveCount(3);
    await expect(ql.getByTestId('viewer-slide').last()).toContainText('Domande?');
    ql = await open(page, 'Personale/Anteprime', 'Lettera condominio.odt');
    await expect(ql.getByTestId('viewer-office')).toContainText("segnalare il guasto all'ascensore");
  });

  test('unknown bytes: info + hex; a name without extension that holds text is text', async ({ page }) => {
    let ql = await open(page, 'Personale/Anteprime', 'dati.bin');
    const info = ql.getByTestId('viewer-info');
    await expect(info).toHaveAttribute('data-reason', 'none');
    await expect(ql.getByTestId('viewer-hex')).toContainText('00000000');
    await expect(ql.getByTestId('viewer-hex').getByRole('row')).toHaveCount(188);
    await expect(ql.getByRole('button', { name: 'Apri con app esterna' })).toHaveCount(0);
    ql = await open(page, 'Personale/Anteprime', 'LEGGIMI');
    await expect(ql.getByTestId('viewer-text')).toBeVisible();
    await expect(ql.getByTestId('viewer-code')).toContainText('"lingua": "it"');
  });

  test('too big and undecodable images say so', async ({ page }) => {
    let ql = await open(page, 'Personale/Anteprime', 'Panorama enorme.tif');
    await expect(ql.getByTestId('viewer-info')).toHaveAttribute('data-reason', 'too_big');
    await expect(ql.getByTestId('viewer-state')).toHaveText("Troppo grande per l'anteprima");
    await expect(ql.getByTestId('viewer-info')).toContainText('fino a 200 MB');
    ql = await open(page, 'Personale/Anteprime', 'Scatto.heic');
    await expect(ql.getByTestId('viewer-info')).toHaveAttribute('data-reason', 'unsupported');
    await expect(ql.getByTestId('viewer-info')).toContainText('HEIF');
  });

  test('image: zoom buttons and keys, fit, EXIF orientation', async ({ page }) => {
    const ql = await open(page, 'Personale/Anteprime', 'Foto ruotata.jpg');
    const img = ql.getByRole('img', { name: 'Foto ruotata.jpg' });
    await expect(img).toBeVisible();
    await expect.poll(async () => img.evaluate((i: HTMLImageElement) => i.clientHeight > i.clientWidth)).toBe(true);
    const v = ql.getByTestId('viewer-image');
    const zoom = ql.getByTestId('viewer-zoom');
    const before = Number((await zoom.textContent())!.replace('%', ''));
    await ql.getByRole('button', { name: 'Ingrandisci' }).click();
    await expect.poll(async () => Number((await zoom.textContent())!.replace('%', ''))).toBeGreaterThan(before);
    await page.keyboard.press('0');
    await expect(zoom).toHaveText(`${before}%`);
    await page.keyboard.press('+');
    await expect(ql.getByRole('button', { name: 'Adatta alla finestra' })).toHaveAttribute('aria-pressed', 'false');
    await ql.getByRole('button', { name: 'Adatta alla finestra' }).click();
    await expect(v).toHaveAttribute('data-zoom', String(before));
  });

  test('PDF: pages, page field, zoom', async ({ page }) => {
    const ql = await open(page, 'Lavoro', 'Contratto affitto.pdf');
    const v = ql.getByTestId('viewer-pdf');
    await expect(v).toHaveAttribute('data-status', 'ready');
    await expect(v).toHaveAttribute('data-pages', '2');
    await expect(ql.getByRole('img', { name: 'Pagina 1' })).toBeVisible();
    await expect(ql.getByRole('img', { name: 'Pagina 1' }).locator('canvas[data-drawn]')).toHaveCount(1);
    await ql.getByRole('button', { name: 'Pagina successiva' }).click();
    await expect(v).toHaveAttribute('data-page', '2');
    await expect(ql.getByRole('textbox', { name: 'Vai alla pagina' })).toHaveValue('2');
    await ql.getByRole('textbox', { name: 'Vai alla pagina' }).fill('1');
    await ql.getByRole('textbox', { name: 'Vai alla pagina' }).press('Enter');
    await expect(v).toHaveAttribute('data-page', '1');
    const w = await ql.getByRole('img', { name: 'Pagina 1' }).evaluate((e) => e.clientWidth);
    await ql.getByRole('button', { name: 'Ingrandisci' }).click();
    await expect(v).toHaveAttribute('data-zoom', '125');
    await expect.poll(() => ql.getByRole('img', { name: 'Pagina 1' }).evaluate((e) => e.clientWidth)).toBeGreaterThan(w);
  });

  test('video: plays from its URL and seeks', async ({ page }) => {
    const ql = await open(page, 'Video', 'Compleanno Giulia.webm');
    const v = ql.locator('video');
    await expect.poll(() => v.evaluate((e: HTMLVideoElement) => e.readyState)).toBeGreaterThanOrEqual(1);
    await v.evaluate((e: HTMLVideoElement) => (e.currentTime = 2.5));
    await expect.poll(() => v.evaluate((e: HTMLVideoElement) => Math.round(e.currentTime * 10) / 10)).toBe(2.5);
    await expect(ql.getByTestId('viewer-media')).toHaveAttribute('data-via', 'scheme');
  });

  test('video full screen: ←/→ seek by default, step to the next file with the "step" setting', async ({ page }) => {
    for (const mode of ['seek', 'step']) {
      const ql = await open(page, 'Video', 'Compleanno Giulia.webm', `&video_arrows=${mode}`);
      const v = ql.locator('video');
      await expect.poll(() => v.evaluate((e: HTMLVideoElement) => e.readyState)).toBeGreaterThanOrEqual(1);
      await ql.getByTestId('mc-fullscreen').click();
      await expect(page.locator('dialog[data-full="true"]')).toHaveCount(1);
      await v.evaluate((e: HTMLVideoElement) => (e.currentTime = 0));
      await page.keyboard.press('ArrowRight');
      if (mode === 'seek') {
        await expect.poll(() => v.evaluate((e: HTMLVideoElement) => e.currentTime)).toBeGreaterThan(0.5);
        await expect(page.getByRole('dialog', { name: 'Anteprima di Compleanno Giulia.webm' })).toBeVisible();
      } else {
        await expect(page.getByRole('dialog', { name: 'Anteprima di Compleanno Giulia.webm' })).toHaveCount(0);
        await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toBeVisible();
      }
    }
  });

  test('Impostazioni: the arrow-keys choice defaults to seek and switches', async ({ page }) => {
    await boot(page, 'screen=settings');
    const g = page.getByTestId('video-arrows');
    await expect(g.getByRole('radio', { name: 'Avanti e indietro' })).toHaveAttribute('aria-checked', 'true');
    await g.getByRole('radio', { name: 'File precedente e successivo' }).click();
    await expect(g.getByRole('radio', { name: 'File precedente e successivo' })).toHaveAttribute('aria-checked', 'true');
  });

  test('video: speed menu, own full screen (controls stay, Esc leaves), never the native player', async ({ page }) => {
    const ql = await open(page, 'Video', 'Compleanno Giulia.webm');
    const v = ql.locator('video');
    await expect.poll(() => v.evaluate((e: HTMLVideoElement) => e.readyState)).toBeGreaterThanOrEqual(1);
    await ql.getByTestId('mc-options').click();
    await ql.getByRole('radio', { name: '1.5×' }).click();
    expect(await v.evaluate((e: HTMLVideoElement) => e.playbackRate)).toBe(1.5);
    await ql.getByTestId('mc-options').click();
    await expect(ql.getByTestId('mc-menu')).toHaveCount(0);
    await ql.getByTestId('mc-fullscreen').click();
    await expect(page.locator('dialog[data-full="true"]')).toHaveCount(1);
    expect(await page.evaluate(() => document.fullscreenElement)).toBeNull();
    await expect(ql.locator('.media-controls')).toBeVisible();
    expect(await v.evaluate((e: HTMLVideoElement) => e.playbackRate)).toBe(1.5);
    await page.keyboard.press('Escape');
    await expect(page.locator('dialog[data-full="true"]')).toHaveCount(0);
    await expect(ql).toBeVisible();
  });

  test('mounted: "Apri con app esterna" opens the file on the drive', async ({ page }) => {
    const ql = await open(page, 'Personale/Anteprime', 'Verbale riunione.docx', '&mount=mounted');
    await ql.getByRole('button', { name: 'Apri con app esterna' }).first().click();
    await expect
      .poll(() => page.evaluate(() => (window as unknown as { __nv: { mock: { viewers: { opened: { mounted: string[] } } } } }).__nv.mock.viewers.opened.mounted))
      .toEqual(['/run/user/1000/nicevault/Documenti/Personale/Anteprime/Verbale riunione.docx']);
  });

  test('←/→ step through every file, Esc closes', async ({ page }) => {
    const ql = await open(page, 'Personale/Anteprime', 'main.rs');
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('dialog', { name: 'Anteprima di Nota sospetta.md' })).toBeVisible();
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowLeft');
    await expect(page.getByRole('dialog', { name: 'Anteprima di Lettera condominio.odt' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(ql).toHaveCount(0);
    await expect(page.getByRole('dialog', { name: /^Anteprima di/ })).toHaveCount(0);
  });
});
