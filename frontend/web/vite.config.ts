import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// `@ui` is the shared view library and `@host` is this application, which is
// how a shared view reaches this build's transport without naming it. See
// frontend/shared/src/ui/README.md.
const ui = fileURLToPath(new URL('../shared/src/ui', import.meta.url));
const host = fileURLToPath(new URL('./src', import.meta.url));

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
  server: {
    // The shared views live outside this package's root.
    fs: { allow: [host, ui] },
    proxy: {
      // In development the API is served by phototools-server.
      '/api': {
        target: process.env.VITE_API_TARGET ?? 'http://127.0.0.1:3000',
        changeOrigin: true,
      },
    },
  },
});
