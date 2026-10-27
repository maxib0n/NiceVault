import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { paraglideVitePlugin } from '@inlang/paraglide-js';
import { rmSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Plugin } from 'vite';

const dropMockAssets = (): Plugin => ({
  name: 'nv-drop-mock-assets',
  apply: 'build',
  closeBundle() {
    if (process.env.NV_KEEP_MOCK !== '1') rmSync(resolve(__dirname, 'dist/mock'), { recursive: true, force: true });
  },
});

const katexWoff2Only = (): Plugin => ({
  name: 'nv-katex-woff2-only',
  enforce: 'pre',
  transform(code, id) {
    if (!/katex[\\/]dist[\\/]katex(\.min)?\.css/.test(id)) return null;
    return code.replace(/,\s*url\([^)]+\.woff\)\s*format\(["']woff["']\)/g, '').replace(/,\s*url\([^)]+\.ttf\)\s*format\(["']truetype["']\)/g, '');
  },
});

const OVERRIDE_MISTAKE =
  /[\\/]node_modules[\\/]\.pnpm[\\/](mermaid|@mermaid-js\+[^@]+|d3(-[a-z-]+)?|dagre-d3-es|dayjs|cytoscape[a-z-]*|cose-base|layout-base|khroma|lodash-es|chevrotain|@chevrotain\+[^@]+|roughjs|@upsetjs\+[^@]+|elkjs|stylis|marked|es-toolkit|@iconify\+[^@]+|delaunator|internmap|robust-predicates|points-on-[a-z]+|path-data-parser|hachure-fill|ts-dedent)@/;
const PROTO_KEYS = 'constructor|valueOf|toString|toLocaleString|hasOwnProperty|isPrototypeOf|propertyIsEnumerable';
const ASSIGN = new RegExp(`(^|[^\\w$.])([A-Za-z_$][\\w$]*(?:\\.[A-Za-z_$][\\w$]*)*)\\.(${PROTO_KEYS})\\s*=(?!=)`, 'g');
const SAFE_ASSIGN =
  'var __nvAssign=function(t){for(var i=1;i<arguments.length;i++){var s=arguments[i];if(s==null)continue;s=Object(s);' +
  'var ks=Object.keys(s).concat(Object.getOwnPropertySymbols(s).filter(function(y){return Object.prototype.propertyIsEnumerable.call(s,y)}));' +
  'for(var j=0;j<ks.length;j++){var k=ks[j];if(k in Object.prototype&&!Object.prototype.hasOwnProperty.call(t,k))' +
  'Object.defineProperty(t,k,{value:s[k],writable:!0,configurable:!0,enumerable:!0});else t[k]=s[k]}}return t};\n';
const frozenPrototypeSafe = (): Plugin => ({
  name: 'nv-frozen-prototype-safe',
  transform(code, id) {
    if (!OVERRIDE_MISTAKE.test(id)) return null;
    let out = code.replace(ASSIGN, (_, pre, obj, key) => `${pre}${obj}[(Object.defineProperty(${obj},"${key}",{value:void 0,writable:!0,configurable:!0}),"${key}")]=`);
    if (/\bObject\.assign\b/.test(out)) out = SAFE_ASSIGN + out.replace(/\bObject\.assign\b/g, '__nvAssign');
    return out;
  },
});

export default defineConfig({
  plugins: [
    paraglideVitePlugin({ project: './project.inlang', outdir: './src/paraglide', strategy: ['baseLocale'] }),
    svelte(),
    dropMockAssets(),
    katexWoff2Only(),
    frozenPrototypeSafe(),
  ],
  optimizeDeps: { rolldownOptions: { plugins: [frozenPrototypeSafe()] } },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    outDir: 'dist',
    target: 'es2022',
    emptyOutDir: true,
    chunkSizeWarningLimit: 1500,
    assetsInlineLimit: (file: string) => (/\.(woff2?|ttf|otf)$/i.test(file) ? false : undefined),
  },
  test: {
    include: ['src/**/*.test.ts'],
    setupFiles: ['src/test-setup.ts'],
    environment: 'node',
  },
});
