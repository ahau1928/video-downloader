import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    strictPort: true,
    port: 1420,
    host: '127.0.0.1',
    watch: {
      ignored: ['**/src-tauri/**', '**/.review/**', '**/release/**', '**/.video-downloader/**']
    }
  },
  envPrefix: ['VITE_', 'TAURI_']
});
