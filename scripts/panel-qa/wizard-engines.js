// Wizard engine/role QA: BAFT (Preview, Direct only) and Reverse tcp-tls (Preview, never Stable/Ready).
// Needs a running agent: BAFT_TOKEN_FILE=<token file> [BAFT_URL=http://127.0.0.1:18080] node wizard-engines.js
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
    const tag = lang;
    const next = () => p.locator('.nav-row button.primary');
    const body = async () => (await p.locator('.card').first().innerText());
    const fill = async () => {
      const f = p.locator('.form-grid input');
      await f.nth(1).fill('node-b'); await f.nth(2).fill('demo2'); await f.nth(3).fill('8080');
    };
    // Direct: BAFT listed as Preview, no Stable/Ready
    await p.locator('.choice').nth(0).click(); await next().click(); await next().click().catch(() => {});
    await fill(); await p.locator('.form-grid input').nth(4).fill('127.0.0.1'); await p.locator('.form-grid input').nth(5).fill('node-b');
    await next().click(); await p.waitForTimeout(200);
    const choices = p.locator('.choice');
    ok(`${tag} direct method step lists tcp-tls, noise-tcp and baft`, await choices.count() === 3);
    const baft = choices.filter({ hasText: 'baft' });
    const bt = await baft.innerText();
    ok(`${tag} baft shown as Preview, not Stable`, /Preview|پیش‌نمایش/.test(bt) && !/Stable|پایدار/.test(bt), bt);
    ok(`${tag} baft states unknown status limit`, /unknown|نامشخص/.test(bt));
    ok(`${tag} baft never Ready`, !/\bReady\b|آماده/.test(bt));
    await baft.click(); await next().click(); await p.waitForTimeout(200);
    await p.locator('button.primary').filter({ hasText: lang === 'fa' ? 'اجرای بررسی' : 'Run checks' }).first().click();
    await p.waitForSelector('.pf'); 
    ok(`${tag} baft preflight is blocked (setup cannot create its config)`, await p.locator('.pf .badge.bad').count() >= 1);
    ok(`${tag} baft continue stays disabled`, await next().isDisabled());
    // Reverse: roles
    await p.goto(BASE + '/#/setup'); await p.reload(); await p.waitForSelector('.shell'); await p.waitForTimeout(500);
    await p.evaluate(() => { window.BAFT.wiz = null; });
    await p.goto(BASE + '/#/overview'); await p.goto(BASE + '/#/setup'); await p.waitForTimeout(500);
    const rev = p.locator('.choice').nth(1);
    ok(`${tag} reverse enabled and labelled Preview`, !(await rev.locator('input').isDisabled()) && /Preview|پیش‌نمایش/.test(await rev.innerText()));
    await rev.click(); await next().click(); await p.waitForTimeout(200);
    const roles = await p.locator('.choice').allInnerTexts();
    const joined = roles.join(' | ');
    ok(`${tag} reverse roles use dial-out / public-node wording`, lang === 'en' ? /public node/i.test(joined) && /dials out/i.test(joined) && !/Users connect to this host/.test(joined) : !/کاربران به این میزبان وصل/.test(joined) && /گرهٔ عمومی|گره عمومی/.test(joined), joined);
    await fill();
    await p.locator('.choice').nth(1).click();
    ok(`${tag} public node asks for the user-facing port`, /User-facing port|پورت ورود کاربران/.test(await body()));
    await p.locator('.choice').nth(0).click();
    ok(`${tag} service node asks for the service port`, /Service port|پورت سرویس/.test(await body()));
    ok(`${tag} no page errors`, errs.length === 0, errs.join(';'));
    await ctx.close();
  }
  await b.close();
  for (const r of results) console.log(r.join(' | '));
  process.exit(results.some(r => r[0] === 'FAIL') ? 1 : 0);
})();
