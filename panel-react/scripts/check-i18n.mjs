import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';

const root = path.resolve(import.meta.dirname, '..', '..');
const panelRoot = path.join(root, 'panel');
const reactRoot = path.join(root, 'panel-react', 'src');
const context = { window: {} };
vm.runInNewContext(fs.readFileSync(path.join(panelRoot, 'i18n.js'), 'utf8'), context, { filename: 'panel/i18n.js' });
const base = context.window.BAFT_I18N;
if (!base?.fa || !base?.en) throw new Error('Classic dictionary did not define both fa and en.');

const reactSource = fs.readFileSync(path.join(reactRoot, 'i18n.ts'), 'utf8').split('const EXTRA:')[1];
if (!reactSource) throw new Error('Could not find React EXTRA dictionaries.');
const faBlock = reactSource.split('  fa: {')[1]?.split('\n  },\n  en: {')[0];
const enBlock = reactSource.split('  en: {')[1]?.split('\n  },\n};')[0];
if (!faBlock || !enBlock) throw new Error('Could not parse React fa/en dictionaries.');
const extractKeys = (source) => [...source.matchAll(/^\s*'((?:[^'\\]|\\.)+)'\s*:/gm)].map((m) => m[1].replace(/\\'/g, "'"));
const extractValue = (source, key) => {
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = source.match(new RegExp("'" + escaped + "'\\s*:\\s*'((?:[^'\\\\]|\\\\.)*)'"));
  return match?.[1] ?? '';
};
const extra = { fa: new Set(extractKeys(faBlock)), en: new Set(extractKeys(enBlock)) };
const keys = Object.fromEntries(['fa', 'en'].map((lang) => [lang, new Set([...Object.keys(base[lang]), ...extra[lang]])]));
const errors = [];
for (const key of extra.fa) if (!extra.en.has(key)) errors.push(`React EXTRA missing en: ${key}`);
for (const key of extra.en) if (!extra.fa.has(key)) errors.push(`React EXTRA missing fa: ${key}`);
for (const lang of ['fa', 'en']) for (const [key, value] of Object.entries(base[lang])) if (typeof value !== 'string' || !value.length) errors.push(`${lang} has empty/non-string value: ${key}`);
const allKeys = new Set([...keys.fa, ...keys.en]);
const placeholders = (value) => [...value.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');
for (const key of allKeys) {
  if (!keys.fa.has(key)) errors.push(`Missing Persian translation: ${key}`);
  if (!keys.en.has(key)) errors.push(`Missing English translation: ${key}`);
  const fa = extra.fa.has(key) ? extractValue(faBlock, key) : base.fa[key];
  const en = extra.en.has(key) ? extractValue(enBlock, key) : base.en[key];
  if (fa && en && placeholders(fa) !== placeholders(en)) errors.push(`Placeholder mismatch (${key}): fa={${placeholders(fa)}} en={${placeholders(en)}}`);
}

const files = [];
function walk(dir) {
  for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, item.name);
    if (item.isDirectory()) walk(file);
    else if (/\.(?:js|jsx|ts|tsx)$/.test(file) && !file.endsWith('/i18n.js') && !file.endsWith('/i18n.ts')) files.push(file);
  }
}
walk(panelRoot);
walk(reactRoot);
const refs = new Set();
for (const file of files) {
  const source = fs.readFileSync(file, 'utf8');
  for (const match of source.matchAll(/\b(?:t|tr|translate)\s*\(\s*['"`]([^'"`]+)['"`]/g)) {
    const key = match[1];
    const tail = source.slice(match.index + match[0].length);
    if (!key.endsWith('.') && !/^\s*\+/.test(tail)) refs.add(key);
  }
}
for (const key of refs) for (const lang of ['fa', 'en']) if (!keys[lang].has(key)) errors.push(`Literal callsite missing ${lang}: ${key}`);

if (errors.length) {
  console.error(errors.map((error) => `✗ ${error}`).join('\n'));
  process.exitCode = 1;
} else {
  console.log(`i18n OK: ${keys.fa.size} keys per language; ${refs.size} literal callsites; placeholders match.`);
}
