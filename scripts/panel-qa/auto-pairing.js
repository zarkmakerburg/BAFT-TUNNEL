// Browser acceptance for the automatic pairing flow (ADR-0050), English and Persian.
// Mocked API: proves UI sequencing and payload boundaries, not root-helper or LAB behavior.
// Run: NODE_PATH=/path/to/node_modules node scripts/panel-qa/auto-pairing.js
const { chromium } = require('playwright');
const http = require('http');
const fs = require('fs');
const path = require('path');

const PANEL = path.resolve(__dirname, '../../panel');
const CODE = 'BAFT1-' + 'A'.repeat(40);
const results = [];
const ok = (name, pass, detail = '') => results.push({ name, pass: !!pass, detail });
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };

function serve() {
  return http.createServer((req, res) => {
    const urlPath = decodeURIComponent(new URL(req.url, 'http://127.0.0.1').pathname);
    const file = path.resolve(PANEL, urlPath === '/' ? 'index.html' : urlPath.replace(/^\//, ''));
    if (!file.startsWith(PANEL + path.sep)) { res.writeHead(403).end(); return; }
    fs.readFile(file, (err, bytes) => {
      if (err) { res.writeHead(404).end(); return; }
      res.writeHead(200, { 'Content-Type': mime[path.extname(file)] || 'application/octet-stream' });
      res.end(bytes);
    });
  });
}

(async () => {
  const server = serve();
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  const base = 'http://127.0.0.1:' + server.address().port;
  const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_BIN || undefined });
  try {
    for (const lang of ['en', 'fa']) {
      for (const role of ['first', 'second']) {
        const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
        const page = await context.newPage();
        page.setDefaultTimeout(8000);
        const posts = [];
        const errors = [];
        let polls = 0, configured = false;
        page.on('pageerror', e => errors.push(String(e)));
        await page.addInitScript(([token, locale]) => { sessionStorage.setItem('token', token); sessionStorage.setItem('lang', locale); }, ['t', lang]);
        await page.route('**/v1/**', route => {
          const req = route.request();
          const u = new URL(req.url());
          const m = req.method();
          if (m === 'POST') { posts.push({ path: u.pathname, body: req.postData() || '' }); }
          if (m === 'GET' && u.pathname === '/v1/status') return route.fulfill({ json: { version: 't', setup_pending: !configured, instance: 'n1', config_dir: '/etc/baft-tunnel', setup_helper: { available: true }, pairing: {} } });
          if (m === 'GET' && u.pathname === '/v1/engines') return route.fulfill({ json: { engines: [{ id: 'tcp-tls', status: 'stable', directions: ['direct'] }] } });
          if (m === 'GET' && u.pathname === '/v1/host/tunnels') return route.fulfill({ json: { tunnels: [] } });
          if (m === 'POST' && (u.pathname === '/v1/pair/offer' || u.pathname === '/v1/pair/accept')) return route.fulfill({ status: 202, json: { state: 'starting' } });
          if (m === 'POST' && u.pathname === '/v1/pair/approve') return route.fulfill({ json: { ok: true } });
          if (m === 'GET' && u.pathname === '/v1/pair/status') {
            polls += 1;
            if (role === 'first') {
              const seq = polls < 2 ? { state: 'waiting', code: CODE, expires_in: 900 } : polls < 4 ? { state: 'approval', nonce: 'n0nce', verify: 'ab12-cd34-ef56', peer: { node_id: 'demo-f-1a2b3c', address: '203.0.113.9:5555' } } : { state: 'done' };
              if (seq.state === 'done') configured = true;
              return route.fulfill({ json: seq });
            }
            if (polls >= 2) configured = true;
            return route.fulfill({ json: polls < 2 ? { state: 'working' } : { state: 'done' } });
          }
          if (m === 'POST' && u.pathname === '/v1/tunnel/lifecycle') return route.fulfill({ json: { result: 'applied' } });
          return route.fulfill({ status: 404, json: { error: 'not mocked' } });
        });
        await page.goto(base + '/#/setup');
        await page.waitForSelector('.shell');
        const tag = lang + '/' + role;
        ok(tag + ' simple flow is the default in setup mode (no wizard stepper)', await page.locator('.stepper').count() === 0 && await page.locator('input[name="autorole"]').count() === 2);
        await page.locator('input[name="autorole"][value="' + role + '"]').check();
        await page.locator('input[name="autoloc"][value="' + (role === 'first' ? 'abroad' : 'iran') + '"]').check();
        const reverse = role === 'first' && lang === 'fa';
        if (role === 'first') await page.locator('input[name="autosvc"][value="' + (reverse ? 'other' : 'here') + '"]').check();
        if (role === 'first') {
          const inputs = page.locator('.form-grid input');
          await inputs.nth(0).fill('demo'); await inputs.nth(1).fill('9000');
          await page.locator('button.primary').click();
          await page.waitForSelector('pre');
          ok(tag + ' shows the code and the exact command for the second server', (await page.locator('pre').innerText()).includes('pair accept ' + CODE));
          await page.locator('button.primary').waitFor();
          await page.locator('button.primary').click(); // approve
          ok(tag + ' approval is sent as approve=true', posts.some(p => p.path === '/v1/pair/approve' && JSON.parse(p.body).approve === true && JSON.parse(p.body).nonce === 'n0nce'));
          ok(tag + ' verification string shown', (await page.content()).includes('ab12-cd34-ef56'));
          const offer = posts.find(p => p.path === '/v1/pair/offer');
          ok(tag + ' offer payload has only name, port and confirm', offer && JSON.stringify(Object.keys(JSON.parse(offer.body)).sort()) === JSON.stringify(reverse ? ['confirm', 'direction', 'name', 'service_port'] : ['confirm', 'name', 'service_port']) && (!reverse || JSON.parse(offer.body).direction === 'reverse'));
        } else {
          await page.locator('textarea').fill('sudo baft-tunnel pair accept ' + CODE);
          await page.locator('button.primary').click();
          const accept = posts.find(p => p.path === '/v1/pair/accept');
          ok(tag + ' accept sends the bare code with confirm', accept && JSON.parse(accept.body).code === CODE && JSON.parse(accept.body).confirm === true && JSON.parse(accept.body).location === 'iran');
        }
        await page.waitForSelector('.banner.info[role="status"]', { timeout: 20000 });
        ok(tag + ' tunnel is started automatically after pairing', posts.some(p => p.path === '/v1/tunnel/lifecycle' && JSON.parse(p.body).action === 'start'));
        ok(tag + ' never sends key, CA or certificate material', !posts.some(p => /PRIVATE|BEGIN CERTIFICATE|ca\.key/.test(p.body)));
        ok(tag + ' no page errors', errors.length === 0, errors.join('; '));
        await context.close();
      }
    }
  } finally {
    await browser.close();
    server.close();
  }
  for (const r of results) console.log((r.pass ? 'PASS: ' : 'FAIL: ') + r.name + (r.pass ? '' : ' ' + r.detail));
  if (results.some(r => !r.pass)) process.exit(1);
  console.log('AUTO_PAIRING_PANEL_TEST=PASS');
})().catch(e => { console.error(e); process.exit(1); });
