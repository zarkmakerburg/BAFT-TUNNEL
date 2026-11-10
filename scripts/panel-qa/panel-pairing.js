// Panel pairing QA (U5b, ADR-0026): an agent in setup mode for one instance; the wizard prepares
// this server through the root setup helper, exports the bundle, imports the peer bundle from a
// baft-tunnel://pair link with its checksum (wrong checksum blocks Join), joins, and the agent
// reloads so Start appears. Needs: the agent in setup mode, the root helper (or its emulation),
// and a peer bundle prepared with the CLI.
//   BAFT_TOKEN_FILE=<token> PEER_BUNDLE=<peer bundle.json> PEER_CHECKSUM=<checksum> \
//   [BAFT_URL=http://127.0.0.1:18180] [LANG_UI=en|fa] [SHOTS=dir] node panel-pairing.js
const { chromium } = require('playwright');
const fs = require('fs');
const BASE = (process.env.BAFT_URL || 'http://127.0.0.1:18180').replace(/\/$/, '');
const TOKEN = fs.readFileSync(process.env.BAFT_TOKEN_FILE, 'utf8').trim();
const PEER = fs.readFileSync(process.env.PEER_BUNDLE, 'utf8');
const CK = process.env.PEER_CHECKSUM;
const LANG = process.env.LANG_UI || 'en';
const SHOTS = process.env.SHOTS || '';
const results = [];
const ok = (n, c, d) => { results.push([c ? 'PASS' : 'FAIL', n, d || '']); };
const link = 'baft-tunnel://pair?v=1&b=' + Buffer.from(PEER, 'utf8').toString('base64url');
(async () => {
  const b = await chromium.launch();
  const ctx = await b.newContext({ viewport: { width: 390, height: 844 } });
  const p = await ctx.newPage();
  const errs = []; p.on('pageerror', e => errs.push(String(e)));
  await p.addInitScript(([t, l]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); }, [TOKEN, LANG]);
  const shot = async (name) => { if (SHOTS) { await p.screenshot({ path: `${SHOTS}/${LANG}-${name}.png`, fullPage: true }); } };
  await p.goto(BASE + '/#/overview'); await p.waitForSelector('.shell'); await p.waitForTimeout(700);
  const ov = await p.locator('#main').innerText();
  ok('overview shows setup mode, not tunnel data', /no tunnel yet|هنوز تونلی ندارد/.test(ov), ov.slice(0, 200));
  await shot('overview-setup-mode');
  await p.goto(BASE + '/#/setup'); await p.waitForTimeout(700);
  const next = () => p.locator('.nav-row button.primary');
  await p.locator('.choice').nth(0).click(); await next().click(); await p.waitForTimeout(200);
  await p.locator('.choice').nth(1).click(); // serve
  const f = p.locator('.form-grid input');
  await f.nth(0).fill('abroad'); await f.nth(1).fill('iran'); await f.nth(2).fill('qademo'); await f.nth(3).fill('18190');
  await next().click(); await p.waitForTimeout(200);
  await p.locator('.choice').filter({ hasText: 'noise-tcp' }).click();
  await next().click(); await p.waitForTimeout(200);
  await p.locator('.card button.primary').first().click(); // run preflight
  await p.waitForTimeout(1500);
  const pf = await p.locator('#main').innerText();
  ok('preflight checks the root setup helper', /Setup helper|کمک‌کار راه‌اندازی/.test(pf));
  await next().click(); await p.waitForTimeout(1200); await next().click(); await p.waitForTimeout(500);
  const fin = await p.locator('#main').innerText();
  ok('last step offers pairing from the panel', /Pair from this panel|جفت‌سازی از همین پنل/.test(fin), fin.slice(0, 300));
  // a) prepare
  await p.locator('.fin input[type=checkbox]').first().check();
  await p.locator('.fin button.primary').first().click();
  await p.waitForSelector('text=/Prepared|آماده شد/', { timeout: 30000 });
  await p.waitForTimeout(800);
  const exp = await p.locator('#main').innerText();
  ok('export shows the own checksum', /[0-9a-f]{8}-[0-9a-f]{8}-[0-9a-f]{8}-[0-9a-f]{8}/.test(exp));
  ok('export offers copy, link and download', /Copy link|کپی لینک/.test(exp) && /Download file|دانلود فایل/.test(exp));
  await shot('export');
  // c) import from link: wrong checksum first
  await p.locator('.fin textarea').fill(link);
  const ck = p.locator('.fin input[placeholder^=xxxx]');
  await ck.fill('00000000-00000000-00000000-00000000');
  await p.locator('.fin button', { hasText: /Check bundle|بررسی بسته/ }).click();
  await p.waitForTimeout(800);
  const bad = await p.locator('#main').innerText();
  ok('wrong checksum is flagged and Join is not offered', /does not match|نمی‌خواند/.test(bad) && !(await p.locator('.fin button', { hasText: /Pair \(join\)|جفت‌سازی \(join\)/ }).count()));
  await ck.fill(CK);
  await p.locator('.fin button', { hasText: /Check bundle|بررسی بسته/ }).click();
  await p.waitForTimeout(800);
  const good = await p.locator('#main').innerText();
  ok('link decoded and previewed with matching checksum', /Checksum matches|checksum درست است/.test(good) && /qademo/.test(good));
  await shot('import-preview');
  const join = p.locator('.fin button', { hasText: /Pair \(join\)|جفت‌سازی \(join\)/ });
  ok('Join stays disabled until confirmed', await join.isDisabled());
  await p.locator('.fin section input[type=checkbox]').check();
  await join.click();
  await p.waitForSelector('text=/Paired; configuration written|جفت شد/', { timeout: 30000 });
  ok('join applied through the helper', true);
  // the agent reloads; Start appears after Check again
  let started = false;
  for (let i = 0; i < 20 && !started; i++) {
    await p.waitForTimeout(1500);
    const again = p.locator('.fin button', { hasText: /Check again|بررسی دوباره/ });
    if (await again.count()) { await again.click(); await p.waitForTimeout(600); }
    started = (await p.locator('.fin button', { hasText: /^(Start|شروع)/ }).count()) > 0;
  }
  ok('agent reloaded with the configuration and Start is offered', started);
  await shot('after-join');
  ok('no page errors', errs.length === 0, errs.join(';'));
  await b.close();
  for (const r of results) console.log(r.join(' | '));
  process.exit(results.some(r => r[0] === 'FAIL') ? 1 : 0);
})();
