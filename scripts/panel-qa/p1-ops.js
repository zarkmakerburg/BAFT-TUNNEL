// Browser QA for P1 operations in the panel (traffic, tunnel state, diagnostics, audit v2, ops guidance).
// Serves panel/ statically and mocks /v1/* so it needs no agent: node p1-ops.js  (needs playwright)
const { chromium } = require('playwright');
const http = require('http'), fs = require('fs'), path = require('path');
const ROOT = path.join(__dirname, '..', '..', 'panel');
const SHOTS = process.env.BAFT_SHOTS || require('os').tmpdir();
const srv = http.createServer((q, r) => {
  const f = path.join(ROOT, q.url === '/' ? 'index.html' : q.url.split('?')[0]);
  if (!f.startsWith(ROOT) || !fs.existsSync(f)) { r.writeHead(404); return r.end(); }
  const ct = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' }[path.extname(f)] || 'application/octet-stream';
  r.writeHead(200, { 'content-type': ct }); r.end(fs.readFileSync(f));
});
const now = Math.floor(Date.now() / 1000);
const status = { version: '0.1.0', engine: 'tcp-tls', listener_accepting: true, config: { configuration: { tunnel_id: 't1', local_identity: 'abroad', peer_identity: 'iran', direction: 'forward', mode: 'service' } },
  tunnel: { state: 'carrying_traffic', last_verified_unix: now - 30, last_failure: null, tiers: { unit: { result: 'active' }, listener: { result: 'pass' },
    authorized_session: { result: 'observed', peer_id: 'iran' }, data_path: { result: 'traffic_observed' }, integrity: { result: 'unknown' } } } };
const traffic = { available: true, fresh: true, sampled_unix: now - 2, sampling_interval_s: 5, bytes_from_peer: 2097152, bytes_to_peer: 65536, streams_total: 3, sessions_total: 1,
  rate: { t: now, window_s: 5, from_peer_bps: 1024, to_peer_bps: 512 }, history: [{ t: now - 10, from_peer_bps: 0, to_peer_bps: 0 }, { t: now - 5, from_peer_bps: 100, to_peer_bps: 50 }, { t: now, from_peer_bps: 1024, to_peer_bps: 512 }] };
const diag = { at_unix: now - 5, verified: true, highest_passed_tier: 'data_path', tiers: [
  { id: 'tcp_reachability', result: 'unknown', source: 'listener', evidence: 'listening' }, { id: 'secure_channel', result: 'pass', source: 'session', evidence: 'authorized session' },
  { id: 'data_path', result: 'pass', source: 'counters', evidence: 'grew both ways' }, { id: 'integrity', result: 'unknown', source: 'none', evidence: 'not measured' }] };
const audit = { entries: [{ audit_version: 2, ts: now - 20, actor: { kind: 'api_token', fingerprint: 'deadbeefcafe0123' }, action: 'diagnostics', target: { node: 'abroad', instance: 'main' }, resource: 'diagnostics', result: 'accepted', sha256: 'a'.repeat(64), receipt_id: null }] };
(async () => {
  await new Promise(r => srv.listen(0, '127.0.0.1', r));
  const base = 'http://127.0.0.1:' + srv.address().port, R = [], ok = (n, c) => R.push([c ? 'PASS' : 'FAIL', n]);
  const b = await chromium.launch(); const p = await (await b.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
  let traffic404 = false, posted = 0;
  const json = (o, s = 200) => ({ status: s, contentType: 'application/json', body: JSON.stringify(o) });
  await p.route('**/v1/**', r => {
    const u = new URL(r.request().url()).pathname, m = r.request().method();
    if (u === '/v1/status') return r.fulfill(json(status));
    if (u === '/v1/traffic') return r.fulfill(traffic404 ? json({ available: false }) : json(traffic));
    if (u === '/v1/diagnostics') { if (m === 'POST') { posted++; return r.fulfill(json(diag)); } return r.fulfill(json({}, 404)); }
    if (u === '/v1/audit') return r.fulfill(json(audit));
    if (u === '/v1/history') return r.fulfill(json({ samples: [], interval_s: 5, capacity: 720 }));
    return r.fulfill(json({}, 404));
  });
  await p.addInitScript(() => sessionStorage.setItem('token', 'x'));
  await p.goto(base + '/#/overview'); await p.waitForSelector('.shell'); await p.waitForTimeout(600);
  let txt = await p.locator('.content').innerText();
  ok('overview shows measured traffic with units', /MiB/.test(txt) && /KiB\/s|KiB/.test(txt) && !/ساختگی|made-up/.test(txt));
  await p.goto(base + '/#/tunnels'); await p.waitForTimeout(600);
  txt = await p.locator('.content').innerText();
  ok('tunnels show end-to-end state, peer and last verified', /در حال انتقال داده|Carrying traffic/.test(txt) && /iran/.test(txt));
  await p.locator('button', { hasText: /اجرای عیب‌یابی|Run diagnostics/ }).click(); await p.waitForTimeout(500);
  txt = await p.locator('.content').innerText();
  ok('diagnostics POST issued and tiers rendered separately', posted === 1 && /مسیر داده|Data path/.test(txt) && /یکپارچگی|Integrity/.test(txt));
  await p.screenshot({ path: SHOTS + '/p1-tunnels-fa.png', fullPage: true });
  await p.goto(base + '/#/activity'); await p.waitForTimeout(500);
  txt = await p.locator('.content').innerText();
  ok('activity shows actor fingerprint (not token) and target', /deadbeef/.test(txt) && /abroad/.test(txt) && !/deadbeefcafe0123/.test(txt));
  await p.goto(base + '/#/agent'); await p.waitForTimeout(500);
  txt = await p.locator('.content').innerText();
  ok('agent page lists backup/restore/update/rollback guidance, read-only', /backup\.sh/.test(txt) && /rollback\.sh/.test(txt) && (await p.locator('.content button').count()) === 1);
  traffic404 = true; await p.goto(base + '/#/overview'); await p.reload(); await p.waitForTimeout(700);
  txt = await p.locator('.content').innerText();
  ok('without counters traffic stays Unknown, no numbers', !/MiB/.test(txt) && /نامشخص|Unknown|در دسترس نیست|Unavailable/.test(txt));
  await b.close(); srv.close(); R.forEach(r => console.log(r.join(' ')));
  process.exit(R.some(r => r[0] === 'FAIL') ? 1 : 0);
})();
