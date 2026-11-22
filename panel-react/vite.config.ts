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
    // Dev only: proxy API and the shared i18n/font assets to a running agent.
    proxy: {
      '/v1': 'http://127.0.0.1:18090',
      '/i18n.js': 'http://127.0.0.1:18090',
      '/fonts': 'http://127.0.0.1:18090',
      // Vite prefixes root-relative HTML/CSS assets with base (/next/); map those dev requests
      // back to the Agent's canonical root asset routes.
      '/next/i18n.js': { target: 'http://127.0.0.1:18090', rewrite: (path) => path.replace(/^\/next/, '') },
      '/next/fonts': { target: 'http://127.0.0.1:18090', rewrite: (path) => path.replace(/^\/next/, '') },
    },
  },
});
