var _a;
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
export default defineConfig({
    plugins: [vue()],
    server: {
        proxy: {
            // In development the API is served by phototools-server.
            '/api': {
                target: (_a = process.env.VITE_API_TARGET) !== null && _a !== void 0 ? _a : 'http://127.0.0.1:3000',
                changeOrigin: true,
            },
        },
    },
});
