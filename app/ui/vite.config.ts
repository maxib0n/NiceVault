import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { paraglideVitePlugin } from '@inlang/paraglide-js';
import { rmSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Plugin } from 'vite';

// public/mock/ holds sample files for the in-browser mock only; keep them out of
// the shipped bundle unless NV_KEEP_MOCK=1 (e.g. to `vite preview` the mock).
const dropMockAssets = (): Plugin => ({
  name: 'nv-drop-mock-assets',
  apply: 'build',
  closeBundle() {
    if (process.env.NV_KEEP_MOCK !== '1') rmSync(resolve(__dirname, 'dist/mock'), { recursive: true, force: true });
  },
});

// Tauri expects a fixed port and no clearing of its CLI output.
export default defineConfig({
  plugins: [
    // i18n: messages/{it,en}.json -> src/paraglide (generated, git-ignored). The locale is the
    // UI's own (lib/i18n.svelte.ts overwrites getLocale), so the strategy is only a fallback.
    paraglideVitePlugin({ project: './project.inlang', outdir: './src/paraglide', strategy: ['baseLocale'] }),
    svelte(),
    dropMockAssets(),
  ],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    outDir: 'dist',
    target: 'es2022',
    emptyOutDir: true,
    // zxcvbn dictionaries and pdf.js are large but lazy-loaded on demand
    chunkSizeWarningLimit: 1500,
  },
  test: {
    include: ['src/**/*.test.ts'],
    setupFiles: ['src/test-setup.ts'],
    environment: 'node',
  },
});
