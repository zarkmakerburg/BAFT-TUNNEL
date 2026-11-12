/* Pages. Each BAFT.register({id, icon, order, primary, needs, refresh, render(ctx) -> Node}).
   id doubles as the i18n key 'nav.<id>'. 'needs' lists extra endpoints to load (status is implicit).
   Rule for every page: show only what a real endpoint returned; otherwise Unknown / unavailable. */
(function () {
  'use strict';
  var BAFT = window.BAFT, h = BAFT.h, t = BAFT.t, C = window.Charts, G = BAFT.GLYPH;

  /* ---- shared components ---- */
  function pill(kind, label) {
    return h('span', { class: 'badge ' + kind }, h('span', { class: 'glyph', 'aria-hidden': 'true', text: G[kind] || G.unknown }), label);
  }
  function card(title, body, extra, right) {
    return h('section', { class: 'card ' + (extra || '') }, h('header', null, h('h2', { text: title }), right || null), body);
  }
  function kpi(label, value, hint, kind) {
    return h('section', { class: 'card kpi' }, h('h3', { text: label }),
      h('div', { class: 'value', text: value }), kind ? pill(kind.k, kind.l) : null, h('div', { class: 'hint', text: hint || '' }));
  }
  function empty(text, cta) {
    return h('div', { class: 'empty' }, h('p', { text: text }), cta ? h('a', { class: 'btn primary', href: cta.href, text: cta.label }) : null);
  }
  function kv(rows) {
    var dl = h('dl', { class: 'kv' });
    rows.forEach(function (r) { dl.appendChild(h('dt', { text: r[0] })); dl.appendChild(h('dd', null, r[1] == null ? '—' : r[1])); });
    return dl;
  }
  function median(a) {
    if (!a.length) { return null; }
    var s = a.slice().sort(function (x, y) { return x - y; }), m = s.length >> 1;
    return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
  }
  function conf(d) { return ((d.status || {}).config || {}).configuration || {}; }
  function life(sec) {
    if (sec == null) { return null; }
    return sec < 0 ? t('expired') : sec < 172800 ? t('hours', { n: BAFT.fmtNum(sec / 3600, 0) }) : t('days', { n: BAFT.fmtNum(sec / 86400, 0) });
  }
  function listenState(s) {
    return s.listener_accepting ? { k: 'ok', l: t('state.listening') } : { k: 'bad', l: t('state.notlistening') };
  }
  function stats(d) {
    var hist = (d.history || {}).samples || [], up = hist.filter(function (x) { return x.up; });
    return { hist: hist, avail: hist.length ? up.length / hist.length * 100 : null, med: median(up.map(function (x) { return x.rtt_ms; })) };
  }
  /* Warnings come only from real data; each has severity, affected resource, reason and a next step. */
  function warnings(d) {
    var s = d.status || {}, c = conf(d), sec = d.security || {}, st = stats(d), out = [];
    if (!s.listener_accepting) { out.push({ sev: 'bad', res: c.tunnel_id, text: t('warn.listener', { t: c.tunnel_id || '—' }), next: t('warn.listener.next') }); }
    if (sec.tls) {
      var cs = sec.certificate_seconds_remaining, ls = sec.crl_seconds_remaining;
      if (cs != null && cs < 30 * 86400) { out.push({ sev: cs < 7 * 86400 ? 'bad' : 'warn', res: t('servers.this'), text: t('warn.cert'), next: t('warn.cert.next') }); }
      if (ls != null && ls < 7 * 86400) { out.push({ sev: ls < 3 * 86400 ? 'bad' : 'warn', res: 'CRL', text: t(ls <= 0 ? 'warn.crl.expired' : 'warn.crl'), next: t('warn.crl.next') }); }
      if (c.peer_certificate_pin_configured === false) { out.push({ sev: 'warn', res: c.tunnel_id, text: t('warn.pin'), next: t('warn.pin.next') }); }
    }
    if (st.avail != null && st.hist.length >= 12 && st.avail < 90) { out.push({ sev: 'warn', res: c.tunnel_id, text: t('warn.avail'), next: t('warn.avail.next') }); }
    if (d.audit === null) { out.push({ sev: 'info', res: 'Agent', text: t('warn.audit'), next: t('warn.audit.next') }); }
    return out;
  }
  var SEV = { bad: 'sev.error', warn: 'sev.warning', info: 'sev.info' };
  function warnList(ws) {
    if (!ws.length) { return h('p', { class: 'muted', text: t('ov.nowarn') }); }
    return h('ul', { class: 'warns' }, ws.map(function (w) {
      return h('li', null, h('div', { class: 'top' }, pill(w.sev === 'bad' ? 'bad' : w.sev === 'warn' ? 'warn' : 'info', t(SEV[w.sev])), w.res ? h('strong', null, BAFT.ltr(w.res)) : null),
        h('div', { text: w.text }), h('div', { class: 'sub', text: w.next }));
    }));
  }
  function fmtBytes(n) {
    if (n == null || isNaN(n)) { return null; }
    var u = ['B', 'KiB', 'MiB', 'GiB', 'TiB'], i = 0, v = Number(n);
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return BAFT.fmtNum(v, i ? 1 : 0) + ' ' + u[i];
  }
  function tierKind(r) {
    return r === 'pass' || r === 'observed' || r === 'traffic_observed' || r === 'active' ? 'ok' : r === 'fail' ? 'bad' : r === 'no_recent_traffic' || r === 'not_observed' ? 'warn' : 'unknown';
  }
  function tierPill(r) { return pill(tierKind(r), t('tier.r.' + r) === 'tier.r.' + r ? String(r || 'unknown') : t('tier.r.' + r)); }
  function tunnelState(tn) {
    var k = tn ? tn.state : 'unknown';
    return { k: k === 'carrying_traffic' ? 'ok' : k === 'session_authorized' ? 'ok' : k === 'running_no_session' ? 'warn' : k === 'down' ? 'bad' : 'unknown', l: t('ts.' + (k || 'unknown')) };
  }
  function dirLabel(x) { return x ? t('dir.' + x) : '—'; }
  function roleLabel(x, dir) { return x ? t((dir === 'reverse' ? 'role.rev.' : 'role.') + x) : '—'; }

  function trafficBody(tr) {
    if (!tr) { return empty(t('ov.traffic.body')); }
    var r = tr.rate, hist = (tr.history || []);
    var rows = [[t('tr.from'), BAFT.ltr(fmtBytes(tr.bytes_from_peer))], [t('tr.to'), BAFT.ltr(fmtBytes(tr.bytes_to_peer))],
      [t('tr.rate'), r ? BAFT.ltr('↓ ' + fmtBytes(r.from_peer_bps) + '/s · ↑ ' + fmtBytes(r.to_peer_bps) + '/s') : t('tr.norate')],
      [t('tr.streams'), BAFT.fmtNum(tr.streams_total || 0, 0)], [t('tr.sessions'), BAFT.fmtNum(tr.sessions_total || 0, 0)],
      [t('tr.sampled'), BAFT.ago(tr.sampled_unix) + ' · ' + t('tr.interval', { s: BAFT.fmtNum(tr.sampling_interval_s || 0, 0) })]];
    var chart = hist.length > 1 ? C.line(hist.map(function (x) { return { x: x.t, y: (x.from_peer_bps || 0) + (x.to_peer_bps || 0) }; }),
      { label: t('tr.rate') + ' (B/s)', fmtX: BAFT.fmtTime, fmtY: function (v) { return fmtBytes(v) + '/s'; } }) : null;
    return h('div', null, kv(rows), chart, h('p', { class: 'sub', text: t('tr.source') }));
  }

  /* ---- Overview ---- */
  BAFT.register({ id: 'overview', icon: 'overview', order: 1, primary: true, needs: ['history', 'security', 'audit', 'desired', 'traffic'], render: function (ctx) {
    var d = ctx.data, s = d.status, c = conf(d), st = stats(d), ws = warnings(d), hist = st.hist, tr = d.traffic && d.traffic.available ? d.traffic : null;
    var entries = ((d.audit || {}).entries || []).slice().reverse();
    var fresh = !d.desired && !entries.length;
    var kpis = h('div', { class: 'grid kpis' },
      kpi(t('kpi.servers'), '1', t('kpi.servers.hint')),
      kpi(t('kpi.tunnels'), '1', t('kpi.tunnels.hint'), listenState(s)),
      kpi(t('kpi.attention'), BAFT.fmtNum(ws.filter(function (w) { return w.sev !== 'info'; }).length, 0), t('kpi.attention.hint')),
      tr ? kpi(t('kpi.traffic'), fmtBytes((tr.bytes_from_peer || 0) + (tr.bytes_to_peer || 0)), t('kpi.traffic.live', { ago: BAFT.ago(tr.sampled_unix) }), tr.fresh ? { k: 'ok', l: t('fresh') } : { k: 'warn', l: t('stale') })
         : kpi(t('kpi.traffic'), t('kpi.traffic.value'), t('kpi.traffic.hint'), { k: 'unknown', l: t('unknown') }));
    var timeline = hist.length ? h('div', null, C.strip(hist, { label: t('ov.health') }),
      h('div', { class: 'legend' }, h('span', { text: BAFT.fmtTime(hist[0].t) }), h('span', { text: BAFT.fmtTime(hist[hist.length - 1].t) }))) : empty(t('empty.history'));
    var pts = hist.map(function (x) { return { x: x.t, y: x.rtt_ms }; });
    var latency = hist.length && st.med != null ? h('div', null, C.line(pts, { label: t('ov.latency') + ' (' + t('ov.latency.unit') + ')', fmtX: BAFT.fmtTime, fmtY: function (v) { return BAFT.fmtNum(v, 1); } }),
      h('div', { class: 'legend' }, h('span', { text: t('ov.latency.unit') }), h('span', { text: BAFT.fmtNum(st.med, 2) + ' ms' }))) : empty(hist.length ? t('ov.latency.none') : t('empty.history'));
    var src = h('p', { class: 'sub', text: t('ov.health.src', { s: BAFT.fmtNum((d.history || {}).interval_s || 5, 0), n: BAFT.fmtNum((d.history || {}).capacity || 0, 0) }) });
    var avail = st.avail == null ? null : h('p', { class: 'sub', text: t('ov.avail') + ': ' + BAFT.fmtNum(st.avail, 1) + '% · ' + t('ov.window', { n: BAFT.fmtNum(hist.length, 0) }) });
    var recent = entries.length ? h('ul', { class: 'warns' }, entries.slice(0, 5).map(function (e) {
      return h('li', null, h('div', { class: 'top' }, pill(e.result === 'accepted' ? 'ok' : 'bad', t(e.result) === e.result ? e.result : t(e.result)), h('span', { class: 'sub', text: new Date(e.ts * 1000).toLocaleString(BAFT.lang === 'fa' ? 'fa-IR' : 'en-GB') })),
        h('div', null, opLabel(e)));
    })) : h('p', { class: 'muted', text: d.audit === null ? t('activity.disabled') : t('ov.recent.empty') });
    return h('div', null,
      fresh ? h('div', { class: 'banner info' }, h('h2', { text: t('ov.start.title') }), h('p', { text: t('ov.start.body') }), h('a', { class: 'btn primary', href: '#/setup', text: t('ov.start.cta') })) : null,
      kpis,
      h('div', { class: 'split' },
        h('div', { class: 'stack' },
          card(t('ov.health'), h('div', null, timeline, src, avail)),
          card(t('ov.latency'), latency),
          card(t('ov.traffic.title'), trafficBody(tr))),
        h('div', { class: 'stack' },
          card(t('ov.warnings'), warnList(ws)),
          card(t('ov.recent'), recent, '', h('a', { href: '#/activity', text: t('nav.activity') })))));
  } });

  /* ---- Servers ---- */
  BAFT.register({ id: 'servers', icon: 'servers', order: 2, primary: true, needs: ['history', 'security', 'engines', 'audit', 'desired'], render: function (ctx) {
    var d = ctx.data, s = d.status, c = conf(d), sec = d.security || {}, st = stats(d);
    var engines = ((d.engines || {}).engines || []).map(function (e) { return e.id; }).join(', ');
    var caps = h('ul', { class: 'warns' },
      h('li', { text: t('cap.engines', { l: engines || '—' }) }),
      h('li', null, t('cap.audit') + ': ', pill(d.audit === null ? 'unknown' : 'ok', d.audit === null ? t('cap.off') : t('cap.on'))),
      h('li', null, t('cap.desired') + ': ', pill(d.audit === null ? 'unknown' : 'ok', d.audit === null ? t('cap.off') : t('cap.on'))),
      h('li', null, t('cap.plan') + ': ', pill('ok', t('cap.on'))),
      h('li', null, t('cap.apply') + ': ', pill('ok', t('cap.on'))));
    var trust = sec.tls === false ? h('p', { class: 'muted', text: t('trust.noise') }) : kv([
      [t('trust.pin'), pill(c.peer_certificate_pin_configured ? 'ok' : 'warn', c.peer_certificate_pin_configured ? t('yes') : t('no'))],
      [t('trust.ca'), pill(c.ca_file_present ? 'ok' : 'bad', c.ca_file_present ? t('yes') : t('no'))],
      [t('trust.certfile'), pill(c.certificate_file_present ? 'ok' : 'bad', c.certificate_file_present ? t('yes') : t('no'))],
      [t('f.cert'), life(sec.certificate_seconds_remaining) || '—'], [t('f.crl'), life(sec.crl_seconds_remaining) || '—'], [t('f.keysrc'), c.private_key_source]]);
    var ls = listenState(s);
    var thisNode = h('section', { class: 'card res' },
      h('div', { class: 'head' }, h('div', null, h('div', { class: 'title' }, BAFT.ltr(c.local_identity)), h('div', { class: 'sub', text: t('servers.this') + ' · ' + roleLabel(c.mode, c.direction) })), pill(ls.k, ls.l)),
      h('p', { class: 'sub', text: t('state.why.local') }),
      kv([[t('f.lastseen'), t('checked', { ago: BAFT.ago(BAFT.fetchedAt) })], [t('f.version'), BAFT.ltr('v' + s.version)],
        [t('f.diag'), st.med == null ? t('unknown') : t('f.latency') + ': ' + BAFT.fmtNum(st.med, 2) + ' ms · ' + t('f.avail') + ': ' + BAFT.fmtNum(st.avail, 1) + '%']]),
      h('hr', { class: 'sep' }), h('h3', { text: t('f.caps') }), caps,
      h('hr', { class: 'sep' }), h('h3', { text: t('f.trust') }), trust);
    var peer = h('section', { class: 'card res' },
      h('div', { class: 'head' }, h('div', null, h('div', { class: 'title' }, BAFT.ltr(c.peer_identity)), h('div', { class: 'sub', text: t('servers.peer') })), pill('unknown', t('state.unknown'))),
      h('p', { class: 'sub', text: t('servers.peer.note') }),
      kv([[t('f.lastseen'), t('notobserved')], [t('f.version'), t('unknown')], [t('f.caps'), t('unknown')]]));
    return h('div', { class: 'stack' }, h('p', { class: 'muted', text: t('servers.note') }), h('div', { class: 'grid cols-2' }, thisNode, peer));
  } });

  /* ---- Diagnostics report (POST/GET /v1/diagnostics): each tier is separate evidence ---- */
  function diagTable(rep) {
    var tiers = rep.tiers || [];
    var head = h('p', { class: 'sub', text: t('diag.at', { ago: BAFT.ago(rep.at_unix) }) + ' · ' + (rep.verified ? t('diag.verified') : t('diag.notverified')) +
      (rep.highest_passed_tier ? ' · ' + t('diag.highest', { tier: t('tier.' + rep.highest_passed_tier) }) : '') });
    var tbl = h('div', { class: 'table-wrap' }, h('table', { class: 'cards' },
      h('thead', null, h('tr', null, ['diag.col.tier', 'diag.col.result', 'diag.col.source', 'diag.col.evidence', 'diag.col.next'].map(function (k) { return h('th', { scope: 'col', text: t(k) }); }))),
      h('tbody', null, tiers.map(function (x) {
        var evidenceKey = x.code || '';
        var evidenceText = evidenceKey && t(evidenceKey, x.params || {}) !== evidenceKey ? t(evidenceKey, x.params || {}) : x.evidence;
        var nextText = x.next_code && t(x.next_code, x.next_params || {}) !== x.next_code ? t(x.next_code, x.next_params || {}) : x.next;
        return h('tr', null, h('td', { 'data-label': t('diag.col.tier'), text: t('tier.' + x.id) === 'tier.' + x.id ? x.id : t('tier.' + x.id) }),
          h('td', { 'data-label': t('diag.col.result') }, tierPill(x.result)),
          h('td', { 'data-label': t('diag.col.source') }, t('diag.source.' + x.source) === 'diag.source.' + x.source ? BAFT.ltr(x.source) : t('diag.source.' + x.source)),
          h('td', { 'data-label': t('diag.col.evidence') }, BAFT.lang === 'fa' ? evidenceText : BAFT.ltr(evidenceText)),
          h('td', { 'data-label': t('diag.col.next') }, x.next ? (BAFT.lang === 'fa' ? nextText : BAFT.ltr(nextText)) : '—'));
      }))));
    var na = rep.next_action || null;
    var nextKey = na && (na.code || ('diag.next.' + na.action));
    var nextParams = Object.assign({}, na && na.params || {});
    if (na && na.code === 'diag.next.rerun_tier' && nextParams.tier) { nextParams.tier = t('tier.' + nextParams.tier); }
    var nextLabel = nextKey ? t(nextKey, nextParams) : '';
    var next = na ? h('p', { class: 'next-action', role: 'status' }, h('strong', { text: t('diag.next') + ' ' }),
      nextLabel !== nextKey ? nextLabel : (t('diag.next.' + na.action) === 'diag.next.' + na.action ? '' : t('diag.next.' + na.action) + ' · ' + (na.text || ''))) : null;
    return h('div', null, head, next, tbl, h('p', { class: 'sub', text: t('diag.integrity') }));
  }

  /* ---- Lifecycle (POST /v1/tunnel/lifecycle): Start enables at boot, Stop disables, Restart ---- */
  function lifeKey() {
    var a = new Uint8Array(12); (window.crypto || {}).getRandomValues && window.crypto.getRandomValues(a);
    return 'ui-life-' + Array.prototype.map.call(a, function (x) { return ('0' + x.toString(16)).slice(-2); }).join('');
  }
  function lifeReceipt(rc) {
    var ok = rc.result === 'applied';
    var boot = rc.boot ? (rc.boot.enabled_at_boot ? t('life.boot.on') : t('life.boot.off')) : null;
    return h('div', { class: 'card', role: 'status' },
      h('p', null, pill(ok ? 'ok' : 'warn', t('rc.' + rc.result) === 'rc.' + rc.result ? String(rc.result) : t('rc.' + rc.result)), ' ', t('life.act.' + rc.stage), ' · ', BAFT.ltr(rc.id || '')),
      rc.reason ? h('p', null, BAFT.ltr(rc.reason)) : null,
      boot ? h('p', { class: 'sub', text: boot }) : null,
      rc.verification ? h('ul', null, rc.verification.map(function (v) { return h('li', null, pill(v.result === 'pass' ? 'ok' : 'warn', v.result), ' ', BAFT.ltr(v.id), h('span', { class: 'muted', text: ' · ' + v.evidence })); })) : null,
      rc.rollback && rc.rollback.performed ? h('p', { class: 'error', text: t('life.rolled') }) : null,
      rc.not_done ? h('details', null, h('summary', { text: t('rc.notdone') }), h('ul', null, rc.not_done.map(function (x) { return h('li', { text: x }); }))) : null,
      h('p', { class: 'sub', text: t('rc.nosecret') }));
  }
  function lifecycle(tn) {
    var unit = tn && tn.tiers && tn.tiers.unit;
    if (!unit || unit.result === 'unknown' && unit.reason) { return h('p', { class: 'sub', text: t('life.noinstance') }); }
    var up = unit.result === 'active', out = h('div', { 'aria-live': 'polite' });
    var state = kv([[t('life.state'), pill(up ? 'ok' : unit.result === 'inactive' ? 'warn' : 'unknown', up ? t('life.running') : unit.result === 'inactive' ? t('life.stopped') : t('state.unknown'))],
      [t('life.boot'), unit.enabled_at_boot == null ? t('unknown') : unit.enabled_at_boot ? t('life.boot.on') : t('life.boot.off')]]);
    var btns = [];
    function act(action) {
      if (!window.confirm(t('life.confirm.' + action))) { return; }
      btns.forEach(function (b) { b.disabled = true; });
      out.textContent = t('life.working');
      BAFT.api('POST', '/v1/tunnel/lifecycle', { action: action, confirm: true, idempotency_key: lifeKey() }).then(function (r) {
        out.textContent = '';
        if (r.body && r.body.result) { out.appendChild(lifeReceipt(r.body)); } else { out.textContent = (r.body && (r.body.detail || r.body.error)) || t('fail'); }
      }, function (e) { out.textContent = e.message || t('fail'); }).then(function () {
        btns.forEach(function (b) { b.disabled = false; });
        BAFT.toast(t('life.done'));
      });
    }
    function btn(action, enabled, primary) {
      var b = h('button', { type: 'button', class: primary ? 'primary' : null, disabled: enabled ? null : true, onclick: function () { act(action); }, text: t('life.act.' + action) });
      btns.push(b); return b;
    }
    return h('div', null, state,
      h('div', { class: 'row' }, btn('start', !up, true), btn('restart', up), btn('stop', up || unit.enabled_at_boot)),
      h('p', { class: 'sub', text: t('life.note') }), out);
  }

  /* ---- Tunnels ---- */
  BAFT.register({ id: 'tunnels', icon: 'tunnels', order: 3, primary: true, needs: ['history', 'security', 'diagnostics'], render: function (ctx) {
    var d = ctx.data, s = d.status, c = conf(d), st = stats(d), ls = listenState(s), tn = s.tunnel || null, ts = tunnelState(tn), dg = d.diagnostics || null;
    var diagOut = h('div', { 'aria-live': 'polite' });
    function showDiag(rep) { diagOut.textContent = ''; diagOut.appendChild(diagTable(rep)); }
    function runDiag(btn) {
      btn.disabled = true; diagOut.textContent = t('diag.running');
      BAFT.api('POST', '/v1/diagnostics').then(function (r) {
        btn.disabled = false;
        if (r.status === 200) { showDiag(r.body); } else { diagOut.textContent = t('diag.failed'); }
      }, function () { btn.disabled = false; diagOut.textContent = t('diag.failed'); });
    }
    var diagBtn = h('button', { type: 'button', onclick: function () { runDiag(diagBtn); }, text: t('diag.run') });
    var verifyOut = h('div', { class: 'sub', 'aria-live': 'polite' });
    var verifyIdempotencyKey = null;
    function runVerify(btn) {
      btn.disabled = true; verifyOut.textContent = t('verify.running');
      if (!verifyIdempotencyKey) { verifyIdempotencyKey = lifeKey().replace('ui-life-', 'ui-verify-'); }
      BAFT.api('POST', '/v1/verify', { idempotency_key: verifyIdempotencyKey }).then(function (r) {
        if (r.status === 200) { verifyIdempotencyKey = null; }
        btn.disabled = false;
        var v = r.body || {};
        var when = v.timestamp ? BAFT.ago(v.timestamp) : t('unknown');
        if (r.status === 200 && v.result === 'pass' && v.sha256_match === true) {
          verifyOut.replaceChildren(
            h('span', { text: t('verify.pass.prefix') }),
            BAFT.ltr(BAFT.fmtNum(v.bytes || 0, 0)),
            h('span', { text: t('verify.pass.bytes') }),
            BAFT.ltr(BAFT.fmtNum(v.duration_ms || 0, 0)),
            h('span', { text: t('verify.pass.duration') }),
            h('span', { text: when })
          );
        } else if (r.status === 200 && v.result === 'fail') {
          verifyOut.textContent = t('verify.fail', { reason: t('verify.reason.' + (v.reason || 'invalid_probe_response')), when: when });
        } else if (r.status === 200) {
          verifyOut.textContent = t('verify.unknown', { reason: t('verify.reason.' + (v.reason || 'probe_failed')), when: when });
        } else {
          verifyOut.textContent = t('verify.failed');
        }
      }, function () { btn.disabled = false; verifyOut.textContent = t('verify.failed'); });
    }
    var verifyBtn = h('button', { type: 'button', onclick: function () { runVerify(verifyBtn); }, text: t('verify.run') });
    if (dg) { showDiag(dg); }
    var res = h('section', { class: 'card res' },
      h('div', { class: 'head' }, h('div', null, h('div', { class: 'title' }, BAFT.ltr(c.tunnel_id)),
        h('div', { class: 'sub' }, dirLabel(c.direction) + ' · ', BAFT.ltr(c.local_identity), ' ⇄ ', BAFT.ltr(c.peer_identity))), pill(tn ? ts.k : ls.k, tn ? ts.l : ls.l)),
      h('p', { class: 'sub', text: (tn ? t('ts.why') : t('state.why.local')) + ' ' + t('checked', { ago: BAFT.ago(BAFT.fetchedAt) }) }),
      kv([[t('f.engine'), BAFT.ltr(s.engine)], [t('f.role'), roleLabel(c.mode, c.direction)],
        [t('f.peerconn'), tn ? tierPill((tn.tiers.authorized_session || {}).result) : pill('unknown', t('state.unknown'))],
        [t('f.peer'), tn && tn.tiers.authorized_session && tn.tiers.authorized_session.peer_id ? BAFT.ltr(tn.tiers.authorized_session.peer_id) : null],
        [t('tier.data_path'), tn ? tierPill((tn.tiers.data_path || {}).result) : pill('unknown', t('state.unknown'))],
        [t('tier.integrity'), tn ? tierPill((tn.tiers.integrity || {}).result) : pill('unknown', t('state.unknown'))],
        [t('verify.last'), tn && tn.tiers.integrity && tn.tiers.integrity.last_probe
          ? pill(tn.tiers.integrity.last_probe.result, t('verify.result.' + tn.tiers.integrity.last_probe.result)) + ' · ' + BAFT.ago(tn.tiers.integrity.last_probe.timestamp)
          : t('never')],
        [t('f.verified'), tn && tn.last_verified_unix ? BAFT.ago(tn.last_verified_unix) : t('f.verified.none')],
        [t('ts.lastfail'), tn && tn.last_failure ? t('tier.' + tn.last_failure.stage) + ' · ' + BAFT.ago(tn.last_failure.at_unix) : t('ts.nofail')]]),
      h('hr', { class: 'sep' }), h('h3', { text: t('diag.title') }), h('p', { class: 'sub', text: t('diag.note') }), diagBtn, diagOut,
      h('hr', { class: 'sep' }), h('h3', { text: t('verify.title') }),
      h('p', { class: 'sub', text: t('verify.note') }), verifyBtn, verifyOut,
      h('hr', { class: 'sep' }), h('h3', { text: t('tunnels.metrics') }),
      kv([[t('f.latency'), st.med == null ? t('unknown') : BAFT.ltr(BAFT.fmtNum(st.med, 2) + ' ms')], [t('f.avail'), st.avail == null ? t('unknown') : [BAFT.ltr(BAFT.fmtNum(st.avail, 1) + '%'), ' · ' + t('ov.window', { n: BAFT.fmtNum(st.hist.length, 0) })]]]),
      h('hr', { class: 'sep' }), h('h3', { text: t('tunnels.config') }),
      kv([[t('f.streams'), BAFT.fmtNum(c.max_streams || 0, 0)], [t('f.pin'), c.peer_certificate_pin_configured == null ? '—' : c.peer_certificate_pin_configured ? t('yes') : t('no')]]),
      h('hr', { class: 'sep' }), h('h3', { text: t('tunnels.actions') }), lifecycle(tn));
    return h('div', { class: 'stack' }, h('p', { class: 'muted', text: t('tunnels.note') }), res,
      h('div', null, h('a', { class: 'btn primary', href: '#/setup', text: t('tunnels.create') })));
  } });

  /* ---- Create tunnel (wizard over the canonical setup planner) ---- */
  var STEPS = ['direction', 'servers', 'method', 'preflight', 'preview', 'apply'];
  function svgEl(tag, attrs, text) {
    var n = document.createElementNS('http://www.w3.org/2000/svg', tag);
    Object.keys(attrs || {}).forEach(function (k) { n.setAttribute(k, attrs[k]); });
    if (text) { n.textContent = text; }
    return n;
  }
  function topology(kind) {
    var s = svgEl('svg', { viewBox: '0 0 260 80', class: 'topo', role: 'img', 'aria-label': t('dir.' + kind) });
    s.appendChild(svgEl('rect', { x: 8, y: 20, width: 80, height: 40, rx: 8 }));
    s.appendChild(svgEl('rect', { x: 172, y: 20, width: 80, height: 40, rx: 8 }));
    if (kind === 'reverse') { s.appendChild(svgEl('rect', { x: 166, y: 14, width: 92, height: 52, rx: 10, class: 'fw' })); }
    s.appendChild(svgEl('line', { x1: 88, y1: 40, x2: 172, y2: 40 }));
    s.appendChild(svgEl('text', { x: 48, y: 44, 'text-anchor': 'middle' }, t('dir.diagram.a')));
    s.appendChild(svgEl('text', { x: 212, y: 44, 'text-anchor': 'middle' }, t('dir.diagram.b')));
    if (kind === 'reverse') { s.appendChild(svgEl('text', { x: 212, y: 12, 'text-anchor': 'middle' }, t('dir.diagram.fw'))); }
    return s;
  }
  function freshWizard(c) {
    return { step: 0, direction: null, role: 'forward', engine: 'tcp-tls', node: c.local_identity || '', peerId: '', tunnel: '', port: '', peerHost: '', serverName: '', localServerName: '',
      tport: '8443', dir: '/etc/baft-tunnel', inst: '', pf: null, plan: null, busy: false, errors: {} };
  }
  function body(w) {
    var b = { engine: w.engine, direction: w.direction, role: w.role, node_id: w.node.trim(), peer_id: w.peerId.trim(), tunnel_id: w.tunnel.trim(),
      service_port: Number(w.port), tunnel_port: Number(w.tport), dir: w.dir.trim() };
    if (w.inst.trim()) { b.instance = w.inst.trim(); }
    if (w.role === 'forward') { b.peer_host = w.peerHost.trim(); if (w.engine === 'tcp-tls') { b.server_name = w.serverName.trim(); } }
    return b;
  }
  BAFT.register({ id: 'setup', icon: 'setup', order: 4, primary: true, needs: ['engines'], refresh: false, render: function (ctx) {
    var w = BAFT.wiz = BAFT.wiz || freshWizard(conf(ctx.data));
    var st0 = ctx.data.status || {};
    // Setup mode (ADR-0026): this agent pairs exactly its own instance in its own directory.
    if (st0.setup_pending && !w.prefilled) { w.inst = st0.instance || ''; w.dir = st0.config_dir || w.dir; w.prefilled = true; }
    var engines = ((ctx.data.engines || {}).engines || []);
    var root = h('div'), live = h('div', { 'aria-live': 'polite' });
    function invalidate() { w.pf = null; w.plan = null; }
    function field(key, label, opts) {
      opts = opts || {};
      var input = h('input', { name: key, value: w[key], type: opts.type || 'text', autocomplete: 'off', inputmode: opts.inputmode, 'aria-invalid': w.errors[key] ? 'true' : null,
        'aria-describedby': w.errors[key] ? 'e-' + key : null, oninput: function () {
          w[key] = input.value; invalidate();
          if (key === 'node' || key === 'localServerName') {
            w.csrReceipt = null; w.csrPem = ''; w.pkiReceipt = null;
            w.certificatePem = ''; w.caPem = ''; w.crlPem = ''; w.pkiConfirm = false;
          }
        } });
      if (opts.ltr !== false) { input.setAttribute('dir', 'ltr'); }
      return h('label', { class: 'field' }, h('span', { text: label }), input, w.errors[key] ? h('div', { class: 'fielderr', id: 'e-' + key, text: w.errors[key] }) : null);
    }
    function choice(name, value, checked, titleKids, desc, onpick, extra, disabled) {
      var input = h('input', { type: 'radio', name: name, value: value, checked: checked ? true : null, disabled: disabled ? true : null, onchange: function () { onpick(value); } });
      return h('label', { class: 'choice' }, input, extra || null, h('div', { class: 't' }, titleKids), h('div', { class: 'sub', text: desc }));
    }
    function validate(step) {
      var e = {};
      if (step === 0 && !w.direction) { e.direction = t('err.required'); }
      if (step === 1) {
        ['node', 'peerId', 'tunnel'].forEach(function (k) { if (!w[k].trim()) { e[k] = t('err.required'); } });
        if (w.node.trim() && w.node.trim() === w.peerId.trim()) { e.peerId = t('err.same'); }
        var p = Number(w.port); if (!(p >= 1024 && p <= 65535)) { e.port = t('err.port'); }
        var q = Number(w.tport); if (!(q >= 1024 && q <= 65535)) { e.tport = t('err.port'); }
        if (w.role === 'forward') {
          if (!w.peerHost.trim()) { e.peerHost = t('err.required'); }
          if (w.engine === 'tcp-tls' && !w.serverName.trim()) { e.serverName = t('err.required'); }
        }
        if (w.engine === 'tcp-tls' && !w.localServerName.trim()) { e.localServerName = t('err.required'); }
      }
      w.errors = e;
      return !Object.keys(e).length;
    }
    function post(path) {
      return BAFT.api('POST', path, body(w)).then(function (r) {
        if (r.status !== 200) { throw new Error((r.body && (r.body.detail || r.body.error)) || t('fail')); }
        return r.body;
      });
    }
    function runPreflight() {
      w.busy = true; w.pfError = null; draw();
      post('/v1/setup/preflight').then(function (b) { w.pf = b; }, function (e) { w.pfError = e.message; }).then(function () { w.busy = false; draw(); });
    }
    function buildPlan() {
      w.busy = true; w.planError = null; draw();
      post('/v1/setup/plan').then(function (b) { w.plan = b; }, function (e) { w.planError = e.message; }).then(function () { w.busy = false; draw(); });
    }
    function go(n) {
      if (n > w.step && !validate(w.step)) { draw(); return; }
      w.step = n; w.errors = {}; draw();
      if (n === 4 && !w.plan) { buildPlan(); }
    }
    function stepper() {
      return h('ol', { class: 'stepper', 'aria-label': t('setup.title') }, STEPS.map(function (k, i) {
        return h('li', { class: i < w.step ? 'done' : '', 'aria-current': i === w.step ? 'step' : null },
          h('span', { class: 'n', 'aria-hidden': 'true', text: i < w.step ? '✓' : BAFT.fmtNum(i + 1, 0) }), h('span', { class: 'l', text: t('step.' + k) }));
      }));
    }
    function stepDirection() {
      return h('div', null, h('h2', { text: t('dir.q') }),
        h('div', { class: 'choices', role: 'radiogroup', 'aria-label': t('dir.q') },
          ['direct', 'reverse'].map(function (k) {
            var avail = engines.some(function (e) { return e.directions.indexOf(k) >= 0; });
            return choice('direction', k, w.direction === k, avail ? (k === 'reverse' ? [t('dir.reverse.t'), pill('warn', t('dir.preview'))] : t('dir.' + k + '.t')) : [t('dir.' + k + '.t'), pill('unknown', t('dir.na.pill'))], avail ? t('dir.' + k + '.d') : t('dir.na.d'),
              function (v) { w.direction = v; w.errors = {}; invalidate(); draw(); }, topology(k), !avail);
          })),
        h('p', { class: 'sub', text: t('dir.same') }), w.errors.direction ? h('p', { class: 'fielderr', role: 'alert', text: w.errors.direction }) : null);
    }
    function stepServers() {
      var fwd = w.role === 'forward', rev = w.direction === 'reverse';
      return h('div', null, h('h2', { text: t('srv.q') }), h('p', { class: 'sub', text: t('srv.noinv') }),
        h('div', { class: 'choices', role: 'radiogroup', 'aria-label': t('srv.q') },
          choice('role', 'forward', fwd, t(rev ? 'srv.rev.forward' : 'srv.forward'), t(rev ? 'srv.rev.forward.d' : 'srv.opens'), function (v) { w.role = v; invalidate(); draw(); }),
          choice('role', 'serve', !fwd, t(rev ? 'srv.rev.serve' : 'srv.serve'), rev ? t('srv.rev.serve.d') : '', function (v) { w.role = v; invalidate(); draw(); })),
        h('div', { class: 'form-grid' }, field('node', t('f.node')), field('peerId', t('f.peerid')), field('tunnel', t('f.tunnel')), field('port', t(rev && !fwd ? 'f.port.ingress' : 'f.port'), { type: 'number', inputmode: 'numeric' }),
          fwd ? field('peerHost', t('f.peerhost'), { inputmode: 'decimal' }) : null,
          fwd && w.engine === 'tcp-tls' ? field('serverName', t('f.servername')) : null,
          w.engine === 'tcp-tls' ? h('div', null, field('localServerName', t('f.localserver')), h('p', { class: 'sub', text: t('f.localserver.help') })) : null),
        h('details', null, h('summary', { text: t('adv') }), h('div', { class: 'form-grid' }, field('tport', t('f.tport'), { type: 'number', inputmode: 'numeric' }), field('dir', t('f.dir')), field('inst', t('f.inst')))));
    }
    function stepMethod() {
      var ok = engines.filter(function (e) { return e.directions.indexOf(w.direction) >= 0; });
      return h('div', null, h('h2', { text: t('meth.q') }), h('div', { class: 'choices', role: 'radiogroup', 'aria-label': t('meth.q') }, ok.map(function (e) {
        var rec = e.status === 'stable' && w.direction !== 'reverse';
        var pre = e.status === 'preview' || w.direction === 'reverse';
        var lab = (((e.evidence || {}).lab_two_host) || {})[w.direction];
        var labPill = lab === 'pass' ? pill('ok', t('meth.lab.pass')) : pill('unknown', t('meth.lab.unknown'));
        return choice('engine', e.id, w.engine === e.id, [BAFT.ltr(e.id), pill(rec ? 'ok' : 'warn', rec ? t('meth.rec') + ' · ' + t('meth.stable') : pre ? t('dir.preview') : t('meth.experimental')), labPill], t('meth.' + e.id) + (e.id === 'baft' ? ' ' + t('meth.baft.limits') : '') + ((e.guided_setup || {}).available === false ? ' ' + t('meth.noguided') : '') + (w.direction === 'reverse' ? ' ' + t('meth.rev.note') : ''),
          function (v) { w.engine = v; invalidate(); draw(); }, null, (e.guided_setup || {}).available === false);
      })));
    }
    function stepPreflight() {
      var pf = w.pf;
      var list = pf ? h('ul', { class: 'pf' }, pf.checks.map(function (c) {
        var ev = c.code ? t(c.code, c.params || {}) : c.evidence;
        if (!c.code || ev === c.code) {
          ev = c.result === 'unknown' && BAFT_I18N.en['pfu.' + c.id] ? t('pfu.' + c.id)
            : c.id === 'pki_files' && c.result === 'warning' && BAFT_I18N.en['pfe.pki_files'] ? t('pfe.pki_files') : c.evidence;
        }
        var fix = c.result !== 'pass' && c.remediation ? (BAFT_I18N.en['pfr.' + c.id] ? t('pfr.' + c.id) : c.remediation) : null;
        var evNode = c.result === 'unknown' || BAFT.lang === 'fa' ? ev : BAFT.ltr(ev);
        return h('li', null, h('strong', { text: t('pfc.' + c.id) === 'pfc.' + c.id ? c.id : t('pfc.' + c.id) }),
          h('span', { class: 'muted', text: ' · ' + t(c.scope === 'peer' ? 'pf.scope.peer' : 'pf.scope.local') + ' ' }),
          pill({ pass: 'ok', warning: 'warn', blocked: 'bad', unknown: 'unknown' }[c.result], t('res.' + c.result)), h('span', { class: 'ev' }, evNode),
          fix ? h('span', { class: 'muted', text: ' ' + t('pf.fix') + ': ' + fix }) : null);
      })) : null;
      return h('div', null, h('h2', { text: t('pf.title') }), h('p', { class: 'sub', text: t('pf.note') }),
        h('button', { class: 'primary', type: 'button', disabled: w.busy ? true : null, onclick: runPreflight, text: w.busy ? t('pf.running') : t('pf.run') }),
        w.pfError ? h('p', { class: 'error', role: 'alert', text: w.pfError }) : null,
        pf ? h('div', null, h('p', null, t('pf.overall') + ': ', pill({ pass: 'ok', warning: 'warn', blocked: 'bad', unknown: 'unknown' }[pf.overall], t('res.' + pf.overall))), list, h('p', { class: 'sub', text: t('pf.unknownnote') })) : h('p', { class: 'sub', text: t('pf.stale') }));
    }
    function effects(p) {
      var pf = w.pf, cfg = p.config || {}, eng = engines.filter(function (e) { return e.id === w.engine; })[0] || {};
      var serve = cfg.mode === 'serve', rev = w.direction === 'reverse';
      var unknown = pf ? pf.checks.filter(function (c) { return c.result === 'unknown'; }).map(function (c) { return t('pfc.' + c.id) === 'pfc.' + c.id ? c.id : t('pfc.' + c.id); }) : [];
      var lab = ((eng.evidence || {}).lab_two_host || {})[w.direction];
      return h('div', null,
        h('h3', { text: t('pv.topology') }),
        kv([[t('sum.dir'), dirLabel(w.direction)], [t('pv.thisrole'), roleLabel(serve ? 'serve' : 'forward', w.direction)],
          [t('pv.otherrole'), roleLabel(serve ? 'forward' : 'serve', w.direction)], [t('sum.engine'), BAFT.ltr(w.engine)],
          [t('pv.listener'), serve ? h('span', null, BAFT.ltr(cfg.listen || '—'), ' (' + t('pv.thisserver') + ')') : t('pv.otherserver')],
          [t('pv.dialer'), serve ? t('pv.otherserver') : h('span', null, BAFT.ltr(cfg.remote || '—'), ' (' + t('pv.thisserver') + ' → ' + t('pv.otherserver') + ')')],
          [t(rev && serve ? 'pv.ingress' : 'pv.service'), BAFT.ltr(rev ? (serve ? (cfg.ingress || '—') : (cfg.target || '—')) : (serve ? (cfg.target || '—') : (cfg.listen || '—')))],
          [t('pv.labstate'), lab === 'pass' ? pill('ok', t('meth.lab.pass')) : pill('unknown', t('meth.lab.unknown'))]]),
        h('h3', { text: t('pv.thisserver') }), h('p', { text: t('pv.thisfx') }),
        h('h3', { text: t('pv.otherserver') }), h('p', { text: t('pv.otherfx') }),
        h('h3', { text: t('pv.fw') }), h('p', { text: t('pv.fwbody') }),
        h('h3', { text: t('pv.secrets') }), h('p', { text: t('pv.secretsbody') }),
        h('h3', { text: t('pv.rollback') }), h('p', { text: t('pv.rollbackbody') }),
        h('h3', { text: t('pv.unknowns') }),
        unknown.length ? h('ul', null, unknown.map(function (u) { return h('li', { text: u }); })) : h('p', { class: 'muted', text: pf ? t('pv.nounknown') : t('pv.nopf') }));
    }
    function stepPreview() {
      var p = w.plan;
      return h('div', null, h('h2', { text: t('pv.title') }), h('div', { class: 'banner info', text: t('pv.none') }),
        w.busy ? h('p', { text: t('pv.load') }) : null, w.planError ? h('p', { class: 'error', role: 'alert', text: w.planError }) : null,
        p ? effects(p) : null,
        p ? h('div', null, h('h3', { text: t('pv.files') }), h('pre', { text: p.files.map(function (x) { return x.path + ' [' + x.mode + '] ' + x.note; }).join('\n') }),
          h('h3', { text: t('pv.pending') }), h('pre', { text: p.pending.join('\n') }), h('h3', { text: t('pv.cfg') }), h('pre', { text: JSON.stringify(p.config, null, 2) })) : null);
    }
    function applyKey() {
      if (!w.applyKey) {
        var a = new Uint8Array(12); (window.crypto || {}).getRandomValues && window.crypto.getRandomValues(a);
        w.applyKey = 'ui-' + Array.prototype.map.call(a, function (x) { return ('0' + x.toString(16)).slice(-2); }).join('');
      }
      return w.applyKey;
    }
    function runApply() {
      w.busy = true; w.applyErr = null; draw();
      BAFT.api('POST', '/v1/setup/apply', { stage: 'init', confirm: true, idempotency_key: applyKey(), plan: body(w) }).then(function (r) {
        w.applyReceipt = r.body && r.body.result ? r.body : null;
        if (!w.applyReceipt) { w.applyErr = (r.body && (r.body.detail || r.body.error)) || t('fail'); }
      }, function (e) { w.applyErr = e.message; }).then(function () { w.busy = false; draw(); });
    }
    function receiptView(rc) {
      var ok = rc.result === 'applied';
      return h('section', { class: 'card', 'aria-live': 'polite' }, h('h3', { text: t('rc.title') }),
        h('p', null, pill(ok ? 'ok' : 'warn', t('rc.' + rc.result)), ' ', BAFT.ltr(rc.id || '')),
        rc.reason ? h('p', { text: rc.reason }) : null,
        h('h3', { text: t('rc.verify') }), h('ul', null, (rc.verification || []).map(function (v) { return h('li', null, pill(v.result === 'pass' ? 'ok' : 'warn', v.result), ' ', BAFT.ltr(v.id), h('span', { class: 'muted', text: ' · ' + v.evidence })); })),
        h('h3', { text: t('rc.changes') }), h('pre', { text: (rc.changes || []).map(function (c) { return c.action + ' ' + c.path + ' [' + c.mode + ']'; }).join('\n') || '—' }),
        rc.rollback && rc.rollback.performed ? h('p', { class: 'error', text: t('rc.rolled') + ' ' + (rc.rollback.removed || []).join(', ') }) : null,
        rc.pairing ? h('p', null, t('rc.checksum') + ' ', BAFT.ltr(rc.pairing.checksum)) : null,
        rc.not_done ? h('div', null, h('h3', { text: t('rc.notdone') }), h('ul', null, rc.not_done.map(function (x) { return h('li', { text: x }); }))) : null,
        rc.next_commands ? h('div', null, h('h3', { text: t('rc.next') }), h('pre', { text: rc.next_commands.join('\n') })) : null,
        h('p', { class: 'sub', text: t('rc.nosecret') }));
    }
    /* ---- Panel pairing (U5b): init, export own bundle, import peer bundle with checksum, join ---- */
    function newKey(prefix) {
      var a = new Uint8Array(12); (window.crypto || {}).getRandomValues && window.crypto.getRandomValues(a);
      return prefix + Array.prototype.map.call(a, function (x) { return ('0' + x.toString(16)).slice(-2); }).join('');
    }
    function b64url(text) { return btoa(unescape(encodeURIComponent(text))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''); }
    function pairLink(text) { return 'baft-tunnel://pair?v=1&b=' + b64url(text); }
    /* A pasted link or raw JSON -> bundle text; the checksum never travels in the link. */
    function bundleFromInput(text) {
      var v = (text || '').trim(), m = /^baft-tunnel:\/\/pair\?(?:.*&)?b=([A-Za-z0-9_-]+)/.exec(v);
      if (!m) { return v; }
      var b = m[1].replace(/-/g, '+').replace(/_/g, '/');
      while (b.length % 4) { b += '='; }
      try { return decodeURIComponent(escape(atob(b))); } catch (e) { return ''; }
    }
    function loadOwn() {
      BAFT.api('GET', '/v1/setup/bundle').then(function (r) { w.own = r.status === 200 ? r.body : null; draw(); }, function () {});
    }
    function pollReload(tries) {
      BAFT.api('GET', '/v1/status').then(function (r) {
        if (r.status === 200 && r.body && !r.body.setup_pending) { w.status = r.body; w.reloading = false; draw(); return; }
        if (tries > 0) { setTimeout(function () { pollReload(tries - 1); }, 2000); } else { w.reloading = false; draw(); }
      }, function () { if (tries > 0) { setTimeout(function () { pollReload(tries - 1); }, 2000); } });
    }
    function runStage(stage, extra) {
      w.busy = true; w.pairErr = null; draw();
      var req = { stage: stage, confirm: true, idempotency_key: stage === 'init' ? applyKey() : (w.joinKey = w.joinKey || newKey('ui-join-')), plan: body(w) };
      Object.keys(extra || {}).forEach(function (k) { req[k] = extra[k]; });
      BAFT.api('POST', '/v1/setup/apply', req).then(function (r) {
        var rc = r.body && r.body.result ? r.body : null;
        if (!rc) { w.pairErr = (r.body && (r.body.detail || r.body.error)) || t('fail'); return; }
        if (stage === 'init') { w.applyReceipt = rc; if (rc.result === 'applied') { loadOwn(); } }
        else {
          w.joinReceipt = rc;
          if (rc.result !== 'applied') { w.joinKey = null; } else { w.reloading = true; setTimeout(function () { pollReload(15); }, 2000); }
        }
      }, function (e) { w.pairErr = e.message; }).then(function () { w.busy = false; draw(); });
    }
    function inspectPeer() {
      var text = bundleFromInput(w.peerText);
      w.inspect = null; w.inspectErr = null;
      if (!text) { w.inspectErr = t('pair.badlink'); draw(); return; }
      w.peerBundle = text; w.busy = true; draw();
      BAFT.api('POST', '/v1/setup/bundle/inspect', { bundle: text, checksum: (w.peerCk || '').trim() || null }).then(function (r) {
        if (r.status === 200) { w.inspect = r.body; } else { w.inspectErr = (r.body && (r.body.detail || r.body.error)) || t('fail'); }
      }, function (e) { w.inspectErr = e.message; }).then(function () { w.busy = false; draw(); });
    }
    function copyText(text) {
      if (navigator.clipboard) { navigator.clipboard.writeText(text).then(function () { BAFT.toast(t('ap.copied')); }, function () {}); }
    }
    function download(name, text) {
      var a = h('a', { href: URL.createObjectURL(new Blob([text], { type: 'application/json' })), download: name });
      document.body.appendChild(a); a.click(); setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 0);
    }
    /* An applied receipt folds away (evidence on demand); a failed one stays open. */
    function folded(rc) {
      return rc.result === 'applied' ? h('details', null, h('summary', { text: t('rc.title') + ' · ' + t('rc.applied') }), receiptView(rc)) : receiptView(rc);
    }
    function pkiEnrollmentSection() {
      var out = [h('p', { class: 'sub', text: t('pki.intro') })];
      var identityReady = !!(w.node.trim() && w.localServerName.trim());
      if (!identityReady) {
        out.push(h('p', { class: 'fielderr', role: 'alert', text: t('pki.identityRequired') }));
        return h('div', null, out);
      }
      if (w.pkiError) { out.push(h('p', { class: 'error', role: 'alert', text: w.pkiError })); }
      if (!w.csrPem) {
        out.push(h('div', { class: 'nav-row' },
          h('button', { type: 'button', class: 'primary', disabled: w.pkiBusy ? true : null, text: w.pkiBusy ? t('pki.working') : t('pki.create'), onclick: function () { requestCsr('create'); } }),
          h('button', { type: 'button', disabled: w.pkiBusy ? true : null, text: t('pki.recover'), onclick: function () { requestCsr('read'); } })));
        return h('div', null, out);
      }
      out.push(kv([
        [t('pki.node'), BAFT.ltr(w.node.trim())],
        [t('pki.san'), BAFT.ltr(w.csrReceipt && w.csrReceipt.expected_server_san || w.localServerName.trim())],
        [t('pki.keyFingerprint'), BAFT.ltr(w.csrReceipt && w.csrReceipt.public_key_sha256 || '—')]
      ]));
      out.push(h('label', { class: 'field' }, h('span', { text: t('pki.csr') }),
        h('textarea', { rows: 5, readonly: true, dir: 'ltr', 'aria-label': t('pki.csr') }, w.csrPem)));
      out.push(h('div', { class: 'nav-row' },
        h('button', { type: 'button', text: t('pki.copy'), onclick: function () { copyText(w.csrPem); } }),
        h('button', { type: 'button', text: t('pki.download'), onclick: function () {
          var a = h('a', { href: URL.createObjectURL(new Blob([w.csrPem], { type: 'application/pkcs10' })), download: w.node.trim() + '.csr' });
          document.body.appendChild(a); a.click(); setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 0);
        } })));
      out.push(h('h4', { text: t('pki.offlineTitle') }), h('p', { class: 'sub', text: t('pki.offlineBody') }),
        h('pre', { dir: 'ltr', text: 'scripts/pki/baft-pki.sh issue-node CA_DIR ' + w.node.trim() + ' CSR_FILE OUTPUT_DIR ' + w.localServerName.trim() }));
      out.push(h('h4', { text: t('pki.uploadTitle') }), h('p', { class: 'sub', text: t('pki.uploadBody') }));
      function pemInput(key, label, marker, maxBytes) {
        var input = h('input', { type: 'file', accept: '.pem,.crt,.cer,.crl,application/x-pem-file,text/plain', 'aria-label': label, onchange: function (e) {
          var file = e.target.files && e.target.files[0];
          w[key] = '';
          if (!file) { draw(); return; }
          if (file.size <= 0 || file.size > maxBytes) { w.pkiError = t('pki.fileSize'); draw(); return; }
          var reader = new FileReader();
          reader.onload = function () {
            var pem = String(reader.result || '').trim();
            var value = pem + '\n';
            var begin = marker[0], end = marker[1];
            if (pem.indexOf('PRIVATE KEY') >= 0 || pem.indexOf(begin) !== 0 || !pem.endsWith(end)
              || (pem.match(/-----BEGIN /g) || []).length !== 1 || (pem.match(/-----END /g) || []).length !== 1) {
              w.pkiError = t('pki.badPem'); w[key] = '';
            } else { w.pkiError = null; w[key] = value; }
            draw();
          };
          reader.onerror = function () { w.pkiError = t('pki.fileRead'); draw(); };
          reader.readAsText(file);
        } });
        return h('label', { class: 'field pki-upload' }, h('span', { text: label }), input,
          w[key] ? h('span', { class: 'sub', text: t('pki.fileLoaded') }) : null);
      }
      out.push(pemInput('certificatePem', t('pki.leaf'), ['-----BEGIN CERTIFICATE-----', '-----END CERTIFICATE-----'], 64 * 1024));
      out.push(pemInput('caPem', t('pki.ca'), ['-----BEGIN CERTIFICATE-----', '-----END CERTIFICATE-----'], 64 * 1024));
      out.push(pemInput('crlPem', t('pki.crl'), ['-----BEGIN X509 CRL-----', '-----END X509 CRL-----'], 256 * 1024));
      out.push(h('label', { class: 'sub pki-confirm' }, h('input', { type: 'checkbox', checked: w.pkiConfirm ? true : null, onchange: function (e) { w.pkiConfirm = e.target.checked; draw(); } }), ' ', t('pki.confirm')));
      out.push(h('button', { type: 'button', class: 'primary', disabled: (!w.certificatePem || !w.caPem || !w.crlPem || !w.pkiConfirm || w.pkiBusy) ? true : null,
        text: w.pkiBusy ? t('pki.working') : t('pki.install'), onclick: installPki }));
      if (w.pkiReceipt && w.pkiReceipt.result === 'installed') {
        out.push(h('div', { class: 'banner info', role: 'status' }, h('strong', { text: t('pki.installed') }),
          h('p', { text: t('pki.fingerprint') + ': ' + (w.pkiReceipt.certificate_sha256 || '') }),
          h('p', { text: t('pki.validUntil') + ': ' + (w.pkiReceipt.certificate_not_after ? new Date(w.pkiReceipt.certificate_not_after * 1000).toISOString() : '—') }),
          h('p', { text: t('pki.crlRemaining') + ': ' + (w.pkiReceipt.crl_seconds_remaining || 0) + 's' }),
          h('p', { text: t('pki.noRestart') })));
      }
      return h('div', null, out);
    }
    function requestCsr(operation) {
      w.pkiBusy = true; w.pkiError = null; draw();
      var req = { request_id: newKey('ui-csr-'), node_id: w.node.trim(), server_name: w.localServerName.trim() };
      if (operation === 'create') { req.confirm = true; }
      BAFT.api('POST', '/v1/pki/csr/' + operation, req).then(function (r) {
        if ((r.status !== 200 && r.status !== 201) || !r.body || !r.body.csr_pem
          || r.body.node_id !== w.node.trim() || r.body.private_key_exported !== false
          || !r.body.expected_server_san || !r.body.public_key_sha256 || r.body.public_key_sha256.length !== 64) {
          w.pkiError = (r.body && (r.body.detail || r.body.error)) || t('pki.invalidCsrReceipt'); return;
        }
        w.csrReceipt = r.body; w.csrPem = r.body.csr_pem;
      }, function (e) { w.pkiError = e.message; }).then(function () { w.pkiBusy = false; draw(); });
    }
    function installPki() {
      w.pkiBusy = true; w.pkiError = null; draw();
      var req = {
        request_id: newKey('ui-cert-'), node_id: w.node.trim(), server_name: w.localServerName.trim(), confirm: true,
        certificate_pem: w.certificatePem, ca_pem: w.caPem, crl_pem: w.crlPem
      };
      BAFT.api('POST', '/v1/pki/certificate/install', req).then(function (r) {
        if (r.status !== 200 || !r.body || r.body.result !== 'installed' || r.body.private_key_exported !== false || r.body.service_restarted !== false) {
          w.pkiError = (r.body && (r.body.detail || r.body.error)) || t('fail'); w.pkiReceipt = null; return;
        }
        w.pkiReceipt = r.body;
      }, function (e) { w.pkiError = e.message; }).then(function () { w.pkiBusy = false; draw(); });
    }

    function pairSection(st) {
      var pr = st.pairing || {}, helper = st.setup_helper || {};
      var scopeOk = w.inst.trim() === (st.instance || '') && w.dir.trim() === (st.config_dir || '');
      var initDone = pr.initialized || (w.applyReceipt && w.applyReceipt.result === 'applied');
      var pkiRequired = w.engine === 'tcp-tls';
      var pkiReady = !pkiRequired || (w.pkiReceipt && w.pkiReceipt.result === 'installed');
      if (initDone && !w.own && !w.ownLoading) { w.ownLoading = true; loadOwn(); }
      var out = [h('p', { class: 'sub', text: t('pair.intro') })];
      if (pkiRequired && !initDone) {
        out.push(h('h3', { text: t('pki.title') }), pkiEnrollmentSection());
        if (!pkiReady) { out.push(h('p', { class: 'fielderr', role: 'status', text: t('pki.mustInstall') })); }
      }
      if (!helper.available) { out.push(h('p', { class: 'error', role: 'alert', text: t('pair.nohelper') + ' ' + (helper.reason || '') })); }
      if (!scopeOk) { out.push(h('p', { class: 'fielderr', role: 'alert', text: t('pair.scope', { inst: st.instance || '—', dir: st.config_dir || '—' }) })); }
      // a) prepare
      var chk = h('label', { class: 'pair-a-confirm' }, h('input', { type: 'checkbox', checked: w.applyConfirm ? 'checked' : null, onchange: function (e) { w.applyConfirm = e.target.checked; draw(); } }), ' ', t('ap.confirm'));
      out.push(h('h3', { text: t('pair.a') }), initDone ? h('p', null, pill('ok', t('pair.a.done'))) : h('div', null, h('div', { class: 'banner' }, h('p', { text: t('pair.a.body') })), chk,
        h('button', { type: 'button', class: 'primary', text: t('pair.a.go'), disabled: (!w.applyConfirm || w.busy || !helper.available || !scopeOk || !pkiReady || (w.pf && w.pf.overall === 'blocked')) ? 'disabled' : null, onclick: function () { runStage('init'); } })));
      if (w.applyReceipt) { out.push(folded(w.applyReceipt)); }
      // b) export
      out.push(h('h3', { text: t('pair.b') }));
      if (!initDone) { out.push(h('p', { class: 'muted', text: t('pair.b.wait') })); }
      else if (!w.own) { out.push(h('p', { class: 'muted', text: t('pv.load') })); }
      else {
        out.push(h('p', null, t('rc.checksum') + ' ', h('strong', null, BAFT.ltr(w.own.checksum))), h('p', { class: 'sub', text: t('pair.b.oob') }),
          h('div', { class: 'nav-row' },
            h('button', { type: 'button', text: t('pair.b.copy'), onclick: function () { copyText(w.own.bundle); } }),
            h('button', { type: 'button', text: t('pair.b.link'), onclick: function () { copyText(pairLink(w.own.bundle)); } }),
            h('button', { type: 'button', text: t('pair.b.dl'), onclick: function () { download('baft-bundle-' + (st.instance || 'node') + '.json', w.own.bundle); } })),
          h('details', null, h('summary', { text: t('pair.b.show') }), h('pre', { dir: 'ltr', text: w.own.bundle })));
      }
      // c) import + join
      out.push(h('h3', { text: t('pair.c') }));
      if (pr.joined || (w.joinReceipt && w.joinReceipt.result === 'applied')) {
        out.push(h('p', null, pill('ok', t('pair.c.done'))), w.joinReceipt ? folded(w.joinReceipt) : null,
          w.reloading ? h('p', { class: 'sub', role: 'status', text: t('pair.reloading') }) : null);
        return h('div', null, out);
      }
      var ta = h('textarea', { rows: 5, dir: 'ltr', 'aria-label': t('pair.c.paste'), placeholder: '{"version":1,...}  /  baft-tunnel://pair?v=1&b=...', oninput: function () { w.peerText = ta.value; w.inspect = null; } });
      ta.value = w.peerText || '';
      var file = h('input', { type: 'file', accept: '.json,application/json', 'aria-label': t('pair.c.file'), onchange: function (e) {
        var f = e.target.files && e.target.files[0];
        if (!f || f.size > 4096) { w.inspectErr = t('pair.toolarge'); draw(); return; }
        var rd = new FileReader(); rd.onload = function () { w.peerText = String(rd.result); w.inspect = null; draw(); }; rd.readAsText(f);
      } });
      var ck = h('input', { type: 'text', dir: 'ltr', autocomplete: 'off', value: w.peerCk || '', placeholder: 'xxxxxxxx-xxxxxxxx-xxxxxxxx-xxxxxxxx', oninput: function () { w.peerCk = ck.value; w.inspect = null; } });
      out.push(h('p', { class: 'sub', text: t('pair.c.body') }), h('label', { class: 'field' }, h('span', { text: t('pair.c.paste') }), ta),
        h('label', { class: 'field' }, h('span', { text: t('pair.c.file') }), file),
        h('label', { class: 'field' }, h('span', { text: t('pair.c.ck') }), ck),
        h('button', { type: 'button', text: t('pair.c.check'), disabled: (w.busy || !initDone) ? 'disabled' : null, onclick: inspectPeer }),
        w.inspectErr ? h('p', { class: 'error', role: 'alert', text: w.inspectErr }) : null);
      var v = w.inspect;
      if (v) {
        var f = v.fields || {};
        var good = v.checksum_match === true && v.matches_this_node === true;
        out.push(h('section', { class: 'card', 'aria-live': 'polite' }, h('h3', { text: t('pair.c.preview') }),
          kv([[t('sum.tunnel'), BAFT.ltr(f.tunnel_id)], [t('pair.f.node'), BAFT.ltr(f.node_id)], [t('pair.f.peer'), BAFT.ltr(f.peer_id)],
            [t('pair.f.role'), roleLabel(f.role, f.direction)], [t('sum.engine'), BAFT.ltr(f.engine)], [t('sum.dir'), dirLabel(f.direction)],
            [t('pair.f.ports'), BAFT.ltr(f.service_port + ' / ' + f.tunnel_port)], [t('rc.checksum'), BAFT.ltr(v.checksum)]]),
          h('p', null, v.checksum_match === true ? pill('ok', t('pair.ck.ok')) : v.checksum_match === false ? pill('bad', t('pair.ck.bad')) : pill('unknown', t('pair.ck.none'))),
          v.matches_this_node === false ? h('p', { class: 'error', role: 'alert', text: t('pair.mismatch') + ' ' + (v.mismatches || []).join(', ') }) : null,
          good ? h('div', { class: 'stack' }, h('label', null, h('input', { type: 'checkbox', checked: w.joinConfirm ? 'checked' : null, onchange: function (e) { w.joinConfirm = e.target.checked; draw(); } }), ' ', t('pair.c.confirm')),
            h('button', { type: 'button', class: 'primary', text: t('pair.c.go'), disabled: (!w.joinConfirm || w.busy) ? 'disabled' : null,
              onclick: function () { runStage('join', { bundle: w.peerBundle, checksum: (w.peerCk || '').trim() }); } })) : null));
      }
      if (w.joinReceipt) { out.push(folded(w.joinReceipt)); }
      return h('div', null, out);
    }
    function stepApply() {
      var cmds = w.plan ? w.plan.commands.join('\n') : '';
      var copy = h('button', { type: 'button', text: t('ap.copy'), onclick: function () {
        if (navigator.clipboard) { navigator.clipboard.writeText(cmds).then(function () { BAFT.toast(t('ap.copied')); }, function () {}); }
      } });
      var gated = w.pf && w.pf.overall === 'blocked';
      var go = h('button', { type: 'button', class: 'primary', text: t('ap.go'), disabled: (!w.applyConfirm || w.busy || gated || (w.applyReceipt && w.applyReceipt.result === 'applied')) ? 'disabled' : null, onclick: runApply });
      var chk = h('label', null, h('input', { type: 'checkbox', checked: w.applyConfirm ? 'checked' : null, onchange: function (e) { w.applyConfirm = e.target.checked; draw(); } }), ' ', t('ap.confirm'));
      // Last step (U5a): 1. the printed setup commands, run as root; 2. Start once the config exists
      // (the agent is bound to the instance); 3. diagnostics with one next action.
      var tn = (w.status || (ctx.data.status || {})).tunnel || null;
      var recheck = h('button', { type: 'button', disabled: w.busy ? true : null, text: t('fin.recheck'), onclick: function () {
        w.busy = true; draw();
        BAFT.api('GET', '/v1/status').then(function (r) { if (r.status === 200) { w.status = r.body; } }, function () {}).then(function () { w.busy = false; draw(); });
      } });
      var diagOut = h('div', { 'aria-live': 'polite' });
      if (w.diag) { diagOut.appendChild(diagTable(w.diag)); }
      var diagBtn = h('button', { type: 'button', text: t('diag.run'), onclick: function () {
        diagBtn.disabled = true; diagOut.textContent = t('diag.running');
        BAFT.api('POST', '/v1/diagnostics').then(function (r) {
          diagBtn.disabled = false; diagOut.textContent = '';
          if (r.status === 200) { w.diag = r.body; diagOut.appendChild(diagTable(r.body)); } else { diagOut.textContent = t('diag.failed'); }
        }, function () { diagBtn.disabled = false; diagOut.textContent = t('diag.failed'); });
      } });
      var bound = tn && tn.tiers && tn.tiers.unit && !(tn.tiers.unit.result === 'unknown' && tn.tiers.unit.reason);
      var st = w.status || ctx.data.status || {};
      // Panel pairing while the agent is in setup mode; it stays visible after join until reload.
      var pairing = !!(st.setup_pending || w.joinReceipt || w.applyReceipt && ctx.data.status && ctx.data.status.setup_pending);
      return h('div', null, h('h2', { text: t('fin.title') }), h('p', { class: 'sub', text: t('fin.intro') }),
        h('ol', { class: 'stack fin' },
          pairing ? h('li', null, h('h3', { text: t('pair.title') }), pairSection(st), w.pairErr ? h('p', { class: 'error', role: 'alert', text: w.pairErr }) : null,
            h('details', null, h('summary', { text: t('pair.alt') }), h('p', { text: t('ap.run.root') }), h('pre', { dir: 'ltr', text: cmds || '—' }), copy)) : null,
          pairing ? null : h('li', null, h('h3', { text: t('fin.s1') }), h('p', { text: t('ap.run.root') }), h('pre', { dir: 'ltr', text: cmds || '—' }), copy,
            h('p', { class: 'sub', text: t('fin.s1.peer') }),
            h('details', null, h('summary', { text: t('fin.agentapply') }), h('div', { class: 'banner' }, h('p', { text: t('ap.body') })),
              chk, go, w.applyErr ? h('p', { class: 'error', role: 'alert', text: w.applyErr }) : null, w.applyReceipt ? receiptView(w.applyReceipt) : null)),
          h('li', null, h('h3', { text: t('fin.s2') }),
            bound ? lifecycle(tn) : st.setup_pending ? h('p', { class: 'sub', text: t('pair.s2.wait') }) : h('div', null, h('p', { class: 'sub', text: t('fin.s2.wait') }), h('pre', { dir: 'ltr', text: 'sudo scripts/install.sh --register ' + (w.inst.trim() || 'NAME') })),
            recheck),
          h('li', null, h('h3', { text: t('fin.s3') }), h('p', { class: 'sub', text: t('diag.note') }), diagBtn, diagOut)));
    }
    function summary() {
      var det = h('details', null, h('summary', { text: t('sum.title') }), kv([
        [t('sum.dir'), w.direction ? dirLabel(w.direction) : t('sum.unset')], [t('sum.role'), roleLabel(w.role === 'serve' ? 'serve' : 'forward')],
        [t('sum.engine'), BAFT.ltr(w.engine)], [t('sum.tunnel'), w.tunnel ? BAFT.ltr(w.tunnel) : t('sum.unset')]]));
      if (window.innerWidth >= 900) { det.setAttribute('open', ''); }
      return h('section', { class: 'card' }, det);
    }
    function draw() {
      root.textContent = '';
      var pfBlocked = w.step === 3 && w.pf && w.pf.overall === 'blocked';
      var step = [stepDirection, stepServers, stepMethod, stepPreflight, stepPreview, stepApply][w.step]();
      var needPf = w.step === 3 && !w.pf;
      var nextDisabled = pfBlocked || needPf || (w.step === 4 && !w.plan) || w.busy;
      root.appendChild(h('div', null, h('p', { class: 'muted', text: t('setup.intro') }), stepper(),
        h('div', { class: 'split' },
          h('section', { class: 'card' }, h('p', { class: 'sub', text: t('step.n', { n: BAFT.fmtNum(w.step + 1, 0), m: BAFT.fmtNum(STEPS.length, 0) }) }), step,
            pfBlocked ? h('p', { class: 'fielderr', role: 'alert', text: t('next.blocked') }) : null,
            h('div', { class: 'nav-row' },
              h('button', { type: 'button', disabled: w.step === 0 ? true : null, onclick: function () { go(w.step - 1); }, text: t('back') }),
              w.step < STEPS.length - 1 ? h('button', { class: 'primary', type: 'button', disabled: nextDisabled ? true : null, onclick: function () { go(w.step + 1); }, text: t('next') }) : null)),
          summary())));
    }
    draw();
    root.appendChild(live);
    return root;
  } });

  /* ---- Operations guidance: read-only. The agent is unprivileged, so the panel never runs these. ---- */
  function opsBody() {
    var cmds = [['ops.backup', 'sudo scripts/backup.sh'], ['ops.restore', 'sudo scripts/restore.sh --stop --yes BACKUP_DIR'],
      ['ops.update', 'sudo scripts/update.sh --binary NEW_BINARY --instance NAME'], ['ops.rollback', 'sudo scripts/rollback.sh']];
    return h('div', null, h('p', { class: 'sub', text: t('ops.note') }),
      h('ul', { class: 'warns' }, cmds.map(function (c) { return h('li', null, h('strong', { text: t(c[0]) }), h('div', { class: 'sub', text: t(c[0] + '.d') }), h('code', { dir: 'ltr', text: c[1] })); })),
      h('p', { class: 'sub', text: t('ops.limits') }));
  }

  /* ---- Agent ---- */
  BAFT.register({ id: 'agent', icon: 'agent', order: 5, needs: ['history', 'audit', 'engines'], render: function (ctx) {
    var d = ctx.data, s = d.status, hist = d.history || {};
    var out = h('div', { 'aria-live': 'polite' });
    function check() {
      var t0 = performance.now();
      fetch('/v1/health').then(function (r) { if (!r.ok) { throw new Error('x'); } return r.json(); }).then(function () {
        out.textContent = t('agent.healthok', { ms: BAFT.fmtNum(performance.now() - t0, 0) });
      }, function () { out.textContent = t('agent.healthfail'); });
    }
    return h('div', { class: 'stack' }, h('div', { class: 'grid cols-2' },
      card(t('agent.health'), h('div', null, kv([[t('f.version'), BAFT.ltr('v' + s.version)], [t('f.lastseen'), t('checked', { ago: BAFT.ago(BAFT.fetchedAt) })]]),
        h('p', { class: 'sub', text: t('agent.sampler', { s: BAFT.fmtNum(hist.interval_s || 0, 0), n: BAFT.fmtNum(hist.capacity || 0, 0) }) }),
        h('button', { type: 'button', onclick: check, text: t('agent.check') }), out)),
      card(t('agent.caps'), h('div', null,
        h('ul', { class: 'warns' },
          h('li', null, t('cap.audit') + ': ', pill(d.audit === null ? 'unknown' : 'ok', d.audit === null ? t('cap.off') : t('cap.on'))),
          h('li', null, t('cap.desired') + ': ', pill(d.audit === null ? 'unknown' : 'ok', d.audit === null ? t('cap.off') : t('cap.on'))),
          h('li', null, t('cap.plan') + ': ', pill('ok', t('cap.on'))),
          h('li', null, t('cap.apply') + ': ', pill('ok', t('cap.on')))),
        h('p', { class: 'sub', text: t('agent.loopback') }), h('p', { class: 'sub', text: t('agent.secret') }))),
      ), card(t('ops.title'), opsBody()));
  } });

  /* ---- Activity ---- */
  function opLabel(e) {
    var k = 'op.' + (e.resource || 'desired_state');
    var v = t(k);
    return v === k ? BAFT.ltr(e.resource || e.action || '—') : v;
  }
  var activityFilter = 'all';
  BAFT.register({ id: 'activity', icon: 'activity', order: 6, needs: ['audit'], render: function (ctx) {
    var a = ctx.data.audit;
    if (!a) { return card(t('activity.title'), empty(t('activity.disabled'))); }
    var all = (a.entries || []).slice().reverse();
    var rows = all.filter(function (e) { return activityFilter === 'all' || e.result === activityFilter; });
    var sel = h('select', { 'aria-label': t('filter.result'), onchange: function () { activityFilter = sel.value; BAFT.refresh(true); } },
      [['all', t('filter.all')], ['accepted', t('accepted')], ['rejected', t('rejected')]].map(function (o) { return h('option', { value: o[0], text: o[1], selected: o[0] === activityFilter ? true : null }); }));
    var table = rows.length ? h('div', { class: 'table-wrap' }, h('table', { class: 'cards' },
      h('thead', null, h('tr', null, ['col.time', 'col.op', 'col.actor', 'col.target', 'col.result', 'col.receipt', 'col.detail'].map(function (k) { return h('th', { scope: 'col', text: t(k) }); }))),
      h('tbody', null, rows.map(function (e) {
        return h('tr', null,
          h('td', { 'data-label': t('col.time'), text: new Date(e.ts * 1000).toLocaleString(BAFT.lang === 'fa' ? 'fa-IR' : 'en-GB') }),
          h('td', { 'data-label': t('col.op') }, opLabel(e)),
          h('td', { 'data-label': t('col.actor') }, e.actor ? BAFT.ltr(t('actor.token') + ' ' + String(e.actor.fingerprint || '').slice(0, 8)) : t('unknown')),
          h('td', { 'data-label': t('col.target') }, e.target ? BAFT.ltr([e.target.node, e.target.instance].filter(Boolean).join(' / ')) : '—'),
          h('td', { 'data-label': t('col.result') }, pill(e.result === 'accepted' ? 'ok' : 'bad', t(e.result) === e.result ? e.result : t(e.result))),
          h('td', { 'data-label': t('col.receipt') }, h('code', { text: String(e.receipt_id || e.sha256 || '').slice(0, 12) })),
          h('td', { 'data-label': t('col.detail') }, e.detail ? BAFT.ltr(e.detail) : '—'));
      })))) : empty(t('activity.empty'));
    return card(t('activity.title'), h('div', null, h('p', { class: 'sub', text: t('activity.note') + ' ' + t('activity.actor') }),
      h('label', { class: 'field' }, h('span', { text: t('filter.result') }), sel), table));
  } });

  /* ---- Settings ---- */
  BAFT.register({ id: 'settings', icon: 'settings', order: 7, refresh: false, render: function (ctx) {
    var s = ctx.data.status || {};
    function choice(cur, opts, onpick) {
      return h('div', { class: 'row', role: 'group' }, opts.map(function (o) {
        return h('button', { type: 'button', class: o[0] === cur ? 'primary' : '', 'aria-pressed': o[0] === cur ? 'true' : 'false', onclick: function () { onpick(o[0]); }, text: o[1] });
      }));
    }
    return h('div', { class: 'grid cols-2' },
      card(t('settings.title'), h('div', null,
        h('div', { class: 'field' }, h('span', { class: 'sub', text: t('settings.language') }), choice(BAFT.lang, Object.keys(BAFT_LANGS).map(function (k) { return [k, BAFT_LANGS[k].label]; }),
          function (k) { BAFT.lang = k; BAFT.applyLang(); BAFT.start(); })),
        h('div', { class: 'field' }, h('span', { class: 'sub', text: t('settings.theme') }), choice(BAFT.theme, [['dark', t('theme.dark')], ['light', t('theme.light')], ['auto', t('theme.auto')]],
          function (k) { BAFT.theme = k; BAFT.applyTheme(); BAFT.start(); })),
        h('p', { class: 'sub', text: t('settings.later') }))),
      card(t('settings.about'), h('div', null, kv([[t('settings.version'), s.version ? BAFT.ltr('v' + s.version) : null]]),
        h('h3', { text: t('settings.session') }), h('p', { class: 'sub', text: t('settings.sessionhint') }),
        h('button', { type: 'button', onclick: BAFT.logout, text: t('logout') }))));
  } });
})();
