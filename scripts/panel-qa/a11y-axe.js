// Automated accessibility audit of the panel with axe-core (roadmap B6).
// Static panel + mocked API: this checks the rendered markup of every page in fa/en, dark/light, desktop/mobile.
// It is an automated check only; it does not replace the physical phone pass or a formal audit.
// Run: NODE_PATH=/path/to/node_modules node scripts/panel-qa/a11y-axe.js
// Env: BAFT_A11Y_REPORT=<file> writes the full JSON report; BAFT_A11Y_FAIL_ON=critical,serious (default).
const { chromium } = require('playwright');
const http = require('http');
const fs = require('fs');
const path = require('path');

const PANEL = path.resolve(__dirname, '../../panel');
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const FAIL_ON = (process.env.BAFT_A11Y_FAIL_ON || 'critical,serious').split(',');
const PAGES = ['overview', 'tunnels', 'servers', 'monitoring', 'setup', 'diagnostics', 'activity', 'agent', 'settings'];
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };

function serve() {
  return http.createServer((req, res) => {
    const p = decodeURIComponent(new URL(req.url, 'http://127.0.0.1').pathname);
    const file = path.resolve(PANEL, p === '/' ? 'index.html' : p.replace(/^\//, ''));
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
  const report = [];
  let failures = 0;
  try {
    for (const [vname, viewport] of [['desktop', { width: 1280, height: 900 }], ['mobile', { width: 390, height: 844 }]]) {
      for (const lang of ['fa', 'en']) {
        for (const theme of ['dark', 'light']) {
          const ctx = await browser.newContext({ viewport, bypassCSP: true });
          const page = await ctx.newPage();
          await page.addInitScript(([l, t]) => {
            sessionStorage.setItem('token', 'test-token');
            sessionStorage.setItem('lang', l);
            try { localStorage.setItem('theme', t); localStorage.setItem('baft-theme', t); } catch (e) { /* storage may be blocked */ }
          }, [lang, theme]);
          // Honest minimal agent: status only; everything else is "not found" so pages render their Unknown states.
          await page.route('**/v1/**', route => {
            const u = new URL(route.request().url());
            if (route.request().method() === 'GET' && u.pathname === '/v1/status') {
              return route.fulfill({ json: { version: 'test', instance: 'node1', config_dir: '/etc/baft-tunnel', setup_pending: false } });
            }
            return route.fulfill({ status: 404, json: { error: 'not in this audit' } });
          });
          await page.goto(base + '/');
          await page.waitForSelector('.shell');
          await page.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
          await page.addScriptTag({ content: AXE });
          for (const name of PAGES) {
            await page.goto(base + '/#/' + name);
            await page.waitForTimeout(500);
            const result = await page.evaluate(() => axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] } }));
            const tag = `${vname}-${lang}-${theme}-${name}`;
            for (const v of result.violations) {
              const blocking = FAIL_ON.includes(v.impact);
              if (blocking) failures++;
              report.push({ page: tag, rule: v.id, impact: v.impact, blocking, help: v.help, nodes: v.nodes.slice(0, 5).map(n => n.target.join(' ')), count: v.nodes.length });
            }
          }
          await ctx.close();
        }
      }
    }
  } finally {
    await browser.close();
    server.close();
  }
  if (process.env.BAFT_A11Y_REPORT) fs.writeFileSync(process.env.BAFT_A11Y_REPORT, JSON.stringify({ fail_on: FAIL_ON, blocking: failures, violations: report }, null, 1));
  const by = {};
  for (const r of report) { const k = `${r.impact} ${r.rule}`; (by[k] = by[k] || { blocking: r.blocking, pages: 0, nodes: r.nodes[0] }).pages++; }
  for (const [k, v] of Object.entries(by)) console.log(`${v.blocking ? 'FAIL' : 'NOTE'} ${k} on ${v.pages} page-runs, e.g. ${v.nodes}`);
  console.log(failures ? `A11Y_AXE=FAIL (${failures} blocking violations)` : 'A11Y_AXE=PASS (no critical/serious axe violations; automated check only)');
  process.exit(failures ? 1 : 0);
})();
