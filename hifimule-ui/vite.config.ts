import { defineConfig } from "vite";
import { cpSync } from "node:fs";
import { resolve } from "node:path";
import { localeCatalog } from './localeCatalog.ts';

const host = process.env.TAURI_DEV_HOST;
const shoelaceAssets = resolve(import.meta.dirname, 'node_modules/@shoelace-style/shoelace/dist/assets');

// https://vite.dev/config/
export default defineConfig(async () => ({
  root: import.meta.dirname,
  plugins: [localeCatalog(resolve(import.meta.dirname, '../hifimule-i18n/catalog.json')), {
    name: 'copy-shoelace-assets',
    closeBundle() {
      cpSync(shoelaceAssets, resolve(import.meta.dirname, 'dist/shoelace/assets'), { recursive: true });
    },
  }],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
        protocol: "ws",
        host,
        port: 1421,
      }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  resolve: {
    alias: {
      '@hifimule/i18n-catalog': resolve(import.meta.dirname, '../hifimule-i18n/catalog.json'),
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target:
      process.env.TAURI_ENV_PLATFORM == 'windows'
        ? 'chrome105'
        : 'safari13',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rolldownOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        splashscreen: resolve(import.meta.dirname, 'splashscreen.html'),
      },
    },
  },
}));
