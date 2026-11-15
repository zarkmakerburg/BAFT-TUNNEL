// Browser pass of the panel-managed offline tcp-tls PKI flow against a REAL agent (no mocked API).
// The CSR is created by the root PKI helper, signed on a throwaway offline CA with scripts/pki/baft-pki.sh,
// and installed through the panel. Evidence level: local browser + real agent/helper, not LAB.
// Needs a fresh agent with setup pending and an available PKI helper:
//   BAFT_TOKEN_FILE=<token file> [BAFT_URL=http://127.0.0.1:18080] [BAFT_LANGS=en,fa] node pki-real-agent.js
// Each language needs a fresh agent state; with both languages the second reuses the CSR of the first.
const { chromium } = require('playwright');
const { execFileSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const ROOT = path.resolve(__dirname, '../..');
const BASE = (process.env.BAFT_URL || 'http://127.0.0.1:18080').replace(/\/$/, '');
const TOKEN = fs.readFileSync(process.env.BAFT_TOKEN_FILE, 'utf8').trim();
const LANGS = (process.env.BAFT_LANGS || 'en').split(',');
const NODE = 'node1', SAN = 'node.example.net';
const results = [];
const ok = (name, pass, detail = '') => results.push({ name, pass: !!pass, detail });

(async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'baft-pki-real-'));
  const pass = path.join(tmp, 'pass');
  fs.writeFileSync(pass, 'qa-only-throwaway-passphrase\n', { mode: 0o600 });
  const pki = (...args) => execFileSync('bash', [path.join(ROOT, 'scripts/pki/baft-pki.sh'), ...args],
    { env: { ...process.env, BAFT_PKI_PASSFILE: pass }, stdio: ['ignore', 'pipe', 'pipe'] }).toString();
  pki('init-ca', path.join(tmp, 'ca'), 'BAFT-TUNNEL QA CA');
  const browser = await chromium.launch();
  try {
    for (const lang of LANGS) {
      const context = await browser.newContext({ locale: lang === 'fa' ? 'fa-IR' : 'en-US', acceptDownloads: true });
      await context.addInitScript(([t, l]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); }, [TOKEN, lang]);
      const page = await context.newPage();
      const pageErrors = [];
      const api = [];
      page.on('pageerror', e => pageErrors.push(e.message));
      page.on('response', r => { const u = new URL(r.url()); if (u.pathname.startsWith('/v1/')) api.push({ m: r.request().method(), p: u.pathname, s: r.status(), body: r.request().postData() || '' }); });
      await page.goto(BASE + '/#/setup');
      await page.waitForSelector('.shell');
      const next = () => page.locator('.nav-row button.primary');
      await page.locator('.choice').first().click(); // Direct
      await next().click(); // Servers
      // Service (serve) role: the peer dials in, so preflight needs no reachable peer address here.
      await page.locator('.choice').nth(1).click();
      const f = page.locator('.form-grid input');
      await f.nth(0).fill(NODE); await f.nth(1).fill('node2'); await f.nth(2).fill('qapki'); await f.nth(3).fill('18192');
      await page.locator('input[name="localServerName"]').fill(SAN);
      await next().click(); // Method (tcp-tls default)
      await next().click(); // Preflight
      await page.locator('.card button.primary').first().click();
      await page.waitForFunction(() => /Overall|وضعیت کلی/.test(document.querySelector('#main').innerText), null, { timeout: 30000 }).catch(() => {});
      ok(lang + ' preflight is not blocked', await next().isEnabled(), (await page.locator('#main').innerText()).match(/Blocked[\s\S]{0,200}|مسدود[\s\S]{0,200}/)?.[0] || '');
      await next().click(); // Preview
      await page.waitForTimeout(1500);
      await next().click(); // Apply / pairing
      await page.waitForTimeout(800);
      const mainText = () => page.locator('#main').innerText();
      const initButton = page.locator('button.primary').filter({ hasText: /Prepare and verify|آماده‌سازی و بررسی/ });
      const initConfirm = page.locator('.pair-a-confirm input');
      if (await initConfirm.count()) await initConfirm.check();
      ok(lang + ' Init stays disabled before certificates are installed', await initButton.count() === 1 && await initButton.isDisabled());
      const create = page.locator('button').filter({ hasText: /Create CSR|ساخت CSR|Show CSR|نمایش CSR/ });
      ok(lang + ' offers CSR generation', await create.count() >= 1);
      if (await create.count()) await create.first().click();
      const download = page.locator('button').filter({ hasText: /Download CSR|دانلود CSR/ });
      await download.waitFor({ timeout: 30000 }).catch(() => {});
      const csrCall = api.find(c => /\/v1\/pki\/csr\/(create|read)/.test(c.p));
      ok(lang + ' real PKI helper created/returned the CSR', csrCall && csrCall.s >= 200 && csrCall.s < 300, csrCall ? csrCall.p + ' ' + csrCall.s : 'no call');
      ok(lang + ' CSR request is bound to this node and its SAN', csrCall && /"node_id":"node1"/.test(csrCall.body) && /node\.example\.net/.test(csrCall.body));
      const csrFile = path.join(tmp, lang + '-node.csr');
      if (await download.count()) {
        const [d] = await Promise.all([page.waitForEvent('download'), download.click()]);
        await d.saveAs(csrFile);
      }
      const csr = fs.existsSync(csrFile) ? fs.readFileSync(csrFile, 'utf8') : '';
      ok(lang + ' downloaded CSR is a CSR and holds no private key', /BEGIN CERTIFICATE REQUEST/.test(csr) && !/PRIVATE KEY/.test(csr));
      const out = path.join(tmp, lang + '-out');
      let signed = false;
      try { pki('issue-node', path.join(tmp, 'ca'), NODE, csrFile, out, SAN); signed = true; } catch (e) { ok(lang + ' offline CA signs the CSR', false, String(e.stderr || e)); }
      if (signed) ok(lang + ' offline CA signs the CSR', true);
      const files = page.locator('.pki-upload input[type=file]');
      ok(lang + ' exposes exactly three public uploads', await files.count() === 3);
      if (signed && await files.count() === 3) {
        await files.nth(0).setInputFiles(path.join(out, 'node.crt'));
        await files.nth(1).setInputFiles(path.join(out, 'ca.crt'));
        await files.nth(2).setInputFiles(path.join(out, 'ca.crl'));
        await page.waitForFunction(() => document.querySelectorAll('.pki-upload .sub').length >= 3);
      }
      const confirm = page.locator('.pki-confirm input');
      if (await confirm.count()) await confirm.check();
      const upload = page.locator('button').filter({ hasText: /Validate and install|اعتبارسنجی و نصب/ });
      await page.waitForFunction(() => {
        const b = Array.from(document.querySelectorAll('button')).find(x => /Validate and install|اعتبارسنجی و نصب/.test(x.textContent));
        return !!b && !b.disabled;
      }, null, { timeout: 10000 }).catch(() => {});
      if (await upload.count()) {
        await Promise.all([
          page.waitForResponse(r => new URL(r.url()).pathname === '/v1/pki/certificate/install', { timeout: 60000 }).catch(() => {}),
          upload.click(),
        ]);
        await page.waitForTimeout(500);
      }
      const install = api.find(c => c.p === '/v1/pki/certificate/install');
      ok(lang + ' real helper installed the certificate', install && install.s === 200, install ? String(install.s) : 'no call: ' + (await mainText()).slice(-900).replace(/\s+/g, ' '));
      ok(lang + ' install upload carries no private key', install && !/PRIVATE KEY/.test(install.body));
      const fp = signed ? execFileSync('openssl', ['x509', '-in', path.join(out, 'node.crt'), '-noout', '-fingerprint', '-sha256']).toString().split('=')[1].replace(/:/g, '').trim().toLowerCase() : 'unsigned';
      ok(lang + ' panel shows the installed certificate fingerprint', (await mainText()).toLowerCase().replace(/[:\s]/g, '').includes(fp), fp);
      ok(lang + ' Init unlocks after a real install', await initButton.isEnabled());
      ok(lang + ' no token in page text', !(await page.content()).includes(TOKEN));
      ok(lang + ' no 5xx from the agent', !api.some(c => c.s >= 500), api.filter(c => c.s >= 500).map(c => c.p).join(','));
      ok(lang + ' no page errors', pageErrors.length === 0, pageErrors.join('; '));
      await context.close();
    }
  } finally {
    await browser.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  for (const r of results) console.log((r.pass ? 'PASS' : 'FAIL') + ' | ' + r.name + (r.detail ? ' | ' + r.detail : ''));
  process.exitCode = results.some(r => !r.pass) ? 1 : 0;
})();
