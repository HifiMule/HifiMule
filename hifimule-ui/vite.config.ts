import { defineConfig } from "vite";
import { cpSync } from "fs";
import { resolve } from "path";

const host = process.env.TAURI_DEV_HOST;
const shoelaceAssets = resolve(__dirname, 'node_modules/@shoelace-style/shoelace/dist/assets');

// https://vite.dev/config/
export default defineConfig(async () => ({
  root: __dirname,
  plugins: [{
    name: 'copy-shoelace-assets',
    closeBundle() {
      cpSync(shoelaceAssets, resolve(__dirname, 'dist/shoelace/assets'), { recursive: true });
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
      '@hifimule/i18n-catalog': resolve(__dirname, '../hifimule-i18n/catalog.json'),
    },
  },
  build: {
    target:
      process.env.TAURI_ENV_PLATFORM == 'windows'
        ? 'chrome105'
        : 'safari13',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        splashscreen: resolve(__dirname, 'splashscreen.html'),
      },
    },
  },
  outDir: 'dist',
  // Recommended for Tauri to ensure assets are linked correctly
  emptyOutDir: true,
}));
