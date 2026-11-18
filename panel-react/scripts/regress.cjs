// Regression checks: node scripts/regress.cjs BASE_URL TOKEN  (exits 1 on failure)
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const [base, token] = process.argv.slice(2);
const fails = [];
const ok = (c, m) => { if (!c) fails.push(m); };
(async () => {
  const b = await chromium.launch();
  const mk = async (w, h, mobile, lang) => {
    const c = await b.newContext({ viewport: { width: w, height: h }, isMobile: mobile, hasTouch: mobile });
    await c.addInitScript(([t, l]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); }, [token, lang]);
    const p = await c.newPage(); p.on('pageerror', (e) => fails.push('pageerror: ' + e.message));
    return p;
  };
  for (const [lang, rtl] of [['fa', true], ['en', false]]) {
    const p = await mk(390, 844, true, lang);
    await p.goto(base + '/next/#/overview'); await p.waitForSelector('.menu-btn');
    await p.click('.menu-btn'); await p.waitForTimeout(500);
    const r = await p.evaluate(() => document.querySelector('dialog.ov-drawer').getBoundingClientRect());
    ok(rtl ? r.right >= 389 : r.left <= 1, `drawer opens from the ${rtl ? 'right' : 'left'} in ${lang}`);
  }
  const p = await mk(1280, 800, false, 'fa');
  await p.goto(base + '/next/#/overview'); await p.waitForSelector('.nav a');
  await p.keyboard.press('Control+b'); await p.waitForTimeout(300);
  for (const r of ['tunnels', 'monitoring', 'setup', 'settings']) {
    await p.click(`.nav a[href="#/${r}"]`); await p.waitForTimeout(700);
    ok(await p.evaluate(() => document.querySelector('.content')?.innerText.length > 50), `rail: ${r} is not blank`);
  }
  await p.goto(base + '/next/#/setup'); await p.waitForSelector('.stepper');
  await p.click('text=/بعدی|Next/'); await p.waitForTimeout(300);
  ok(await p.locator('input[name=loc]').count() === 2, 'location question present');
  ok(await p.locator('input[name=direction]').count() === 0, 'nothing else before location is chosen');
  await p.locator('input[name=loc]').first().check(); await p.waitForTimeout(200);
  ok(await p.locator('input[name=role][value=forward]').isChecked(), 'Iran defaults to forward');
  ok(await p.locator('select').count() >= 1, 'peer selector present');
  await b.close();
  console.log(JSON.stringify({ fails }, null, 1)); process.exit(fails.length ? 1 : 0);
})();
