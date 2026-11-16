// First-run flows in a REAL browser against a REAL two-host LAB pair (R4): the setup wizard on both
// hosts, PKI enrollment through the panel (tcp-tls/baft: CSR created by the root PKI helper, signed by
// an offline CA outside the browser, certificate/CA/CRL uploaded through the panel), Prepare (init),
// own bundle download, peer bundle upload with a wrong then the right checksum, Join, agent reload and
// Start, all through the panel. Run by lab/real-hosts/panel-pairing-stage.sh with LAB_PANEL_BROWSER=1
// through SSH port forwards to each host's loopback agent. The shell stage verifies host state after.
// Env: IRAN_URL ABROAD_URL IRAN_TOKEN_FILE ABROAD_TOKEN_FILE ENGINE IRAN_INST ABROAD_INST CONF_DIR
//      PEER_HOST IRAN_SAN ABROAD_SAN MSVC MTP OUT_DIR [SIGN_CMD] [LANG_UI=en|fa]
// Host addresses are secrets of the LAB: they are never printed (details are masked).
const { chromium } = require('playwright');
const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const E = process.env;
const ENGINE = E.ENGINE;
const X509 = ENGINE === 'tcp-tls' || ENGINE === 'baft';
const LANG = E.LANG_UI || 'en';
const OUT = E.OUT_DIR;
const SIDES = {
  abroad: { url: E.ABROAD_URL.replace(/\/$/, ''), token: fs.readFileSync(E.ABROAD_TOKEN_FILE, 'utf8').trim(), inst: E.ABROAD_INST, node: 'abroad', peer: 'iran', role: 'serve', san: E.ABROAD_SAN },
  iran: { url: E.IRAN_URL.replace(/\/$/, ''), token: fs.readFileSync(E.IRAN_TOKEN_FILE, 'utf8').trim(), inst: E.IRAN_INST, node: 'iran', peer: 'abroad', role: 'forward', san: E.IRAN_SAN },
};
const SECRETS = [E.PEER_HOST, E.IRAN_SAN, E.ABROAD_SAN, SIDES.abroad.token, SIDES.iran.token].filter(Boolean);
const mask = s => { s = String(s); for (const x of SECRETS) s = s.split(x).join('<masked>'); return s.replace(/\b\d{1,3}(\.\d{1,3}){3}\b/g, '<ip>'); };
const R = [];
const ok = (n, c, d) => { R.push([c ? 'PASS' : 'FAIL', n, c ? '' : mask(d || '').slice(0, 600)]); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const until = async (fn, ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await fn()) return true; await sleep(1000); } return false; };
const CK_RE = /[0-9a-f]{8}-[0-9a-f]{8}-[0-9a-f]{8}-[0-9a-f]{8}/;

