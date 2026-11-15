// Visual + layout QA against a REAL running agent: screenshots of every page and overlay at four
// viewports in Persian RTL and English LTR, plus automatic checks (no horizontal overflow, no console
// or CSP errors, correct dir, sidebar side, no unflipped technical identifiers).
//   BAFT_TOKEN_FILE=<token> [BAFT_URL=http://127.0.0.1:18080] [BAFT_SHOTS=<dir>] [BAFT_THEMES=dark,light] node screenshots.js
const { chromium } = require('playwright');
const fs = require('fs');
const BASE = (process.env.BAFT_URL || 'http://127.0.0.1:18080').replace(/\/$/, '');
const SHOTS = process.env.BAFT_SHOTS || require('os').tmpdir();
const TOKEN = fs.readFileSync(process.env.BAFT_TOKEN_FILE, 'utf8').trim();
const THEMES = (process.env.BAFT_THEMES || 'dark').split(',');
const VIEWPORTS = { desktop: { width: 1440, height: 900 }, laptop: { width: 1100, height: 760 }, tablet: { width: 820, height: 1100 }, mobile: { width: 390, height: 844 } };
const PAGES = ['overview', 'tunnels', 'servers', 'monitoring', 'setup', 'diagnostics', 'activity', 'agent', 'settings'];
const R = [];
const ok = (n, c, d) => R.push([c ? 'PASS' : 'FAIL', n, d || '']);
(async () => {
  fs.mkdirSync(SHOTS, { recursive: true });
  const b = await chromium.launch();
  for (const [vn, vp] of Object.entries(VIEWPORTS)) {
    for (const lang of ['fa', 'en']) {
      for (const theme of THEMES) {
        const tag = `${vn}-${lang}-${theme}`;
        const ctx = await b.newContext({ viewport: vp, deviceScaleFactor: 2, hasTouch: vn === 'mobile', isMobile: vn === 'mobile' });
        const p = await ctx.newPage();
        const errs = [];
        p.on('pageerror', e => errs.push(String(e)));
        p.on('console', m => { if (/Failed to load resource/.test(m.text())) return; if (m.type() === 'error' || /Content Security Policy|Refused/i.test(m.text())) errs.push(m.text()); });
        // 404 is the honest answer for "nothing stored yet"; any other 4xx/5xx is a defect.
        p.on('response', r => { const u = new URL(r.url()); if (r.status() >= 400 && !(r.status() === 404 && ['/v1/desired', '/v1/diagnostics'].includes(u.pathname))) errs.push(r.status() + ' ' + u.pathname); });
        await p.addInitScript(([t, l, th]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); sessionStorage.setItem('theme', th); }, [TOKEN, lang, theme]);
        await p.goto(BASE + '/'); await p.waitForSelector('.shell');
        ok(`${tag} dir`, (await p.evaluate(() => document.documentElement.dir)) === (lang === 'fa' ? 'rtl' : 'ltr'));
        if (vn !== 'tablet' && vn !== 'mobile') {
          const side = await p.evaluate(() => { const r = document.querySelector('.sidebar').getBoundingClientRect(); return { l: Math.round(r.left), r: Math.round(r.right), w: document.documentElement.clientWidth }; });
          ok(`${tag} sidebar on the ${lang === 'fa' ? 'right' : 'left'}`, lang === 'fa' ? side.r === side.w : side.l === 0, JSON.stringify(side));
        }
        for (const page of PAGES) {
          await p.goto(BASE + '/#/' + page); await p.waitForTimeout(900);
          ok(`${tag} ${page} no horizontal overflow`, !(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)));
          ok(`${tag} ${page} has exactly one h1`, (await p.locator('h1').count()) === 1);
          await p.screenshot({ path: `${SHOTS}/${tag}-${page}.png`, fullPage: false });
        }
        // overlays
        await p.goto(BASE + '/#/overview'); await p.waitForTimeout(500);
        await p.keyboard.press('Control+k'); await p.waitForSelector('dialog.ov-palette[open]');
        ok(`${tag} palette opens and focuses its input`, await p.evaluate(() => document.activeElement && document.activeElement.getAttribute('role') === 'combobox'));
        await p.screenshot({ path: `${SHOTS}/${tag}-palette.png` });
        await p.keyboard.press('Escape'); await p.waitForTimeout(150);
        ok(`${tag} palette closes with Escape and unlocks scroll`, (await p.locator('dialog[open]').count()) === 0 && !(await p.evaluate(() => document.documentElement.classList.contains('lock'))));
        await p.locator('.bell').click(); await p.waitForSelector('dialog.ov-popover[open]');
        await p.screenshot({ path: `${SHOTS}/${tag}-notifications.png` });
        await p.keyboard.press('Escape'); await p.waitForTimeout(150);
        if (vn === 'tablet' || vn === 'mobile') {
          await p.locator('.menu-btn').click(); await p.waitForSelector('dialog.ov-drawer[open]'); await p.waitForTimeout(350);
          const edge = await p.evaluate(() => { const r = document.querySelector('dialog.ov-drawer').getBoundingClientRect(); return { l: Math.round(r.left), r: Math.round(r.right), w: document.documentElement.clientWidth }; });
          ok(`${tag} drawer opens from the ${lang === 'fa' ? 'right' : 'left'} edge`, lang === 'fa' ? edge.r === edge.w : edge.l === 0, JSON.stringify(edge));
          ok(`${tag} drawer locks background scroll`, await p.evaluate(() => document.documentElement.classList.contains('lock')));
          await p.screenshot({ path: `${SHOTS}/${tag}-drawer.png` });
          await p.keyboard.press('Escape'); await p.waitForTimeout(150);
          ok(`${tag} drawer closes and returns focus to the menu button`, await p.evaluate(() => document.activeElement && document.activeElement.classList.contains('menu-btn')));
        }
        ok(`${tag} no console/CSP/page errors`, errs.length === 0, errs.slice(0, 3).join(' | '));
        await ctx.close();
      }
    }
  }
  await b.close();
  R.forEach(r => console.log(r.join(' | ')));
  const fails = R.filter(r => r[0] === 'FAIL').length;
  console.log(`SCREENSHOTS_QA fails=${fails} of ${R.length}`);
  process.exit(fails ? 1 : 0);
})();
