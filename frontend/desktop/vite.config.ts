import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// Tauri serves this build from disk; there is no dev proxy because the desktop
// never talks to the server from the webview (specification §8).
//
// `@ui` is the shared view library and `@host` is this application, which is
// how a shared view reaches this build's Tauri transport without naming it. See
// frontend/shared/src/ui/README.md.
const ui = fileURLToPath(new URL('../shared/src/ui', import.meta.url));
const host = fileURLToPath(new URL('./src', import.meta.url));
// The dev server has to serve index.html, which sits at the package root rather
// than in src/. Naming `allow` at all replaces Vite's default of the project
// root, so leaving this out refuses the entry document itself.
const root = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  plugins: [vue()],
  resolve: {
    // The shared views resolve `vue` from their own package; without this a
    // build would bundle two Vue runtimes and reactivity would silently break.
    dedupe: ['vue', 'vue-router'],
    alias: [
      { find: /^@ui\//, replacement: `${ui}/` },
      { find: /^@host\//, replacement: `${host}/` },
    ],
  },
  clearScreen: false,
  server: {
    port: 5174,
    strictPort: true,
    // The shared views live outside this package's root.
    fs: { allow: [root, ui] },
  },
  build: { target: 'es2022' },
});
