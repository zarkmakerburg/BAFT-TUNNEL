// Guards the committed build output: it must exist, be self-contained and match what the agent embeds.
import { readFileSync, existsSync } from 'node:fs';
const need = ['dist/index.html', 'dist/assets/app.js', 'dist/assets/app.css'];
for (const f of need) if (!existsSync(f)) { console.error('missing', f); process.exit(1); }
const html = readFileSync('dist/index.html', 'utf8');
const bad = [/https?:\/\//.test(html) ? 'absolute URL in index.html' : '', /<style|style=/.test(html) ? 'inline style' : '', /<script(?![^>]*\bsrc=)/.test(html) ? 'inline script' : ''].filter(Boolean);
const js = readFileSync('dist/assets/app.js', 'utf8');
const allowed = /https?:\/\/(www\.w3\.org|react\.dev)[^"'`\s)]*|http:\/\/\$\{/g;
if (/https?:\/\//.test(js.replace(allowed, ''))) bad.push('unexpected absolute URL in app.js');
if (bad.length) { console.error('dist check failed:', bad.join(', ')); process.exit(1); }
console.log('dist ok:', need.map((f) => `${f} ${readFileSync(f).length}B`).join(', '));
