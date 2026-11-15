// Panel in a real browser against a REAL installed two-host LAB pair (agent services, protocol v2).
// Run by lab/real-hosts/agent-service-stage.sh with LAB_PANEL_BROWSER=1 through an SSH port forward
// to the forward node's loopback agent. Covers what local QA could not: the integrity probe on a v2
// pair, and lifecycle Restart / Stop / Start through the confirm dialog on a live tunnel.
// BAFT_AGENT_URL=http://127.0.0.1:PORT BAFT_TOKEN_FILE=/path/token node lab-panel-live.js
const { chromium } = require('playwright');
const fs = require('fs');
const base = process.env.BAFT_AGENT_URL;
const token = fs.readFileSync(process.env.BAFT_TOKEN_FILE, 'utf8').trim();
const SHOTS = process.env.BAFT_SHOTS || require('os').tmpdir();
const READY = /\b(ready|production[- ]ready|verified end-to-end)\b|آماده\s*(است|به کار)/i;
const api = async (m, p, body) => {
  const r = await fetch(base + p, { method: m, headers: { authorization: 'Bearer ' + token, 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
  return r.json();
};
const sleep = ms => new Promise(r => setTimeout(r, ms));
const unit = async () => (await api('GET', '/v1/status')).tunnel.tiers.unit;
const until = async (fn, ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await fn()) return true; await sleep(1000); } return false; };
(async () => {
  const R = [], ok = (n, c, extra) => { R.push([c ? 'PASS' : 'FAIL', n]); if (!c && extra !== undefined) console.log('detail ' + n + ': ' + JSON.stringify(extra).slice(0, 600)); };
  const b = await chromium.launch();
  const p = await (await b.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
  const bad = [];
  p.on('response', r => { if (r.url().startsWith(base + '/v1/') && r.status() >= 500) bad.push(r.status() + ' ' + r.url()); });
  p.on('console', m => { if (/Content Security Policy/i.test(m.text())) bad.push(m.text()); });
  await p.goto(base + '/');
  await p.fill('#token', token); await p.press('#token', 'Enter'); await p.waitForSelector('.shell');
  const page = async id => { await p.goto(base + '/#/' + id); await p.reload(); await p.waitForSelector('.shell'); await p.waitForTimeout(1500); return p.locator('.content').innerText(); };
  const content = () => p.locator('.content').innerText();

  let txt = await page('tunnels');
  ok('tunnels: unit shown running, protocol v2, no "ready"', /Running|در حال اجرا/.test(txt) && /v2/.test(txt) && !READY.test(txt), txt.slice(0, 400));

  // Integrity probe from the Diagnostics page on a protocol v2 pair.
  await page('diagnostics');
  await p.locator('button', { hasText: /Run integrity probe|اجرای آزمون یکپارچگی/ }).click();
  const probed = await until(async () => /Passed ·|موفق ·|Unknown ·|نتیجه نامشخص|Probe failed|آزمون ناموفق|could not run|اجرا نشد/.test(await content()), 45000);
  txt = await content();
  const st = await api('GET', '/v1/status');
  const integ = st.tunnel.tiers.integrity;
  ok('diagnostics: integrity probe shows Passed with bytes checked', probed && /(Passed|موفق) · /.test(txt) && /(bytes checked|بایت بررسی شد)/.test(txt), txt.slice(-400));
  ok('API agrees: integrity tier pass, last probe sha256 match', integ.result === 'pass' && integ.last_probe && integ.last_probe.result === 'pass', integ);
  await p.screenshot({ path: SHOTS + '/lab-integrity.png', fullPage: true });

  // Lifecycle through the confirm dialog: Restart, Stop, Start.
  const life = async (label, action) => {
    await page('tunnels');
    const btn = p.locator('.content button', { hasText: label });
    ok(action + ': button enabled', await btn.isEnabled());
    await btn.click();
    const dlg = p.locator('dialog[open]');
    await dlg.waitFor({ timeout: 5000 });
    await dlg.locator('button.danger, button.primary').click();
    const done = await until(async () => /Applied|اعمال شد|Refused|رد شد|Rolled back|بازگردانی شد/.test(await content()), 120000);
    txt = await content();
    ok(action + ': receipt shows Applied', done && /Applied|اعمال شد/.test(txt), txt.slice(-500));
    ok(action + ': receipt never shows the token', !txt.includes(token));
    return txt;
  };
  const pid0 = (await api('GET', '/v1/status')).tunnel;
  txt = await life(/^\s*(Restart|راه‌اندازی مجدد)\s*$/, 'restart');
  ok('restart: receipt lists new_process pass', /new_process/.test(txt));
  ok('restart: unit active and enabled at boot (API)', await until(async () => { const u = await unit(); return u.result === 'active' && u.enabled_at_boot === true; }, 30000), await unit());
  await sleep(21000); // the panel holds refresh 20 s after an action
  txt = await life(/^\s*(Stop|توقف)\s*$/, 'stop');
  ok('stop: unit inactive and not enabled at boot (API)', await until(async () => { const u = await unit(); return u.result === 'inactive' && u.enabled_at_boot === false; }, 30000), await unit());
  await sleep(21000);
  txt = await page('tunnels');
  ok('tunnels after stop: shown Stopped, Start offered', /Stopped|متوقف/.test(txt));
  txt = await life(/^\s*(Start|شروع)\s*$/, 'start');
  ok('start: unit active and enabled at boot (API)', await until(async () => { const u = await unit(); return u.result === 'active' && u.enabled_at_boot === true; }, 30000), await unit());
  await p.screenshot({ path: SHOTS + '/lab-lifecycle.png', fullPage: true });

  txt = await page('activity');
  ok('activity lists the lifecycle and verify actions, never the token', /lifecycle|verify/i.test(txt) && !txt.includes(token), txt.slice(0, 300));
  ok('no 5xx from the agent, no CSP violation', bad.length === 0, bad);
  void pid0;
  await b.close();
  R.forEach(r => console.log('LAB_PANEL ' + r.join(' ')));
  process.exit(R.some(r => r[0] === 'FAIL') ? 1 : 0);
})().catch(e => { console.log('LAB_PANEL FAIL exception ' + (e && e.message || e).toString().replace(token, '<token>').slice(0, 400)); process.exit(1); });
