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
      if (ls != null && ls < 3 * 86400) { out.push({ sev: ls < 86400 ? 'bad' : 'warn', res: 'CRL', text: t('warn.crl'), next: t('warn.crl.next') }); }
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
  function dirLabel(x) { return x ? t('dir.' + x) : '—'; }
  function roleLabel(x) { return x ? t('role.' + x) : '—'; }

  /* ---- Overview ---- */
  BAFT.register({ id: 'overview', icon: 'overview', order: 1, primary: true, needs: ['history', 'security', 'audit', 'desired'], render: function (ctx) {
    var d = ctx.data, s = d.status, c = conf(d), st = stats(d), ws = warnings(d), hist = st.hist;
    var entries = ((d.audit || {}).entries || []).slice().reverse();
    var fresh = !d.desired && !entries.length;
    var kpis = h('div', { class: 'grid kpis' },
      kpi(t('kpi.servers'), '1', t('kpi.servers.hint')),
      kpi(t('kpi.tunnels'), '1', t('kpi.tunnels.hint'), listenState(s)),
      kpi(t('kpi.attention'), BAFT.fmtNum(ws.filter(function (w) { return w.sev !== 'info'; }).length, 0), t('kpi.attention.hint')),
      kpi(t('kpi.traffic'), t('kpi.traffic.value'), t('kpi.traffic.hint'), { k: 'unknown', l: t('unknown') }));
    var timeline = hist.length ? h('div', null, C.strip(hist, { label: t('ov.health') }),
      h('div', { class: 'legend' }, h('span', { text: BAFT.fmtTime(hist[0].t) }), h('span', { text: BAFT.fmtTime(hist[hist.length - 1].t) }))) : empty(t('empty.history'));
    var pts = hist.map(function (x) { return { x: x.t, y: x.rtt_ms }; });
    var latency = hist.length && st.med != null ? h('div', null, C.line(pts, { label: t('ov.latency') + ' (' + t('ov.latency.unit') + ')', fmtX: BAFT.fmtTime, fmtY: function (v) { return BAFT.fmtNum(v, 1); } }),
      h('div', { class: 'legend' }, h('span', { text: t('ov.latency.unit') }), h('span', { text: BAFT.fmtNum(st.med, 2) + ' ms' }))) : empty(hist.length ? t('ov.latency.none') : t('empty.history'));
    var src = h('p', { class: 'sub', text: t('ov.health.src', { s: BAFT.fmtNum((d.history || {}).interval_s || 5, 0), n: BAFT.fmtNum((d.history || {}).capacity || 0, 0) }) });
    var avail = st.avail == null ? null : h('p', { class: 'sub', text: t('ov.avail') + ': ' + BAFT.fmtNum(st.avail, 1) + '% · ' + t('ov.window', { n: BAFT.fmtNum(hist.length, 0) }) });
    var recent = entries.length ? h('ul', { class: 'warns' }, entries.slice(0, 5).map(function (e) {
      return h('li', null, h('div', { class: 'top' }, pill(e.result === 'accepted' ? 'ok' : 'bad', t(e.result) === e.result ? e.result : t(e.result)), h('span', { class: 'sub', text: new Date(e.ts * 1000).toLocaleString(BAFT.lang === 'fa' ? 'fa-IR' : 'en-GB') })),
        h('div', { text: t('op.desired') }));
    })) : h('p', { class: 'muted', text: d.audit === null ? t('activity.disabled') : t('ov.recent.empty') });
    return h('div', null,
      fresh ? h('div', { class: 'banner info' }, h('h2', { text: t('ov.start.title') }), h('p', { text: t('ov.start.body') }), h('a', { class: 'btn primary', href: '#/setup', text: t('ov.start.cta') })) : null,
      kpis,
      h('div', { class: 'split' },
        h('div', { class: 'stack' },
          card(t('ov.health'), h('div', null, timeline, src, avail)),
          card(t('ov.latency'), latency),
          card(t('ov.traffic.title'), empty(t('ov.traffic.body')))),
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
      h('div', { class: 'head' }, h('div', null, h('div', { class: 'title' }, BAFT.ltr(c.local_identity)), h('div', { class: 'sub', text: t('servers.this') + ' · ' + roleLabel(c.mode) })), pill(ls.k, ls.l)),
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

  /* ---- Tunnels ---- */
  BAFT.register({ id: 'tunnels', icon: 'tunnels', order: 3, primary: true, needs: ['history', 'security'], render: function (ctx) {
    var d = ctx.data, s = d.status, c = conf(d), st = stats(d), ls = listenState(s);
    var res = h('section', { class: 'card res' },
      h('div', { class: 'head' }, h('div', null, h('div', { class: 'title' }, BAFT.ltr(c.tunnel_id)),
        h('div', { class: 'sub' }, dirLabel(c.direction) + ' · ', BAFT.ltr(c.local_identity), ' ⇄ ', BAFT.ltr(c.peer_identity))), pill(ls.k, ls.l)),
      h('p', { class: 'sub', text: t('state.why.local') + ' ' + t('checked', { ago: BAFT.ago(BAFT.fetchedAt) }) }),
      kv([[t('f.engine'), BAFT.ltr(s.engine)], [t('f.role'), roleLabel(c.mode)], [t('f.peerconn'), pill('unknown', t('state.unknown'))],
        [t('f.verified'), t('f.verified.none')]]),
      h('hr', { class: 'sep' }), h('h3', { text: t('tunnels.metrics') }),
      kv([[t('f.latency'), st.med == null ? t('unknown') : BAFT.ltr(BAFT.fmtNum(st.med, 2) + ' ms')], [t('f.avail'), st.avail == null ? t('unknown') : [BAFT.ltr(BAFT.fmtNum(st.avail, 1) + '%'), ' · ' + t('ov.window', { n: BAFT.fmtNum(st.hist.length, 0) })]]]),
      h('hr', { class: 'sep' }), h('h3', { text: t('tunnels.config') }),
      kv([[t('f.streams'), BAFT.fmtNum(c.max_streams || 0, 0)], [t('f.pin'), c.peer_certificate_pin_configured == null ? '—' : c.peer_certificate_pin_configured ? t('yes') : t('no')]]),
      h('hr', { class: 'sep' }), h('h3', { text: t('tunnels.actions') }), h('p', { class: 'sub', text: t('tunnels.noactions' ) }));
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
    return { step: 0, direction: null, role: 'forward', engine: 'tcp-tls', node: c.local_identity || '', peerId: '', tunnel: '', port: '', peerHost: '', serverName: '',
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
    var engines = ((ctx.data.engines || {}).engines || []);
    var root = h('div'), live = h('div', { 'aria-live': 'polite' });
    function invalidate() { w.pf = null; w.plan = null; }
    function field(key, label, opts) {
      opts = opts || {};
      var input = h('input', { value: w[key], type: opts.type || 'text', autocomplete: 'off', inputmode: opts.inputmode, 'aria-invalid': w.errors[key] ? 'true' : null,
        'aria-describedby': w.errors[key] ? 'e-' + key : null, oninput: function () { w[key] = input.value; invalidate(); } });
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
      var fwd = w.role === 'forward';
      return h('div', null, h('h2', { text: t('srv.q') }), h('p', { class: 'sub', text: t('srv.noinv') }),
        h('div', { class: 'choices', role: 'radiogroup', 'aria-label': t('srv.q') },
          choice('role', 'forward', fwd, t('srv.forward'), t('srv.opens'), function (v) { w.role = v; invalidate(); draw(); }),
          choice('role', 'serve', !fwd, t('srv.serve'), '', function (v) { w.role = v; invalidate(); draw(); })),
        h('div', { class: 'form-grid' }, field('node', t('f.node')), field('peerId', t('f.peerid')), field('tunnel', t('f.tunnel')), field('port', t('f.port'), { type: 'number', inputmode: 'numeric' }),
          fwd ? field('peerHost', t('f.peerhost'), { inputmode: 'decimal' }) : null,
          fwd && w.engine === 'tcp-tls' ? field('serverName', t('f.servername')) : null),
        h('details', null, h('summary', { text: t('adv') }), h('div', { class: 'form-grid' }, field('tport', t('f.tport'), { type: 'number', inputmode: 'numeric' }), field('dir', t('f.dir')), field('inst', t('f.inst')))));
    }
    function stepMethod() {
      var ok = engines.filter(function (e) { return e.directions.indexOf(w.direction) >= 0; });
      return h('div', null, h('h2', { text: t('meth.q') }), h('div', { class: 'choices', role: 'radiogroup', 'aria-label': t('meth.q') }, ok.map(function (e) {
        var rec = e.status === 'stable';
        var lab = (((e.evidence || {}).lab_two_host) || {})[w.direction];
        var labPill = lab === 'pass' ? pill('ok', t('meth.lab.pass')) : pill('unknown', t('meth.lab.unknown'));
        return choice('engine', e.id, w.engine === e.id, [BAFT.ltr(e.id), pill(rec ? 'ok' : 'warn', rec ? t('meth.rec') + ' · ' + t('meth.stable') : t('meth.experimental')), labPill], t('meth.' + e.id),
          function (v) { w.engine = v; invalidate(); draw(); });
      })));
    }
    function stepPreflight() {
      var pf = w.pf;
      var list = pf ? h('ul', { class: 'pf' }, pf.checks.map(function (c) {
        var ev = c.result === 'unknown' && BAFT_I18N.en['pfu.' + c.id] ? t('pfu.' + c.id) : c.evidence;
        var fix = c.result !== 'pass' && c.remediation ? (BAFT_I18N.en['pfr.' + c.id] ? t('pfr.' + c.id) : c.remediation) : null;
        return h('li', null, h('strong', { text: t('pfc.' + c.id) === 'pfc.' + c.id ? c.id : t('pfc.' + c.id) }),
          h('span', { class: 'muted', text: ' · ' + t(c.scope === 'peer' ? 'pf.scope.peer' : 'pf.scope.local') + ' ' }),
          pill({ pass: 'ok', warning: 'warn', blocked: 'bad', unknown: 'unknown' }[c.result], t('res.' + c.result)), h('span', { class: 'ev' }, c.result === 'unknown' ? ev : BAFT.ltr(ev)),
          fix ? h('span', { class: 'muted', text: ' ' + t('pf.fix') + ': ' + fix }) : null);
      })) : null;
      return h('div', null, h('h2', { text: t('pf.title') }), h('p', { class: 'sub', text: t('pf.note') }),
        h('button', { class: 'primary', type: 'button', disabled: w.busy ? true : null, onclick: runPreflight, text: w.busy ? t('pf.running') : t('pf.run') }),
        w.pfError ? h('p', { class: 'error', role: 'alert', text: w.pfError }) : null,
        pf ? h('div', null, h('p', null, t('pf.overall') + ': ', pill({ pass: 'ok', warning: 'warn', blocked: 'bad', unknown: 'unknown' }[pf.overall], t('res.' + pf.overall))), list, h('p', { class: 'sub', text: t('pf.unknownnote') })) : h('p', { class: 'sub', text: t('pf.stale') }));
    }
    function effects(p) {
      var pf = w.pf, cfg = p.config || {}, eng = engines.filter(function (e) { return e.id === w.engine; })[0] || {};
      var serve = cfg.mode === 'serve';
      var unknown = pf ? pf.checks.filter(function (c) { return c.result === 'unknown'; }).map(function (c) { return t('pfc.' + c.id) === 'pfc.' + c.id ? c.id : t('pfc.' + c.id); }) : [];
      var lab = ((eng.evidence || {}).lab_two_host || {})[w.direction];
      return h('div', null,
        h('h3', { text: t('pv.topology') }),
        kv([[t('sum.dir'), dirLabel(w.direction)], [t('pv.thisrole'), roleLabel(serve ? 'serve' : 'forward')],
          [t('pv.otherrole'), roleLabel(serve ? 'forward' : 'serve')], [t('sum.engine'), BAFT.ltr(w.engine)],
          [t('pv.listener'), serve ? h('span', null, BAFT.ltr(cfg.listen || '—'), ' (' + t('pv.thisserver') + ')') : t('pv.otherserver')],
          [t('pv.dialer'), serve ? t('pv.otherserver') : h('span', null, BAFT.ltr(cfg.remote || '—'), ' (' + t('pv.thisserver') + ' → ' + t('pv.otherserver') + ')')],
          [t('pv.service'), BAFT.ltr(serve ? (cfg.target || '—') : (cfg.listen || '—'))],
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
    function stepApply() {
      var cmds = w.plan ? w.plan.commands.join('\n') : '';
      var copy = h('button', { type: 'button', text: t('ap.copy'), onclick: function () {
        if (navigator.clipboard) { navigator.clipboard.writeText(cmds).then(function () { BAFT.toast(t('ap.copied')); }, function () {}); }
      } });
      var gated = w.pf && w.pf.overall === 'blocked';
      var go = h('button', { type: 'button', class: 'primary', text: t('ap.go'), disabled: (!w.applyConfirm || w.busy || gated || (w.applyReceipt && w.applyReceipt.result === 'applied')) ? 'disabled' : null, onclick: runApply });
      var chk = h('label', null, h('input', { type: 'checkbox', checked: w.applyConfirm ? 'checked' : null, onchange: function (e) { w.applyConfirm = e.target.checked; draw(); } }), ' ', t('ap.confirm'));
      return h('div', null, h('div', { class: 'banner' }, h('h2', { text: t('ap.title') }), h('p', { text: t('ap.body') })),
        chk, go, w.applyErr ? h('p', { class: 'error', role: 'alert', text: w.applyErr }) : null, w.applyReceipt ? receiptView(w.applyReceipt) : null,
        h('p', { text: t('ap.run') }), h('pre', { text: cmds }), copy, h('p', { class: 'sub', text: t('ap.after') }));
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
    return h('div', { class: 'grid cols-2' },
      card(t('agent.health'), h('div', null, kv([[t('f.version'), BAFT.ltr('v' + s.version)], [t('f.lastseen'), t('checked', { ago: BAFT.ago(BAFT.fetchedAt) })]]),
        h('p', { class: 'sub', text: t('agent.sampler', { s: BAFT.fmtNum(hist.interval_s || 0, 0), n: BAFT.fmtNum(hist.capacity || 0, 0) }) }),
        h('button', { type: 'button', onclick: check, text: t('agent.check') }), out)),
      card(t('agent.caps'), h('div', null,
        h('ul', { class: 'warns' },
          h('li', null, t('cap.audit') + ': ', pill(d.audit === null ? 'unknown' : 'ok', d.audit === null ? t('cap.off') : t('cap.on'))),
          h('li', null, t('cap.desired') + ': ', pill(d.audit === null ? 'unknown' : 'ok', d.audit === null ? t('cap.off') : t('cap.on'))),
          h('li', null, t('cap.plan') + ': ', pill('ok', t('cap.on'))),
          h('li', null, t('cap.apply') + ': ', pill('ok', t('cap.on')))),
        h('p', { class: 'sub', text: t('agent.loopback') }), h('p', { class: 'sub', text: t('agent.secret') }))));
  } });

  /* ---- Activity ---- */
  var activityFilter = 'all';
  BAFT.register({ id: 'activity', icon: 'activity', order: 6, needs: ['audit'], render: function (ctx) {
    var a = ctx.data.audit;
    if (!a) { return card(t('activity.title'), empty(t('activity.disabled'))); }
    var all = (a.entries || []).slice().reverse();
    var rows = all.filter(function (e) { return activityFilter === 'all' || e.result === activityFilter; });
    var sel = h('select', { 'aria-label': t('filter.result'), onchange: function () { activityFilter = sel.value; BAFT.refresh(true); } },
      [['all', t('filter.all')], ['accepted', t('accepted')], ['rejected', t('rejected')]].map(function (o) { return h('option', { value: o[0], text: o[1], selected: o[0] === activityFilter ? true : null }); }));
    var table = rows.length ? h('div', { class: 'table-wrap' }, h('table', { class: 'cards' },
      h('thead', null, h('tr', null, ['col.time', 'col.op', 'col.result', 'col.receipt', 'col.detail'].map(function (k) { return h('th', { scope: 'col', text: t(k) }); }))),
      h('tbody', null, rows.map(function (e) {
        return h('tr', null,
          h('td', { 'data-label': t('col.time'), text: new Date(e.ts * 1000).toLocaleString(BAFT.lang === 'fa' ? 'fa-IR' : 'en-GB') }),
          h('td', { 'data-label': t('col.op'), text: t('op.desired') }),
          h('td', { 'data-label': t('col.result') }, pill(e.result === 'accepted' ? 'ok' : 'bad', t(e.result) === e.result ? e.result : t(e.result))),
          h('td', { 'data-label': t('col.receipt') }, h('code', { text: String(e.sha256 || '').slice(0, 12) })),
          h('td', { 'data-label': t('col.detail') }, e.detail ? BAFT.ltr(e.detail) : '—'));
      })))) : empty(t('activity.empty'));
    return card(t('activity.title'), h('div', null, h('p', { class: 'sub', text: t('activity.note') + ' ' + t('activity.noactor') }),
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
