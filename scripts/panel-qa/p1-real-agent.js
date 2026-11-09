// Panel validation against a REAL running agent (no mocked /v1). Not part of CI.
// BAFT_AGENT_URL=http://127.0.0.1:18080 BAFT_TOKEN_FILE=/path/token BAFT_EXPECT=carrying|degraded node p1-real-agent.js
// carrying: tunnel up and payload sent through it before the run. degraded: peer stopped before the run.
const { chromium } = require('playwright');
const fs = require('fs');
const base = process.env.BAFT_AGENT_URL || 'http://127.0.0.1:18080';
const token = fs.readFileSync(process.env.BAFT_TOKEN_FILE, 'utf8').trim();
const expect = process.env.BAFT_EXPECT || 'carrying';
const SHOTS = process.env.BAFT_SHOTS || require('os').tmpdir();
const READY = /\b(ready|production[- ]ready|verified end-to-end)\b|آماده\s*(است|به کار)/i;
const api = async (m, p) => {
  const r = await fetch(base + p, { method: m, headers: { authorization: 'Bearer ' + token } });
  return r.json();
};
(async () => {
  const R = [], ok = (n, c) => R.push([c ? 'PASS' : 'FAIL', n]);
  const b = await chromium.launch();
  const p = await (await b.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
  const bad = [];
  p.on('response', r => { if (r.url().startsWith(base + '/v1/') && r.status() >= 500) bad.push(r.url()); });
  p.on('console', m => { if (/Content Security Policy/i.test(m.text())) bad.push(m.text()); });
  await p.goto(base + '/');
  await p.fill('#token', 'wrong-token'); await p.press('#token', 'Enter'); await p.waitForTimeout(600);
  ok('wrong token refused, no shell shown', (await p.locator('.shell').count()) === 0 && (await p.locator('.error:visible').count()) === 1);
  await p.fill('#token', token); await p.press('#token', 'Enter'); await p.waitForSelector('.shell');
  ok('token not persisted outside sessionStorage', await p.evaluate(() => !localStorage.length && !document.cookie));
  const page = async id => { await p.goto(base + '/#/' + id); await p.waitForTimeout(900); return p.locator('.content').innerText(); };

  const st = await api('GET', '/v1/status');
  const tr = await api('GET', '/v1/traffic');
  let txt = await page('overview');
  ok('overview never claims a stronger state than the API (no "ready")', !READY.test(txt));
  if (expect === 'carrying') {
    ok('API: session_authorized or carrying_traffic (never ready), integrity unknown', ['carrying_traffic', 'session_authorized'].includes(st.tunnel.state) && st.tunnel.tiers.integrity.result === 'unknown');
    ok('overview shows real measured traffic with units', tr.available && tr.fresh && /(KiB|MiB|B)\b/.test(txt));
  } else {
    ok('API: not carrying_traffic', st.tunnel.state !== 'carrying_traffic');
    ok('overview does not show a carrying state', !/Carrying traffic|در حال انتقال داده/.test(txt));
  }
  txt = await page('tunnels');
  ok('tunnels page: no "ready", integrity shown as Unknown', !READY.test(txt) && /(Integrity|یکپارچگی)[\s\S]{0,80}(Unknown|نامشخص)/.test(txt));
  if (expect === 'carrying') ok('tunnels page shows peer id from the session', st.tunnel.tiers.authorized_session.peer_id && txt.includes(st.tunnel.tiers.authorized_session.peer_id));
  await p.locator('button', { hasText: /اجرای عیب‌یابی|Run diagnostics/ }).click(); await p.waitForTimeout(1500);
  txt = await p.locator('.content').innerText();
  const d = await api('GET', '/v1/diagnostics');
  const integ = d.tiers.find(t => t.id === 'integrity');
  ok('diagnostics: integrity tier stays unknown after a real run', integ && integ.result === 'unknown');
  ok('diagnostics: tiers rendered separately, no "ready"', /(Data path|مسیر داده)/.test(txt) && !READY.test(txt));
  if (expect === 'carrying') ok('diagnostics: highest tier is data_path or session, never integrity', ['session', 'data_path'].includes(d.highest_passed_tier));
  else ok('diagnostics: no data_path pass when degraded', d.tiers.find(t => t.id === 'data_path').result !== 'pass');
  await p.screenshot({ path: SHOTS + '/real-tunnels.png', fullPage: true });
  txt = await page('activity');
  const au = await api('GET', '/v1/audit');
  const fp = au.entries.length && au.entries[au.entries.length - 1].actor.fingerprint;
  ok('activity shows actor fingerprint, never the token', fp && txt.includes(fp.slice(0, 8)) && !txt.includes(token));
  ok('audit v2 lines carry actor, target and resource', au.entries.every(e => e.audit_version === 2 && e.actor && e.target && e.resource));
  txt = await page('agent');
  ok('agent page documents backup/restore/update/rollback, read-only', /backup/.test(txt) && /rollback/.test(txt) && !READY.test(txt));
  ok('no 5xx from the agent, no CSP violation', bad.length === 0);
  await b.close();
  R.forEach(r => console.log(r.join(' ')));
  process.exit(R.some(r => r[0] === 'FAIL') ? 1 : 0);
})();
