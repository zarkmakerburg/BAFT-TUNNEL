// Browser acceptance for the panel-managed offline tcp-tls PKI flow.
// Uses mocked API responses: this proves UI sequencing and payload boundaries, not root-helper/LAB behavior.
// Run: NODE_PATH=/path/to/node_modules node scripts/panel-qa/pki-enrollment.js
const { chromium } = require('playwright');
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');

const ROOT = path.resolve(__dirname, '../..');
const PANEL = path.join(ROOT, 'panel');
const CSR = '-----BEGIN CERTIFICATE REQUEST-----\nMIIBtestcsr\n-----END CERTIFICATE REQUEST-----\n';
const CERT = '-----BEGIN CERTIFICATE-----\nMIIBtestcert\n-----END CERTIFICATE-----\n';
const CA = '-----BEGIN CERTIFICATE-----\nMIIBtestca\n-----END CERTIFICATE-----\n';
const CRL = '-----BEGIN X509 CRL-----\nMIIBtestcrl\n-----END X509 CRL-----\n';
const results = [];
const ok = (name, pass, detail = '') => results.push({ name, pass: !!pass, detail });
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };

function serve() {
  return http.createServer((req, res) => {
    const urlPath = decodeURIComponent(new URL(req.url, 'http://127.0.0.1').pathname);
    const name = urlPath === '/' ? 'index.html' : urlPath.replace(/^\//, '');
    const file = path.resolve(PANEL, name);
    if (!file.startsWith(PANEL + path.sep) && file !== path.join(PANEL, 'index.html')) {
      res.writeHead(403).end(); return;
    }
    fs.readFile(file, (err, bytes) => {
      if (err) { res.writeHead(404).end(); return; }
      res.writeHead(200, { 'Content-Type': mime[path.extname(file)] || 'application/octet-stream' });
      res.end(bytes);
    });
  });
}

(async () => {
  const server = serve();
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = 'http://127.0.0.1:' + server.address().port;
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'baft-pki-panel-'));
  fs.writeFileSync(path.join(tmp, 'node.crt'), CERT);
  fs.writeFileSync(path.join(tmp, 'private.key'), '-----BEGIN PRIVATE KEY-----\nnot-for-upload\n-----END PRIVATE KEY-----\n');
  fs.writeFileSync(path.join(tmp, 'ca.crt'), CA);
  fs.writeFileSync(path.join(tmp, 'ca.crl'), CRL);
  const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_BIN || undefined });
  try {
    for (const lang of ['en', 'fa']) {
      const context = await browser.newContext({ viewport: { width: 390, height: 844 }, acceptDownloads: true });
      const page = await context.newPage();
      page.setDefaultTimeout(7000);
      const apiCalls = [];
      const pageErrors = [];
      page.on('pageerror', e => pageErrors.push(String(e)));
      await page.addInitScript(([token, locale]) => {
        sessionStorage.setItem('token', token);
        sessionStorage.setItem('lang', locale);
      }, ['test-token', lang]);
      await page.route('**/v1/**', async route => {
        const request = route.request();
        const url = new URL(request.url());
        const body = request.postDataJSON?.() || {};
        apiCalls.push({ method: request.method(), path: url.pathname, body });
        if (request.method() === 'GET' && url.pathname === '/v1/status') return route.fulfill({ json: { version: 'test', setup_pending: true, instance: 'node1', config_dir: '/etc/baft-tunnel', setup_helper: { available: true }, pairing: { initialized: false } } });
        if (request.method() === 'GET' && url.pathname === '/v1/engines') return route.fulfill({ json: { engines: [
          { id: 'tcp-tls', status: 'stable', guided_setup: { available: true }, directions: ['direct'], evidence: { lab_two_host: { direct: 'pass' } } },
          { id: 'noise-tcp', status: 'experimental', guided_setup: { available: true }, directions: ['direct'], evidence: { lab_two_host: { direct: 'pass' } } },
          { id: 'baft', status: 'preview', guided_setup: { available: false }, directions: ['direct'] }
        ] } });
        if (request.method() === 'POST' && url.pathname === '/v1/setup/preflight') return route.fulfill({ json: { overall: 'warning', checks: [{ id: 'pki_files', result: 'warning', scope: 'local', evidence: 'certificate set is not installed yet', remediation: 'complete enrollment' }], apply_available: true } });
        if (request.method() === 'POST' && url.pathname === '/v1/setup/plan') return route.fulfill({ json: { files: [], pending: [], commands: ['sudo baft-tunnel setup init'], config: { engine: 'tcp-tls' } } });
        if (request.method() === 'POST' && url.pathname === '/v1/pki/csr/create') return route.fulfill({ status: 201, json: { result: 'created', request_id: body.request_id, node_id: body.node_id, expected_server_san: 'DNS:node.example.net', csr_pem: CSR, public_key_sha256: 'a'.repeat(64), private_key_exported: false } });
        if (request.method() === 'POST' && url.pathname === '/v1/pki/csr/read') return route.fulfill({ status: 200, json: { result: 'retrieved', request_id: body.request_id, node_id: body.node_id, expected_server_san: 'DNS:node.example.net', csr_pem: CSR, public_key_sha256: 'a'.repeat(64), private_key_exported: false } });
        if (request.method() === 'POST' && url.pathname === '/v1/pki/certificate/install') return route.fulfill({ status: 200, json: { result: 'installed', node_id: body.node_id, certificate_sha256: 'b'.repeat(64), certificate_not_after: 1800000000, crl_seconds_remaining: 864000, active_set: 'c'.repeat(64), service_restarted: false, private_key_exported: false } });
        if (request.method() === 'POST' && url.pathname === '/v1/setup/apply') return route.fulfill({ status: 200, json: { result: 'applied', id: 'apply-test', verification: [], changes: [], not_done: [] } });
        if (request.method() === 'GET' && url.pathname === '/v1/setup/bundle') return route.fulfill({ status: 200, json: { bundle: '{"version":1}', checksum: '11111111-22222222-33333333-44444444', fields: {} } });
        return route.fulfill({ status: 404, json: { error: 'unexpected test API route' } });
      });

      await page.goto(base + '/#/setup');
      await page.waitForSelector('.shell');
      const next = () => page.locator('.nav-row button.primary');
      await page.locator('.choice').first().click(); // Direct
      await next().click(); // Servers
      const fields = page.locator('.form-grid input');
      await fields.nth(0).fill('node1');
      await fields.nth(1).fill('node2');
      await fields.nth(2).fill('test-tunnel');
      await fields.nth(3).fill('18080');
      await fields.nth(4).fill('192.0.2.20');
      await fields.nth(5).fill('node2.example.net');
      const localSan = page.locator('input[name="localServerName"]');
      ok(lang + ' exposes a separate local DNS/IP identity field', await localSan.count() === 1);
      if (await localSan.count()) await localSan.fill('node.example.net');
      await next().click(); // Method
      await next().click(); // Preflight
      await page.locator('.card button.primary').first().click();
      await page.waitForTimeout(100);
      ok(lang + ' treats missing pre-enrollment certificates as a warning, not a dead-end', /not installed yet|هنوز نصب نشده/.test(await page.locator('#main').innerText()));
      await next().click(); // Preview
      await page.waitForTimeout(150);
      await next().click(); // Apply / pairing
      await page.waitForTimeout(100);
      const mainText = () => page.locator('#main').innerText();
      ok(lang + ' explains local private-key custody', /private key|کلید خصوصی/i.test(await mainText()));
      const initButton = page.locator('button.primary').filter({ hasText: /Prepare and verify|آماده‌سازی و بررسی/ });
      const initConfirm = page.locator('.pair-a-confirm input');
      if (await initConfirm.count()) await initConfirm.check();
      ok(lang + ' keeps tcp-tls Init disabled even after operator confirmation until certificates are installed', await initButton.count() === 1 && await initButton.isDisabled());
      const create = page.locator('button').filter({ hasText: /Create CSR|ساخت CSR/ });
      ok(lang + ' offers CSR generation', await create.count() === 1);
      if (await create.count()) await create.click();
      await page.waitForTimeout(100);
      ok(lang + ' explains that signing happens on the offline CA', /offline|آفلاین/i.test(await mainText()));
      const csrBody = apiCalls.find(c => c.path === '/v1/pki/csr/create')?.body;
      ok(lang + ' binds CSR to this node and its own SAN', csrBody?.node_id === 'node1' && csrBody?.server_name === 'node.example.net' && csrBody?.confirm === true);
      ok(lang + ' never sends a private key to the browser API', !JSON.stringify(apiCalls).includes('PRIVATE KEY'));
      const downloadButton = page.locator('button').filter({ hasText: /Download CSR|دانلود CSR/ });
      ok(lang + ' offers CSR download', await downloadButton.count() === 1);
      if (await downloadButton.count()) {
        const [download] = await Promise.all([page.waitForEvent('download'), downloadButton.click()]);
        const downloadPath = path.join(tmp, lang + '-node.csr');
        await download.saveAs(downloadPath);
        ok(lang + ' downloaded file contains only the CSR', fs.readFileSync(downloadPath, 'utf8') === CSR);
      }
      const files = page.locator('.pki-upload input[type=file]');
      ok(lang + ' exposes exactly three public certificate uploads and no key upload', await files.count() === 3);
      if (await files.count() === 3) {
        await files.nth(0).setInputFiles(path.join(tmp, 'private.key'));
        await page.getByText(/private keys are not accepted|کلید خصوصی پذیرفته نمی‌شود/).waitFor();
        ok(lang + ' rejects a private-key file before any API upload', !apiCalls.some(c => c.path === '/v1/pki/certificate/install'));
        await files.nth(0).setInputFiles(path.join(tmp, 'node.crt'));
        await page.waitForFunction(() => document.querySelectorAll('.pki-upload .sub').length >= 1);
        await files.nth(1).setInputFiles(path.join(tmp, 'ca.crt'));
        await page.waitForFunction(() => document.querySelectorAll('.pki-upload .sub').length >= 2);
        await files.nth(2).setInputFiles(path.join(tmp, 'ca.crl'));
        await page.waitForFunction(() => document.querySelectorAll('.pki-upload .sub').length >= 3);
      }
      const confirm = page.locator('.pki-confirm input');
      if (await confirm.count()) await confirm.check();
      const upload = page.locator('button').filter({ hasText: /Validate and install|اعتبارسنجی و نصب/ });
      ok(lang + ' exposes an explicit certificate installation action', await upload.count() === 1);
      await page.waitForFunction(() => {
        const b = Array.from(document.querySelectorAll('button')).find(x => /Validate and install|اعتبارسنجی و نصب/.test(x.textContent));
        return !!b && !b.disabled;
      });
      if (await upload.count()) await upload.click();
      await page.waitForTimeout(150);
      const installCall = apiCalls.find(c => c.path === '/v1/pki/certificate/install');
      ok(lang + ' uploads only certificate, CA and CRL PEM', !!installCall && installCall.body.certificate_pem === CERT && installCall.body.ca_pem === CA && installCall.body.crl_pem === CRL && !('private_key' in installCall.body));
      ok(lang + ' unlocks Init only after successful certificate install', await initButton.isEnabled());
      ok(lang + ' shows installed fingerprint and no service restart', /b{64}/.test(await mainText()) && /restart|راه‌اندازی مجدد/i.test(await mainText()));
      ok(lang + ' has no browser errors', pageErrors.length === 0, pageErrors.join('; '));
      await context.close();
    }
  } finally {
    await browser.close();
    server.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  for (const r of results) console.log((r.pass ? 'PASS' : 'FAIL') + ' | ' + r.name + (r.detail ? ' | ' + r.detail : ''));
  process.exitCode = results.some(r => !r.pass) ? 1 : 0;
})();
