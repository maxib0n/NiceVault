import { chromium, webkit } from '@playwright/test';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const args = process.argv.slice(2);
const flag = (k) => args.find((a) => a.startsWith(`--${k}=`))?.split('=')[1];
const OUT = resolve(args.find((a) => !a.startsWith('--')) ?? join(HERE, '../test-results/scale-audit'));
const BASE = process.env.NV_AUDIT_URL ?? 'http://127.0.0.1:5472';
const engine = flag('browser') === 'chromium' ? chromium : webkit;
const engineName = flag('browser') === 'chromium' ? 'chromium' : 'webkit';
const only = flag('only');
const sizeOnly = flag('size');
const probe = readFileSync(join(HERE, '../e2e/mock/layout-probe.ts'), 'utf8').match(/\/\/ probe-begin\n([\s\S]*?)\/\/ probe-end/)[1];

export const SCREENS = {
  welcome: 'screen=welcome',
  'welcome-long': 'screen=welcome&recents=long',
  'welcome-empty': 'screen=welcome&empty',
  create1: 'screen=create&step=1',
  create2: 'screen=create&step=2',
  create3: 'screen=create&step=3',
  unlock: 'screen=unlock',
  'unlock-removed': 'screen=unlock&locked=removed',
  'unlock-error': 'screen=unlock&error=wrong-password',
  'unlock-busy': 'screen=unlock&unlocking',
  vault: 'screen=vault',
  'vault-list': 'screen=vault&view=list',
  'vault-menu': 'screen=vault&menu',
  'vault-select': 'screen=vault&select=3',
  'vault-filters': 'screen=vault&filters=kinds:photos,videos;size:large',
  'vault-jobs': 'screen=vault&jobs',
  'vault-delete': 'screen=vault&dialog=delete&select=2',
  'vault-rename': 'screen=vault&dialog=rename',
  'ql-photo': 'screen=vault&dir=Foto&preview=image',
  'ql-pdf': 'screen=vault&preview=pdf',
  'ql-text': 'screen=vault&preview=text',
  details: 'screen=details',
  'details-addpw': 'screen=details&dialog=add-password',
  'details-emblem': 'screen=details&dialog=emblem',
  passwords: 'screen=passwords',
  'pm-edit': 'screen=passwords&dialog=pm-edit',
  'pm-generator': 'screen=passwords&dialog=pm-generator',
  'pm-import': 'screen=passwords&dialog=pm-import',
  settings: 'screen=settings',
  'settings-licences': 'screen=settings&dialog=licences',
  device: 'screen=device',
};

const win = (w, h, z) => [`${w}x${h}${z === 100 ? '' : '@' + z}`, Math.round((w * 100) / z), Math.round((h * 100) / z), z / 100];
export const SIZES = [
  ...[100, 110, 120, 130].flatMap((z) => [win(1280, 800, z), win(900, 600, z)]),
  win(1200, 780, 150),
  ['phone', 390, 844, 3],
];

const browser = await engine.launch();
mkdirSync(OUT, { recursive: true });
const report = [];
for (const [sname, w, h, dpr] of SIZES) {
  if (sizeOnly && sname !== sizeOnly) continue;
  for (const theme of ['dark', 'light']) {
    const phone = sname === 'phone';
    const ctx = await browser.newContext({ reducedMotion: "reduce", viewport: { width: w, height: h }, deviceScaleFactor: dpr, locale: 'it-IT', hasTouch: phone, isMobile: phone && engine === chromium });
    const page = await ctx.newPage();
    for (const [name, q] of Object.entries(SCREENS)) {
      if (only && !name.startsWith(only)) continue;
      const url = `${BASE}/?${q}&theme=${theme}${phone ? '&platform=mobile' : '&chrome=linux'}`;
      try {
        await page.goto(url);
        await page.locator('[data-ready="true"]').waitFor({ timeout: 15000 });
        await page.waitForTimeout(1600);
        const found = await page.evaluate(`(() => { ${probe}; return probeLayout(); })()`);
        const file = `${name}--${sname}--${theme}--${engineName}.png`;
        await page.screenshot({ path: join(OUT, file) });
        if (found.length) report.push({ shot: file, found });
        console.log(`${found.length ? 'FLAG' : 'ok  '} ${file}${found.length ? ' ' + found.slice(0, 3).map((f) => f.kind + ':' + f.a + (f.b ? ' x ' + f.b : '')).join(' | ') : ''}`);
      } catch (e) {
        console.log(`ERR  ${name}--${sname}--${theme}: ${e.message.split('\n')[0]}`);
      }
    }
    await ctx.close();
  }
}
await browser.close();
writeFileSync(join(OUT, `report-${engineName}${sizeOnly ? '-' + sizeOnly : ''}.json`), JSON.stringify(report, null, 1));
console.log(`${report.length} shots flagged; ${join(OUT, 'report.json')}`);
