import localPreviewAssets from './preview-fonts.mjs';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The Rust agent embeds these exact files (src/agent.rs, PANEL_ASSETS), so output names are fixed
// (no content hashes) and everything is one JS + one CSS file served from the agent itself.
export default defineConfig({
  base: '/next/',
  plugins: [react(), localPreviewAssets()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    cssCodeSplit: false,
    modulePreload: false,
    assetsInlineLimit: 0,
    sourcemap: false,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/app.js',
        chunkFileNames: 'assets/app-[name].js',
        assetFileNames: 'assets/app[extname]',
        inlineDynamicImports: true,
      },
    },
  },
  server: {
    // Dev only: proxy fixture API and translations; fonts are served from the local panel assets.
    proxy: {
      '/v1': 'http://127.0.0.1:18090',
      // The embedded production HTML requests the Agent's root-level /i18n.js route.
      '/i18n.js': 'http://127.0.0.1:18090',
    },
  },
});