(async () => {
  fs.mkdirSync(path.join(OUT, 'csr'), { recursive: true });
  const b = await chromium.launch();
  const S = {};
  try {
    for (const side of ['abroad', 'iran']) {
      const s = SIDES[side];
      const ctx = await b.newContext({ viewport: { width: 1280, height: 900 }, acceptDownloads: true, locale: LANG === 'fa' ? 'fa-IR' : 'en-US' });
      await ctx.addInitScript(l => { sessionStorage.setItem('lang', l); }, LANG);
      const p = await ctx.newPage();
      const st = { p, errs: [], bad: [], api: [] };
      p.on('pageerror', e => st.errs.push(String(e)));
      p.on('console', m => { if (/Content Security Policy/i.test(m.text())) st.bad.push('CSP ' + m.text()); });
      p.on('response', r => { const u = new URL(r.url()); if (u.pathname.startsWith('/v1/')) { st.api.push({ p: u.pathname, s: r.status() }); if (r.status() >= 500) st.bad.push(r.status() + ' ' + u.pathname); } });
      S[side] = st;
      // Real sign-in: the operator pastes the token into the login field.
      await p.goto(s.url + '/');
      await p.fill('#token', s.token); await p.press('#token', 'Enter'); await p.waitForSelector('.shell');
      await p.goto(s.url + '/#/setup'); await p.waitForTimeout(1200);
      const main = () => p.locator('#main').innerText();
      const next = () => p.locator('.nav-row button.primary');
      st.main = main; st.next = next;
      await p.locator('.choice').nth(0).click(); await next().click(); await p.waitForTimeout(300); // Direct
      await p.locator('.choice').nth(side === 'iran' ? 0 : 1).click(); // forward | serve
      const f = n => p.locator(`input[name="${n}"]`);
      await f('node').fill(s.node); await f('peerId').fill(s.peer); await f('tunnel').fill('ppair'); await f('port').fill(E.MSVC);
      if (side === 'iran') { await f('peerHost').fill(E.PEER_HOST); if (await f('serverName').count()) await f('serverName').fill(E.PEER_HOST); }
      if (await f('localServerName').count()) await f('localServerName').fill(X509 ? s.san : 'unused.invalid');
      const adv = p.locator('details', { hasText: /Advanced|پیشرفته/ }).first();
      if (await adv.count()) await adv.evaluate(d => { d.open = true; });
      await f('tport').fill(E.MTP); await f('dir').fill(E.CONF_DIR); await f('inst').fill(s.inst);
      ok(side + ': servers step accepts the plan', await next().isEnabled(), await main());
      await next().click(); await p.waitForTimeout(300);
      const eng = p.locator('.choice').filter({ hasText: ENGINE });
      ok(side + ': engine ' + ENGINE + ' is offered and selectable', (await eng.count()) >= 1 && !(await eng.first().locator('input').isDisabled()), await main());
      await eng.first().click();
      await next().click(); await p.waitForTimeout(300);
      await p.locator('.card button.primary').first().click(); // run preflight
      await until(async () => /Overall|وضعیت کلی/.test(await main()) && !/Checking…|در حال بررسی/.test(await main()), 60000);
      const pf = await main();
      ok(side + ': preflight ran through the setup helper and is not blocked', /Setup helper|کمک‌کار راه‌اندازی/.test(pf) && await next().isEnabled(), pf);
      if (ENGINE === 'baft') ok(side + ': preflight shows the engine store check', /Engine Store|engine store|انبار موتور/i.test(pf), pf);
      await next().click(); await until(async () => await next().isEnabled(), 30000); // preview
      await next().click(); await p.waitForTimeout(800); // run / pairing
      const fin = await main();
      ok(side + ': last step offers pairing from the panel', /Pair from this panel|جفت‌سازی از همین پنل/.test(fin), fin);
    }

    if (X509) {
      for (const side of ['abroad', 'iran']) {
        const { p, main } = S[side];
        const prep = p.locator('button.primary').filter({ hasText: /Prepare and verify|آماده‌سازی و بررسی/ });
        const c = p.locator('.pair-a-confirm input'); if (await c.count()) await c.check();
        ok(side + ': Prepare stays locked until certificates are installed', await prep.isDisabled());
        await p.locator('button').filter({ hasText: /Create CSR|ساخت CSR/ }).first().click();
        const dl = p.locator('button').filter({ hasText: /Download CSR|دانلود CSR/ });
        await dl.waitFor({ timeout: 90000 }).catch(() => {});
        ok(side + ': root PKI helper created the CSR', (await dl.count()) === 1, await main());
        if (await dl.count()) {
          const [d] = await Promise.all([p.waitForEvent('download'), dl.click()]);
          await d.saveAs(path.join(OUT, 'csr', side + '.csr'));
        }
        const csr = fs.existsSync(path.join(OUT, 'csr', side + '.csr')) ? fs.readFileSync(path.join(OUT, 'csr', side + '.csr'), 'utf8') : '';
        ok(side + ': downloaded CSR holds no private key', /BEGIN CERTIFICATE REQUEST/.test(csr) && !/PRIVATE KEY/.test(csr));
      }
      // Offline CA outside the browser (the stage runs it in a network-isolated container).
      execFileSync('bash', ['-c', E.SIGN_CMD], { stdio: ['ignore', 'ignore', 'inherit'] });
      for (const side of ['abroad', 'iran']) {
        const { p, main } = S[side];
        const dir = path.join(OUT, 'signed', side);
        const files = p.locator('.pki-upload input[type=file]');
        ok(side + ': three public uploads, no key upload', await files.count() === 3);
        await files.nth(0).setInputFiles(path.join(dir, 'node.crt'));
        await files.nth(1).setInputFiles(path.join(dir, 'ca.crt'));
        await files.nth(2).setInputFiles(path.join(dir, 'ca.crl'));
        await p.waitForFunction(() => document.querySelectorAll('.pki-upload .sub').length >= 3, null, { timeout: 10000 }).catch(() => {});
        await p.locator('.pki-confirm input').check();
        const inst = p.locator('button').filter({ hasText: /Validate and install|اعتبارسنجی و نصب/ });
        const [resp] = await Promise.all([
          p.waitForResponse(r => new URL(r.url()).pathname === '/v1/pki/certificate/install', { timeout: 120000 }).catch(() => null),
          inst.click(),
        ]);
        await p.waitForTimeout(800);
        const fp = execFileSync('openssl', ['x509', '-in', path.join(dir, 'node.crt'), '-noout', '-fingerprint', '-sha256']).toString().split('=')[1].replace(/:/g, '').trim().toLowerCase();
        ok(side + ': root PKI helper installed the certificate', resp && resp.status() === 200, resp ? String(resp.status()) : 'no response');
        ok(side + ': panel shows the installed certificate fingerprint', (await main()).toLowerCase().includes(fp), await main());
      }
    }

    for (const side of ['abroad', 'iran']) {
      const { p, main } = S[side];
      const c = p.locator('.pair-a-confirm input'); if (await c.count() && !(await c.isChecked())) await c.check();
      const prep = p.locator('button.primary').filter({ hasText: /Prepare and verify|آماده‌سازی و بررسی/ });
      ok(side + ': Prepare is enabled', await prep.isEnabled(), await main());
      await prep.click();
      const prepared = await until(async () => CK_RE.test(await main()) && /Prepared|آماده شد/.test(await main()), 120000);
      ok(side + ': Prepare (init) applied by the root helper and the checksum is shown', prepared, await main());
      const ck = ((await main()).match(CK_RE) || [''])[0];
      fs.writeFileSync(path.join(OUT, 'ck-' + side + '.txt'), ck + '\n');
      const dl = p.locator('.fin button', { hasText: /Download file|دانلود فایل/ });
      const [d] = await Promise.all([p.waitForEvent('download'), dl.click()]);
      await d.saveAs(path.join(OUT, 'bundle-' + side + '.json'));
      const bundle = fs.readFileSync(path.join(OUT, 'bundle-' + side + '.json'), 'utf8');
      ok(side + ': downloaded bundle is public (no key material)', /"version"/.test(bundle) && !/PRIVATE KEY/.test(bundle));
    }

    for (const side of ['abroad', 'iran']) {
      const { p, main } = S[side];
      const peer = SIDES[side].peer;
      const peerCk = fs.readFileSync(path.join(OUT, 'ck-' + peer + '.txt'), 'utf8').trim();
      await p.locator('.fin input[type=file][accept^=".json"]').setInputFiles(path.join(OUT, 'bundle-' + peer + '.json'));
      await p.waitForTimeout(500);
      const ck = p.locator('.fin input[placeholder^=xxxx]');
      const check = p.locator('.fin button', { hasText: /Check bundle|بررسی بسته/ });
      const join = p.locator('.fin button', { hasText: /Pair \(join\)|جفت‌سازی \(join\)/ });
      await ck.fill('00000000-00000000-00000000-00000000'); await check.click(); await p.waitForTimeout(1500);
      ok(side + ': wrong checksum is flagged and Join is not offered', /does not match|نمی‌خواند/.test(await main()) && (await join.count()) === 0, await main());
      await ck.fill(peerCk); await check.click(); await p.waitForTimeout(1500);
      ok(side + ': peer bundle previewed with matching checksum', /Checksum matches|checksum درست است/.test(await main()) && (await join.count()) === 1, await main());
      ok(side + ': Join stays disabled until confirmed', await join.isDisabled());
      await p.locator('.fin section input[type=checkbox]').check();
      await join.click();
      ok(side + ': Join applied through the root helper', await until(async () => /Paired; configuration written|جفت شد/.test(await main()), 120000), await main());
    }

    for (const side of ['abroad', 'iran']) { // serve first, then forward
      const { p, main } = S[side];
      let start = null;
      await until(async () => {
        const again = p.locator('.fin button', { hasText: /Check again|بررسی دوباره/ });
        if (await again.count()) { await again.click().catch(() => {}); await sleep(800); }
        const s = p.locator('.fin button', { hasText: /^(Start|شروع)/ });
        if (await s.count()) { start = s.first(); return true; }
        return false;
      }, 90000);
      ok(side + ': agent reloaded by itself and the wizard offers Start', !!start, await main());
      if (!start) continue;
      const life = p.waitForResponse(r => new URL(r.url()).pathname === '/v1/tunnel/lifecycle', { timeout: 120000 }).catch(() => null);
      await start.click();
      const dlg = p.locator('dialog[open]');
      if (await dlg.waitFor({ timeout: 5000 }).then(() => true, () => false)) await dlg.locator('button.danger, button.primary').click();
      const resp = await life;
      let body = null; try { body = resp && await resp.json(); } catch (e) { body = null; }
      ok(side + ': Start from the wizard was applied by the agent', resp && resp.status() === 200 && body && body.result === 'applied', JSON.stringify({ status: resp && resp.status(), body }));
      // The unit must really run: ask the agent (same token, same forward) until it reports active.
      const unitUp = await until(async () => {
        try {
          const r = await fetch(SIDES[side].url + '/v1/status', { headers: { authorization: 'Bearer ' + SIDES[side].token } });
          const j = await r.json(); return j.tunnel && j.tunnel.tiers && j.tunnel.tiers.unit && j.tunnel.tiers.unit.result === 'active';
        } catch (e) { return false; }
      }, 60000);
      ok(side + ': agent reports the tunnel unit active after Start', unitUp);
    }

    for (const side of ['abroad', 'iran']) {
      const st = S[side];
      const html = await st.p.content();
      ok(side + ': token never shown in the page', !html.includes(SIDES[side].token));
      ok(side + ': no 5xx and no CSP violation', st.bad.length === 0, st.bad.join('; '));
      ok(side + ': no page errors', st.errs.length === 0, st.errs.join('; '));
    }
  } catch (e) {
    ok('browser flow ran to the end', false, e && e.message);
  } finally {
    await b.close();
  }
  for (const r of R) console.log('LAB_PANEL_PAIRING ' + r[0] + ' | ' + r[1] + (r[2] ? ' | ' + r[2] : ''));
  const failed = R.filter(r => r[0] === 'FAIL').length;
  console.log(`LAB_PANEL_PAIRING_BROWSER engine=${ENGINE} lang=${LANG} checks=${R.length} failed=${failed}`);
  process.exit(failed ? 1 : 0);
})();
