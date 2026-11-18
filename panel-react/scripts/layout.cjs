// Layout audit: no horizontal overflow, one scroll container per page, content never hidden behind fixed bars.
// node scripts/layout.cjs BASE_URL TOKEN [OUTDIR]  -> prints JSON, exits 1 on any finding.
// Realistic data is injected by mocking GET /v1/* responses in the browser (the agent itself is untouched).
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const fs = require('fs');
const [base, token, out] = process.argv.slice(2);
const ROUTES = ['overview', 'tunnels', 'servers', 'monitoring', 'setup', 'diagnostics', 'activity', 'agent', 'settings'];
const VIEWS = { 'desktop-1440': [1440, 900], 'desktop-1024': [1024, 700], 'tablet-820': [820, 1100], 'tablet-768-land': [1024, 768], 'mobile-390': [390, 844], 'mobile-360-short': [360, 640], 'mobile-320': [320, 568], 'mobile-land': [844, 390], 'desktop-short': [1280, 600] };
const now = Math.floor(Date.now() / 1000);
const samples = Array.from({ length: 300 }, (_, i) => ({ t: now - (300 - i) * 5, up: i % 40 !== 7, rtt_ms: 40 + (i % 17) }));
const longS = Array.from({ length: 200 }, (_, i) => ({ t: now - (200 - i) * 60, probes: 12, up: 11, from_peer_bytes: 1e6 + i * 1e4, to_peer_bytes: 2e6 }));
const mock = {
  '/v1/history': { interval_s: 5, capacity: 720, samples, alerts: [], long: { samples: longS, totals: { probes: 2400, up: 2200, from_peer_bytes: 5e8, to_peer_bytes: 9e8 }, bucket_s: 60, retention_s: 86400, persisted: true }, diagnostics_runs: [] },
  '/v1/audit': { entries: Array.from({ length: 40 }, (_, i) => ({ ts: now - i * 90, result: i % 5 ? 'accepted' : 'rejected', resource: 'desired_state', actor: { fingerprint: 'abcdef0123456789' }, target: { node: 'node-iran', instance: 'demo-tunnel-with-a-long-instance-name' }, receipt_id: 'r' + 'x'.repeat(40) + i, detail: 'a-very-long-detail-value-without-spaces-' + 'y'.repeat(60) })) },
  '/v1/traffic': { available: true, fresh: true, bytes_from_peer: 123456789, bytes_to_peer: 987654321, rate: { from_peer_bps: 12345, to_peer_bps: 54321 }, streams_total: 12, sessions_total: 3, sampled_unix: now, sampling_interval_s: 5, history: Array.from({ length: 60 }, (_, i) => ({ t: now - (60 - i) * 5, from_peer_bps: 1e4 + i * 100, to_peer_bps: 2e4 })) },
  '/v1/diagnostics': { at_unix: now - 30, verified: false, tiers: ['tcp_reachability', 'secure_channel', 'identity', 'authorization', 'session', 'data_path', 'integrity'].map((id) => ({ id, result: id === 'integrity' ? 'unknown' : 'pass', source: 'active_tcp_probe', evidence: 'connect to 203.0.113.10:8443 succeeded in 41 ms (an intentionally long evidence line to test wrapping behaviour) ' + 'z'.repeat(30), next: 'Run the end-to-end integrity probe from this page to confirm the data path carries bytes intact.' })), next_action: { action: 'check', text: 'x' } },
};
(async () => {
  if (out) fs.mkdirSync(out, { recursive: true });
  const b = await chromium.launch();
  const findings = [];
  const summary = {};
  for (const [vn, [w, h]] of Object.entries(VIEWS)) {
    for (const lang of ['fa', 'en']) {
      const ctx = await b.newContext({ viewport: { width: w, height: h }, hasTouch: w < 900, isMobile: w < 900 });
      await ctx.addInitScript(([t, l]) => { sessionStorage.setItem('token', t); sessionStorage.setItem('lang', l); sessionStorage.setItem('theme', 'dark'); }, [token, lang]);
      const page = await ctx.newPage();
      await page.route('**/v1/**', async (rt) => {
        const u = new URL(rt.request().url()).pathname;
        if (rt.request().method() === 'GET' && mock[u]) return rt.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(mock[u]) });
        return rt.continue();
      });
      for (const r of ROUTES) {
        await page.goto(`${base}/next/#/${r}`, { waitUntil: 'networkidle' });
        await page.waitForSelector('.content:not([aria-busy=true]) .view', { timeout: 8000 }).catch(() => {});
        await page.waitForTimeout(450);
        const res = await page.evaluate(() => {
          const vw = document.documentElement.clientWidth, vh = window.innerHeight;
          const bad = [], scrollers = [];
          const sel = (e) => e.tagName.toLowerCase() + (e.className && typeof e.className === 'string' ? '.' + e.className.trim().split(/\s+/).join('.') : '');
          for (const e of document.querySelectorAll('body *')) {
            const cs = getComputedStyle(e);
            if (cs.display === 'none' || cs.position === 'fixed' && e.closest('dialog')) continue;
            const r = e.getBoundingClientRect();
            if (r.width && !e.closest('.table-wrap,pre,dialog,.sr,.tip-box,.skip,.toasts') && (r.right > vw + 1 || r.left < -1)) bad.push(sel(e) + ` [${Math.round(r.left)},${Math.round(r.right)}]`);
            const ox = cs.overflowX, oy = cs.overflowY;
            const sc = (ox === 'auto' || ox === 'scroll') && e.scrollWidth > e.clientWidth + 1 || (oy === 'auto' || oy === 'scroll') && e.scrollHeight > e.clientHeight + 1;
            if (sc && !e.closest('dialog')) scrollers.push(sel(e));
          }
          const root = document.scrollingElement;
          window.scrollTo(0, root.scrollHeight);
          const last = document.querySelector('.view')?.lastElementChild || document.querySelector('.view');
          const lr = last ? last.getBoundingClientRect() : null;
          const tab = document.querySelector('.tabbar');
          const tabTop = tab && getComputedStyle(tab).display !== 'none' ? tab.getBoundingClientRect().top : vh;
          const top = document.querySelector('.topbar').getBoundingClientRect();
          const out = { vw, sw: root.scrollWidth, docH: root.scrollHeight, vh, bad: [...new Set(bad)].slice(0, 6), scrollers: [...new Set(scrollers)], lastBottom: lr ? Math.round(lr.bottom) : null, tabTop: Math.round(tabTop), topbarTop: Math.round(top.top), htmlLock: document.documentElement.classList.contains('lock'), smallInputs: [...document.querySelectorAll('input:not([type=radio]),select')].filter((e) => getComputedStyle(e).fontSize !== '' && parseFloat(getComputedStyle(e).fontSize) < 16).length, bodyOverflow: getComputedStyle(document.body).overflow };
          window.scrollTo(0, 0);
          return out;
        });
        const key = `${vn}-${lang}-${r}`;
        summary[key] = res;
        if (res.sw > res.vw + 1) findings.push(`${key}: page scrolls horizontally (${res.sw}>${res.vw})`);
        if (res.bad.length) findings.push(`${key}: elements outside viewport: ${res.bad.join(', ')}`);
        if (res.scrollers.length) findings.push(`${key}: nested scroll containers: ${res.scrollers.join(', ')}`);
        if (res.lastBottom !== null && res.lastBottom > res.tabTop + 1) findings.push(`${key}: last content (${res.lastBottom}) hidden behind bottom bar (${res.tabTop})`);
        if (res.topbarTop !== 0) findings.push(`${key}: sticky top bar not at top while scrolled (${res.topbarTop})`);
        if (w < 900 && res.smallInputs) findings.push(`${key}: ${res.smallInputs} form fields under 16px (iOS zooms the page on focus)`);
        if (res.htmlLock) findings.push(`${key}: html.lock stuck (page cannot scroll)`);
        if (out && (vn === 'mobile-390' || vn === 'tablet-820' || vn === 'desktop-1440') && lang === 'fa') await page.screenshot({ path: `${out}/${vn}-${lang}-${r}.png`, fullPage: false });
      }
      await ctx.close();
    }
  }
  await b.close();
  console.log(JSON.stringify({ findings, count: Object.keys(summary).length }, null, 1));
  if (process.env.DUMP) fs.writeFileSync(process.env.DUMP, JSON.stringify(summary, null, 1));
  process.exit(findings.length ? 1 : 0);
})();
