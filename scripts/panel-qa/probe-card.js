// Browser QA for the Servers-page health check card (ADR-0052). Needs a running agent:
//   BAFT_TOKEN_FILE=<token file> [BAFT_URL=http://127.0.0.1:18080] node probe-card.js
// Checks: private target refused, consent off by default and needs the checkbox, no external
// provider row without consent, country/role Unknown without a GeoIP database, state survives
// the page's periodic redraw, English and Persian text. It never enables a provider.
const { chromium } = require('playwright');
const BASE = (process.env.BAFT_URL || 'http://127.0.0.1:18080').replace(/\/$/, '');
const TOKEN = require('fs').readFileSync(process.env.BAFT_TOKEN_FILE, 'utf8').trim();
const R = [];
const ok = (n, c, d) => R.push([c ? 'PASS' : 'FAIL', n, d || '']);
(async () => {
  const b = await chromium.launch({ executablePath: process.env.CHROME_BIN || undefined });
  for (const lang of ['en', 'fa']) {
    const ctx = await b.newContext({ viewport: { width: 390, height: 1400 } });
    const p = await ctx.newPage();
    const errs = []; p.on('pageerror', e => errs.push(String(e)));
    await p.addInitScript(([t, l]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); }, [TOKEN, lang]);
    await p.goto(BASE + '/#/servers'); await p.waitForSelector('.shell'); await p.waitForTimeout(800);
    const title = lang === 'en' ? 'Check a server before adding it' : 'بررسی سرور پیش از افزودن';
    const card = p.locator('section.card', { hasText: title });
    ok(lang + ' card is shown', await card.count() === 1);
    ok(lang + ' consent checkbox is off by default', !(await card.locator('input[type=checkbox]').isChecked()));
    await card.locator('input').first().fill('10.0.0.1');
    await card.locator('button.primary').click(); await p.waitForTimeout(600);
    ok(lang + ' private target is refused', await card.locator('.error').count() === 1);
    await card.locator('input').first().fill('192.0.0.9'); await card.locator('input').nth(1).fill('9');
    await card.locator('button.primary').click();
    await card.locator('h3', { hasText: lang === 'en' ? 'Result' : 'نتیجه' }).waitFor({ timeout: 15000 });
    const body = await card.innerText();
    ok(lang + ' country is Unknown without a database', body.includes(lang === 'en' ? 'Unknown' : 'نامشخص'));
    ok(lang + ' external provider is off without consent', body.includes(lang === 'en' ? 'Off (no consent).' : 'خاموش (اجازه داده نشده).'));
    await p.waitForTimeout(6000); // a periodic page redraw must not wipe the result
    ok(lang + ' result survives a page redraw', (await card.innerText()).includes(lang === 'en' ? 'Result' : 'نتیجه'));
    ok(lang + ' no page errors', errs.length === 0, errs.join('; '));
    await ctx.close();
  }
  await b.close();
  for (const r of R) console.log(r.join(' '));
  if (R.some(r => r[0] === 'FAIL')) process.exit(1);
  console.log('PROBE_CARD_PANEL_TEST=PASS');
})().catch(e => { console.error(e); process.exit(1); });
