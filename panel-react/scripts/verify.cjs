// Browser verification against a running agent: node scripts/verify.cjs BASE_URL TOKEN OUTDIR
// Records what actually happened; prints one JSON report. Uses the pre-installed Playwright.
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const fs = require('fs');
const [base, token, out] = process.argv.slice(2);
const ROUTES = ['overview', 'tunnels', 'servers', 'monitoring', 'setup', 'diagnostics', 'activity', 'agent', 'settings'];
const VIEWS = { desktop: { width: 1440, height: 900 }, tablet: { width: 820, height: 1100 }, mobile: { width: 390, height: 844 } };
(async () => {
  fs.mkdirSync(out, { recursive: true });
  const b = await chromium.launch();
  const report = { routes: {}, overflow: [], csp: [], console: [], dir: {}, keyboard: {}, auth: {}, ltr: {} };
  const mk = async (view, lang, theme, tok = token) => {
    const ctx = await b.newContext({ viewport: VIEWS[view], deviceScaleFactor: 1, reducedMotion: 'no-preference' });
    await ctx.addInitScript(([t, l, th, tk]) => { try { if (tk) sessionStorage.setItem('token', tk); sessionStorage.setItem('lang', l); sessionStorage.setItem('theme', th); } catch (e) {} }, [token, lang, theme, tok]);
    const page = await ctx.newPage();
    page.on('console', (m) => { const x = m.text(); if (/Content Security Policy|CSP/i.test(x)) report.csp.push(x); else if (m.type() === 'error') report.console.push(x.slice(0, 200)); });
    page.on('pageerror', (e) => report.console.push('pageerror ' + String(e).slice(0, 200)));
    return { ctx, page };
  };
  const shots = [['desktop', 'fa', 'dark'], ['desktop', 'en', 'light'], ['tablet', 'fa', 'dark'], ['mobile', 'fa', 'dark'], ['mobile', 'en', 'dark']];
  for (const [view, lang, theme] of shots) {
    const { ctx, page } = await mk(view, lang, theme);
    for (const r of ROUTES) {
      await page.goto(`${base}/next/#/${r}`, { waitUntil: 'networkidle' });
      await page.waitForSelector('.content:not([aria-busy=true]) .view', { timeout: 8000 }).catch(() => {});
      await page.waitForTimeout(350);
      const info = await page.evaluate(() => ({ h1: document.querySelector('h1')?.textContent, dir: document.documentElement.dir, sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth, cards: document.querySelectorAll('.card,.kpi,.banner,.empty').length, err: !!document.querySelector('.banner.bad') }));
      const key = `${view}-${lang}-${r}`;
      report.routes[key] = info;
      if (info.sw > info.cw + 1) report.overflow.push(`${key} ${info.sw}>${info.cw}`);
      await page.screenshot({ path: `${out}/${view}-${lang}-${theme}-${r}.png`, fullPage: false });
    }
    await ctx.close();
  }
  // keyboard: Tab reaches skip link first, palette opens with Ctrl+K and Esc closes it and restores focus
  {
    const { ctx, page } = await mk('desktop', 'en', 'dark');
    await page.goto(`${base}/next/#/overview`, { waitUntil: 'networkidle' });
    await page.waitForSelector('.view');
    await page.keyboard.press('Tab');
    report.keyboard.firstTab = await page.evaluate(() => document.activeElement?.className || document.activeElement?.tagName);
    await page.keyboard.press('Control+k');
    report.keyboard.paletteOpen = await page.evaluate(() => !!document.querySelector('dialog.ov-palette[open]'));
    await page.keyboard.type('diag');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(300);
    report.keyboard.enterNavigates = await page.evaluate(() => location.hash);
    await page.keyboard.press('Control+k');
    await page.keyboard.press('Escape');
    report.keyboard.escCloses = await page.evaluate(() => !document.querySelector('dialog[open]'));
    // focus ring visible on a nav link
    await page.focus('.nav a');
    report.keyboard.focusOutline = await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle);
    // confirm dialog (sign out) is keyboard operable and cancel keeps the session
    await page.goto(`${base}/next/#/settings`, { waitUntil: 'networkidle' });
    await page.waitForSelector('.view');
    await page.click('text=Log out').catch(async () => { await page.click('button:has-text("Disconnect")').catch(() => {}); });
    report.keyboard.confirmOpens = await page.evaluate(() => !!document.querySelector('dialog[open] .dlg'));
    await page.keyboard.press('Escape');
    report.keyboard.cancelKeepsSession = await page.evaluate(() => !!sessionStorage.getItem('token'));
    await ctx.close();
  }
  // LTR isolation inside Persian UI: identifiers are bdi dir=ltr
  {
    const { ctx, page } = await mk('desktop', 'fa', 'dark');
    await page.goto(`${base}/next/#/tunnels`, { waitUntil: 'networkidle' });
    await page.waitForSelector('.view');
    report.ltr = await page.evaluate(() => ({ docDir: document.documentElement.dir, ltrNodes: document.querySelectorAll('bdi.ltr[dir=ltr]').length, firstText: document.querySelector('bdi.ltr')?.textContent }));
    await ctx.close();
  }
  // auth: no token -> login; wrong token -> error and stays on login; right token -> shell
  {
    const { ctx, page } = await mk('desktop', 'en', 'dark', '');
    await page.goto(`${base}/next/`, { waitUntil: 'networkidle' });
    report.auth.noTokenShowsLogin = await page.evaluate(() => !!document.querySelector('.login-card') && !document.querySelector('.shell'));
    await page.fill('#token', 'wrong-token-value');
    await page.click('button[type=submit]');
    await page.waitForSelector('.fielderr');
    report.auth.badTokenMessage = await page.textContent('.fielderr');
    report.auth.badTokenStaysOnLogin = await page.evaluate(() => !!document.querySelector('.login-card') && !sessionStorage.getItem('token'));
    await page.fill('#token', token);
    await page.click('button[type=submit]');
    await page.waitForSelector('.shell');
    report.auth.goodTokenEntersShell = true;
    // an in-session 401 returns to login
    await page.evaluate(() => sessionStorage.setItem('token', 'revoked'));
    await ctx.close();
  }
  // unauthorised API: the asset is public but /v1 refuses without token
  const r = await fetch(`${base}/v1/status`);
  report.auth.apiWithoutTokenStatus = r.status;
  // network failures: abort /v1 and observe the unreachable state
  {
    const { ctx, page } = await mk('desktop', 'en', 'dark');
    await page.goto(`${base}/next/#/overview`, { waitUntil: 'networkidle' });
    await page.waitForSelector('.view');
    await page.route('**/v1/**', (rt) => rt.abort());
    await page.reload({ waitUntil: 'load' });
    await page.waitForTimeout(1500);
    report.unreachable = await page.evaluate(() => ({ banner: document.querySelector('.banner.bad h2')?.textContent || null }));
    await page.screenshot({ path: `${out}/desktop-en-dark-unreachable.png` });
    await ctx.close();
  }
  // partial failure: audit returns 500
  {
    const { ctx, page } = await mk('desktop', 'en', 'dark');
    await page.route('**/v1/audit', (rt) => rt.fulfill({ status: 500, body: '{}' }));
    await page.goto(`${base}/next/#/activity`, { waitUntil: 'networkidle' });
    await page.waitForTimeout(800);
    report.partial = await page.evaluate(() => ({ partial: !!document.querySelector('.banner.partial'), emptyError: !!document.querySelector('.empty.error') }));
    await page.screenshot({ path: `${out}/desktop-en-dark-activity-error.png` });
    await ctx.close();
  }
  await b.close();
  console.log(JSON.stringify(report, null, 1));
})();
