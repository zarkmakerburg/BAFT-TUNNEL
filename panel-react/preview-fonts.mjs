import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const fontDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../panel/fonts');
const allowed = new Set(['Vazirmatn-Regular.woff2', 'Vazirmatn-Medium.woff2', 'Vazirmatn-Bold.woff2']);

export default function localPreviewAssets() {
  return {
    name: 'baft-local-preview-fonts',
    configureServer(server) {
      server.middlewares.use('/fonts', (req, res, next) => {
        const name = (req.url || '').split('?')[0].replace(/^\//, '');
        if (!allowed.has(name)) return next();
        try {
          res.statusCode = 200;
          res.setHeader('Content-Type', 'font/woff2');
          res.setHeader('Cache-Control', 'no-cache');
          res.end(readFileSync(path.join(fontDir, name)));
        } catch { next(); }
      });
    },
  };
}
