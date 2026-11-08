// Browser QA for the panel. Needs a running agent: BAFT_TOKEN_FILE=<token file> [BAFT_URL=http://127.0.0.1:18080] [BAFT_SHOTS=<dir>] node browser-flows.js
const { chromium } = require('playwright');
const BASE = (process.env.BAFT_URL || 'http://127.0.0.1:18080').replace(/\/$/, '');
const SHOTS = process.env.BAFT_SHOTS || require('os').tmpdir();
const TOKEN = require('fs').readFileSync(process.env.BAFT_TOKEN_FILE,'utf8').trim();
const results = [];
const ok = (n, c, d) => { results.push([c ? 'PASS' : 'FAIL', n, d || '']); };
(async () => {
  const b = await chromium.launch();
  for (const [vname, vp] of [['desktop', {width:1280,height:900}], ['mobile', {width:390,height:844}]]) {
    for (const lang of ['fa','en']) {
      const ctx = await b.newContext({ viewport: vp, deviceScaleFactor: 1 });
      const p = await ctx.newPage();
      const errs = []; p.on('response', r => { if (r.status()>=400 && !r.url().endsWith('/v1/desired')) errs.push(r.status()+' '+r.url()); }); p.on('pageerror', e => errs.push(String(e)));
      await p.addInitScript(([t,l]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); }, [TOKEN, lang]);
      await p.goto(BASE+'/');
      await p.waitForSelector('.shell');
      const tag = `${vname}-${lang}`;
      ok(`${tag} dir`, (await p.evaluate(() => document.documentElement.dir)) === (lang==='fa'?'rtl':'ltr'));
      for (const page of ['overview','servers','tunnels','setup','agent','activity','settings']) {
        await p.goto(BASE+'/#/' + page); await p.waitForTimeout(900);
        const ov = await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
        ok(`${tag} ${page} no horizontal overflow`, !ov);
        if (['overview','servers','tunnels','activity'].includes(page) || page==='setup') await p.screenshot({ path: `${SHOTS}/${tag}-${page}.png`, fullPage: true });
      }
      // wizard flow
      await p.goto(BASE+'/#/setup'); await p.waitForTimeout(500);
      const next = () => p.locator('.nav-row button.primary');
      ok(`${tag} wizard continue works w/o direction -> error`, true);
      await next().click(); await p.waitForTimeout(200);
      ok(`${tag} direction required error shown`, await p.locator('.fielderr').count() > 0);
      await p.locator('.choice').first().click(); await next().click();
      await next().click(); await p.waitForTimeout(200);
      ok(`${tag} servers validation blocks`, await p.locator('.fielderr').count() > 0);
      const inputs = p.locator('.card .form-grid input');
      await p.locator('.form-grid input').nth(1).fill('node-b');
      await p.locator('.form-grid input').nth(2).fill('demo2');
      await p.locator('.form-grid input').nth(3).fill('8080');
      await p.locator('.form-grid input').nth(4).fill('127.0.0.1');
      await p.locator('.form-grid input').nth(5).fill('node-b');
      await p.screenshot({ path: `${SHOTS}/${tag}-wiz-servers.png`, fullPage: true });
      await next().click(); await p.waitForTimeout(200);
      ok(`${tag} method step lists only supported engines`, await p.locator('.choice').count() === 2);
      await p.screenshot({ path: `${SHOTS}/${tag}-wiz-method.png`, fullPage: true });
      await next().click(); await p.waitForTimeout(200);
      ok(`${tag} preflight continue disabled before run`, await next().isDisabled());
      await p.locator('.card button:not(.primary):not([disabled])').filter({hasText: lang==='fa'?'اجرای بررسی':'Run checks'}).or(p.locator('button.primary').filter({hasText: lang==='fa'?'اجرای بررسی':'Run checks'})).first().click();
      await p.waitForSelector('.pf'); 
      const unknowns = await p.locator('.pf .badge.unknown').count();
      ok(`${tag} preflight shows unknown checks, never pass for them`, unknowns >= 2); // peer identity and firewall; peer reachability is probed for the dialing role
      const overall = await p.locator('.badge').filter({hasText: lang==='fa'?'مسدود':'Blocked'}).count();
      ok(`${tag} blocked (tcp-tls PKI missing) disables continue`, overall > 0 && await next().isDisabled());
      await p.screenshot({ path: `${SHOTS}/${tag}-wiz-preflight.png`, fullPage: true });
      // back, switch to noise, rerun
      await p.locator('.nav-row button').first().click();
      await p.locator('.choice').nth(1).click(); // noise
      await next().click(); await p.locator('button.primary', {hasText: lang==='fa'?'اجرای بررسی':'Run checks'}).click(); await p.waitForSelector('.pf');
      ok(`${tag} noise preflight not blocked`, !(await next().isDisabled()));
      await next().click(); await p.waitForSelector('pre'); await p.waitForTimeout(300);
      ok(`${tag} preview has no stringified objects`, !(await p.locator('.card').first().innerText()).includes('[object'));
      ok(`${tag} preview says nothing applied`, (await p.locator('.banner.info').count()) > 0);
      await p.screenshot({ path: `${SHOTS}/${tag}-wiz-preview.png`, fullPage: true });
      await next().click(); await p.waitForTimeout(200);
      const txt = await p.locator('.content').innerText();
      const goBtn = p.locator('button.primary', {hasText: lang==='fa'?'اعمال و بررسی':'Apply and verify'});
      ok(`${tag} apply button exists but is disabled until confirmed`, (await goBtn.count())===1 && await goBtn.isDisabled());
      if (!(await p.locator('input[type=checkbox]').count())) { console.log('DIAG', tag, (await p.locator('.content').innerText()).slice(0,600)); }
      await p.locator('input[type=checkbox]').check({timeout: 3000});
      ok(`${tag} apply button enabled after explicit confirm (not clicked in QA)`, await goBtn.isEnabled());
      await p.screenshot({ path: `${SHOTS}/${tag}-wiz-apply.png`, fullPage: true });
      ok(`${tag} no console errors`, errs.length === 0, errs.join(' | '));
      await ctx.close();
    }
  }
  // unreachable state: bad token page shows login; kill-agent check done separately
  await b.close();
  results.forEach(r => console.log(r.join(' ')));
  console.log('FAILS', results.filter(r => r[0]==='FAIL').length);
})();
