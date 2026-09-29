import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The UI is loaded from the app bundle by the Tauri host (tauri://localhost),
// or from this dev server during development (with a mocked backend when
// opened in a normal browser).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    target: 'chrome120',
    cssMinify: true,
    modulePreload: { polyfill: false },
    reportCompressedSize: false,
    rollupOptions: {
      output: {
        // One small bundle; the mock backend and dev-only pages are split out
        // and never loaded in the app.
        manualChunks: (id) => (id.includes('/lib/mock') || id.includes('/lib/dev/') ? 'mock' : undefined),
      },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
  },
});
