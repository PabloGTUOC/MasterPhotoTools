import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// Tauri serves this build from disk; there is no dev proxy because the desktop
// never talks to the server from the webview (specification §8).
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: { port: 5174, strictPort: true },
  build: { target: 'es2022' },
});
