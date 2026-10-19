/// <reference types="vitest/config" />
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// @ts-expect-error process is a node global
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [svelte()],

  // Vitest resolves modules with Node's export conditions by default, which
  // would pull in Svelte's *server* entry point and break `mount`/`onMount` in
  // the DOM tests. Opting into the browser conditions keeps the client build.
  resolve: process.env.VITEST ? { conditions: ['browser'] } : undefined,

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**']
    }
  },

  test: {
    // Store + API unit tests need no DOM and every `@tauri-apps` call is mocked,
    // so the default environment is plain node. Component tests opt into jsdom
    // per file with a `// @vitest-environment jsdom` pragma, which keeps the
    // store suite DOM-free.
    environment: 'node',
    include: ['src/**/*.test.ts']
  }
}));
