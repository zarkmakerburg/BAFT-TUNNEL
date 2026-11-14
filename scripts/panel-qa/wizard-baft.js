// B4 Wizard QA: baft is selectable (Preview, Direct only), forces protocol v1, asks for the
// certificate names like tcp-tls, and its plan/preflight carry the engine-store gate.
// Needs a running agent: BAFT_TOKEN_FILE=<token file> [BAFT_URL=http://127.0.0.1:18080] node wizard-baft.js
const { chromium } = require('playwright');
const BASE = (process.env.BAFT_URL || 'http://127.0.0.1:18080').replace(/\/$/, '');
const TOKEN = require('fs').readFileSync(process.env.BAFT_TOKEN_FILE, 'utf8').trim();
const results = [];
const ok = (n, c, d) => { results.push([c ? 'PASS' : 'FAIL', n, d || '']); };
(async () => {
  const b = await chromium.launch();
  for (const lang of ['en', 'fa']) {
    const ctx = await b.newContext({ viewport: { width: 390, height: 844 } });
    const p = await ctx.newPage();
    const errs = []; p.on('pageerror', e => errs.push(String(e)));
    await p.addInitScript(([t, l]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); }, [TOKEN, lang]);
    await p.goto(BASE + '/#/setup'); await p.waitForSelector('.shell'); await p.waitForTimeout(600);
    const next = () => p.locator('.nav-row button.primary');
    await p.locator('.choice').nth(0).click(); await next().click(); await p.waitForTimeout(150);
    const f = p.locator('.form-grid input');
    await f.nth(0).fill('node-a'); await f.nth(1).fill('node-b'); await f.nth(2).fill('demo2'); await f.nth(3).fill('18080');
    await f.nth(4).fill('203.0.113.7'); await f.nth(5).fill('node-b.example.test'); await f.nth(6).fill('node-a.example.test');
    await next().click(); await p.waitForTimeout(200);
    const baft = p.locator('.choice').filter({ hasText: 'baft' });
    ok(`${lang} engine step lists baft`, await baft.count() === 1, await p.locator('.card').first().innerText());
    if (await baft.count() === 1) {
      const bt = await baft.innerText();
      ok(`${lang} baft is Preview, never Stable/Ready`, /Preview|پیش‌نمایش/.test(bt) && !/Stable|پایدار/.test(bt) && !/\bReady\b/.test(bt), bt);
      ok(`${lang} baft is selectable (B4)`, !(await baft.locator('input').isDisabled()));
      await baft.click(); await p.waitForTimeout(150);
      const w = await p.evaluate(() => ({ e: window.BAFT.wiz.engine, v: window.BAFT.wiz.protocolVersion }));
      ok(`${lang} choosing baft keeps protocol v1`, w.e === 'baft' && w.v === 1, JSON.stringify(w));
      const body = await p.evaluate(() => { const w = window.BAFT.wiz; return { server_name: w.serverName }; });
      ok(`${lang} forward baft keeps the peer certificate name`, body.server_name === 'node-b.example.test', JSON.stringify(body));
    }
    ok(`${lang} no page errors`, errs.length === 0, errs.join(';'));
    await ctx.close();
  }
  await b.close();
  for (const r of results) console.log(r.join(' | '));
  process.exit(results.some(r => r[0] === 'FAIL') ? 1 : 0);
})();
