/* Shared UI components and evidence helpers (BAFT.ui). Pages compose these; they never build
   status colours or "healthy" claims on their own. Overlays use the native <dialog> element
   (focus trap, Esc, inert background); the document scroll is locked while one is open. */
(function () {
  'use strict';
  var BAFT = window.BAFT, h = BAFT.h, t = BAFT.t, G = BAFT.GLYPH;
  var U = (BAFT.ui = {});

  /* ---- status: six semantic states, each = glyph + word + colour, never colour alone ---- */
  U.pill = function (kind, label) {
    return h('span', { class: 'badge ' + kind }, h('span', { class: 'glyph', 'aria-hidden': 'true', text: G[kind] || G.unknown }), label);
  };
  U.card = function (title, body, extra, right) {
    return h('section', { class: 'card ' + (extra || '') }, title ? h('header', null, h('h2', { text: title }), right || null) : null, body);
  };
  U.kpi = function (label, value, hint, kind) {
    return h('section', { class: 'card kpi' }, h('h3', { text: label }), h('div', { class: 'value' }, value),
      kind ? U.pill(kind.k, kind.l) : null, h('div', { class: 'hint', text: hint || '' }));
  };
  /* kind: empty | unavailable | error | stale. Every non-data state says what it is and what to do. */
  U.empty = function (text, cta, kind) {
    return h('div', { class: 'empty ' + (kind || '') }, h('p', { text: text }), cta ? h('a', { class: 'btn primary', href: cta.href, text: cta.label }) : null);
  };
  U.kv = function (rows) {
    var dl = h('dl', { class: 'kv' });
    rows.forEach(function (r) { dl.appendChild(h('dt', { text: r[0] })); dl.appendChild(h('dd', null, r[1] == null ? '—' : r[1])); });
    return dl;
  };
  U.skeleton = function () {
    return h('div', { class: 'skeleton', role: 'status', 'aria-label': t('ui.loading') }, h('span', { class: 'sr', text: t('ui.loading') }),
      h('div', { class: 'sk sk-title' }), h('div', { class: 'grid kpis' }, [1, 2, 3, 4].map(function () { return h('div', { class: 'sk sk-card' }); })), h('div', { class: 'sk sk-block' }));
  };
  U.median = function (a) {
    if (!a.length) { return null; }
    var s = a.slice().sort(function (x, y) { return x - y; }), m = s.length >> 1;
    return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
  };
  U.conf = function (d) { return ((d.status || {}).config || {}).configuration || {}; };
  U.life = function (sec) {
    if (sec == null) { return null; }
    return sec < 0 ? t('expired') : sec < 172800 ? t('hours', { n: BAFT.fmtNum(sec / 3600, 0) }) : t('days', { n: BAFT.fmtNum(sec / 86400, 0) });
  };
  U.listenState = function (s) { return s.listener_accepting ? { k: 'ok', l: t('state.listening') } : { k: 'bad', l: t('state.notlistening') }; };
  U.stats = function (d) {
    var hist = (d.history || {}).samples || [], up = hist.filter(function (x) { return x.up; });
    return { hist: hist, avail: hist.length ? up.length / hist.length * 100 : null, med: U.median(up.map(function (x) { return x.rtt_ms; })) };
  };
  U.fmtBytes = function (n) {
    if (n == null || isNaN(n)) { return null; }
    var u = ['B', 'KiB', 'MiB', 'GiB', 'TiB'], i = 0, v = Number(n);
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return BAFT.fmtNum(v, i ? 1 : 0) + ' ' + u[i];
  };
  U.tierKind = function (r) {
    return r === 'pass' || r === 'observed' || r === 'traffic_observed' || r === 'active' ? 'ok' : r === 'fail' ? 'bad' : r === 'no_recent_traffic' || r === 'not_observed' ? 'warn' : 'unknown';
  };
  U.tierPill = function (r) { return U.pill(U.tierKind(r), t('tier.r.' + r) === 'tier.r.' + r ? String(r || 'unknown') : t('tier.r.' + r)); };
  /* Tunnel state from unit evidence only. "Ready" does not exist: carrying traffic is the strongest claim. */
  U.tunnelState = function (tn) {
    var k = tn ? tn.state : 'unknown';
    return { k: k === 'carrying_traffic' || k === 'session_authorized' ? 'ok' : k === 'running_no_session' ? 'warn' : k === 'down' ? 'bad' : 'unknown', l: t('ts.' + (k || 'unknown')) };
  };
  U.dirLabel = function (x) { return x ? t('dir.' + x) : '—'; };
  U.roleLabel = function (x, dir) { return x ? t((dir === 'reverse' ? 'role.rev.' : 'role.') + x) : '—'; };
  U.opLabel = function (e) {
    var k = 'op.' + (e.resource || 'desired_state'), v = t(k);
    return v === k ? BAFT.ltr(e.resource || e.action || '—') : v;
  };
  /* Warnings come only from real data; each has severity, affected resource, reason and a next step. */
  U.warnings = function (d) {
    var s = d.status || {}, c = U.conf(d), sec = d.security || {}, out = [];
    if (!s.listener_accepting) { out.push({ sev: 'bad', res: c.tunnel_id, text: t('warn.listener', { t: c.tunnel_id || '—' }), next: t('warn.listener.next'), href: '#/tunnels' }); }
    if (sec.tls) {
      var cs = sec.certificate_seconds_remaining, ls = sec.crl_seconds_remaining;
      if (cs != null && cs < 30 * 86400) { out.push({ sev: cs < 7 * 86400 ? 'bad' : 'warn', res: t('servers.this'), text: t('warn.cert'), next: t('warn.cert.next'), href: '#/servers' }); }
      if (ls != null && ls < 7 * 86400) { out.push({ sev: ls < 3 * 86400 ? 'bad' : 'warn', res: 'CRL', text: t(ls <= 0 ? 'warn.crl.expired' : 'warn.crl'), next: t('warn.crl.next'), href: '#/servers' }); }
      if (c.peer_certificate_pin_configured === false) { out.push({ sev: 'warn', res: c.tunnel_id, text: t('warn.pin'), next: t('warn.pin.next'), href: '#/servers' }); }
    }
    ((d.history || {}).alerts || []).forEach(function (a) {
      var ev = a.evidence || {};
      out.push({ sev: 'warn', res: c.tunnel_id, text: t('alert.' + a.id, { p: BAFT.fmtNum(ev.availability_percent, 1), n: BAFT.fmtNum(ev.probes, 0) }), next: t('warn.avail.next'), href: '#/monitoring' });
    });
    if (d.traffic && d.traffic.available && d.traffic.fresh === false) { out.push({ sev: 'warn', res: c.tunnel_id, text: t('warn.stale'), next: t('warn.stale.next'), href: '#/diagnostics' }); }
    if (d.meta && d.meta.audit === 'unavailable') { out.push({ sev: 'info', res: 'Agent', text: t('warn.audit'), next: t('warn.audit.next'), href: '#/agent' }); }
    return out;
  };
  var SEV = { bad: 'sev.error', warn: 'sev.warning', info: 'sev.info' };
  U.warnList = function (ws, onNavigate) {
    if (!ws.length) { return h('p', { class: 'muted', text: t('ov.nowarn') }); }
    return h('ul', { class: 'warns' }, ws.map(function (w) {
      return h('li', { class: 'w-' + w.sev }, h('div', { class: 'top' }, U.pill(w.sev === 'bad' ? 'bad' : w.sev === 'warn' ? 'warn' : 'info', t(SEV[w.sev])), w.res ? h('strong', null, BAFT.ltr(w.res)) : null),
        h('div', { text: w.text }), h('div', { class: 'sub', text: w.next }),
        w.href ? h('a', { class: 'more-link', href: w.href, onclick: onNavigate || null }, t('ui.details'), BAFT.icon('chevron', 'flip')) : null);
    }));
  };

  /* ---- overlays ---- */
  var open = [];
  function lockScroll() { document.documentElement.classList.toggle('lock', open.length > 0); }
  U.overlayOpen = function () { return open.length > 0; };
  U.closeAll = function () { open.slice().forEach(function (d) { if (d.open) { d.close(); } }); open = []; lockScroll(); };
  /* A modal dialog. kind: dialog | drawer | palette | popover. Returns the <dialog>. */
  function overlay(kind, labelText, content, opener, extraClass) {
    var d = h('dialog', { class: 'ov ov-' + kind + (extraClass ? ' ' + extraClass : ''), 'aria-label': labelText }, content);
    document.body.appendChild(d);
    // Backdrop click closes (the dialog itself fills only its box; the backdrop is the dialog element).
    d.addEventListener('click', function (e) { if (e.target === d) { d.close(); } });
    d.addEventListener('close', function () {
      open = open.filter(function (x) { return x !== d; }); lockScroll(); d.remove();
      if (opener && opener.isConnected) { opener.focus(); }
    });
    open.push(d); lockScroll();
    d.showModal();
    return d;
  }
  U.overlay = overlay;
  U.closeBtn = function (d) { return h('button', { class: 'ghost icon-btn', type: 'button', 'aria-label': t('ui.close'), title: t('ui.close'), onclick: function () { d.close(); } }, BAFT.icon('close')); };

  /* Confirmation for state-changing actions: names the action and its consequence; resolves true/false. */
  U.confirm = function (title, body, okLabel, danger) {
    return new Promise(function (resolve) {
      var done = false, d;
      function finish(v) { if (!done) { done = true; resolve(v); } }
      var ok = h('button', { type: 'button', class: danger ? 'danger' : 'primary', onclick: function () { finish(true); d.close(); }, text: okLabel });
      var cancel = h('button', { type: 'button', onclick: function () { d.close(); }, text: t('ui.cancel') });
      d = overlay('dialog', title, h('div', { class: 'ov-body' }, h('h2', { text: title }), h('p', { text: body }), h('div', { class: 'row end' }, cancel, ok)), document.activeElement);
      d.addEventListener('close', function () { finish(false); });
      cancel.focus();
    });
  };

  /* Mobile / tablet navigation drawer: the same grouped tree as the sidebar. */
  U.navDrawer = function (opener) {
    var d;
    var tree = h('nav', { 'aria-label': t('ui.nav.main'), class: 'drawer-nav' }, BAFT.navTree());
    d = overlay('drawer', t('ui.nav.main'), h('div', { class: 'ov-body' }, h('div', { class: 'ov-head' }, h('div', { class: 'brand' }, BAFT.mark(), h('span', { class: 'brand-name', text: t('app') })), U.closeBtn(d || { close: function () { } })), tree), opener);
    // The close button was created before `d` existed; rebind it.
    var cb = d.querySelector('.ov-head .icon-btn'); if (cb) { cb.onclick = function () { d.close(); }; }
    tree.addEventListener('click', function (e) { if (e.target.closest('a')) { d.close(); } });
    return d;
  };

  /* Notifications: the same evidence-derived warnings as the Overview, nothing more. */
  U.notifications = function (opener) {
    var ws = U.warnings(BAFT.data || {}), d;
    var body = h('div', { class: 'ov-body' }, h('div', { class: 'ov-head' }, h('h2', { text: t('ui.notif') }), h('button', { class: 'ghost icon-btn', type: 'button', 'aria-label': t('ui.close'), onclick: function () { d.close(); } }, BAFT.icon('close'))),
      U.warnList(ws, function () { d.close(); }), h('p', { class: 'sub', text: t('ui.notif.src') }));
    d = overlay('popover', t('ui.notif'), body, opener);
    return d;
  };

  /* Command palette (Ctrl/Cmd+K): navigation and local actions only. Nothing here talks to the backend. */
  U.palette = function () {
    if (open.some(function (x) { return x.classList.contains('ov-palette'); })) { return; }
    var items = BAFT.pages.map(function (p) { return { label: t('nav.' + p.id), hint: t('nav.g.' + (p.group || 'system')), icon: p.icon, run: function () { BAFT.go(p.id); } }; });
    items.push({ label: t('ui.cmd.theme'), hint: t('ui.cmd.local'), icon: 'sun', run: function () { BAFT.setTheme(BAFT.theme === 'light' ? 'dark' : 'light'); } });
    items.push({ label: t('ui.cmd.lang'), hint: t('ui.cmd.local'), icon: 'globe', run: function () { BAFT.setLang(BAFT.lang === 'fa' ? 'en' : 'fa'); } });
    items.push({ label: t('ui.nav.collapse'), hint: t('ui.cmd.local'), icon: 'panel', run: function () { BAFT.setNav(BAFT.nav === 'rail' ? 'full' : 'rail'); } });
    items.push({ label: t('logout'), hint: t('ui.cmd.local'), icon: 'logout', run: BAFT.logout });
    var sel = 0, shown = items, d;
    var input = h('input', { type: 'text', role: 'combobox', 'aria-expanded': 'true', 'aria-controls': 'cmd-list', 'aria-autocomplete': 'list', 'aria-label': t('ui.cmd.open'), placeholder: t('ui.cmd.ph'), autocomplete: 'off' });
    var list = h('ul', { class: 'cmd-list', id: 'cmd-list', role: 'listbox', 'aria-label': t('ui.cmd.open') });
    var empty = h('p', { class: 'sub cmd-empty', role: 'status', hidden: true, text: t('ui.cmd.none') });
    function draw() {
      list.textContent = '';
      shown.forEach(function (it, i) {
        var li = h('li', { role: 'option', id: 'cmd-' + i, 'aria-selected': i === sel ? 'true' : 'false', class: i === sel ? 'sel' : '', onclick: function () { run(i); } }, BAFT.icon(it.icon), h('span', { class: 'cl', text: it.label }), h('span', { class: 'ch', text: it.hint }));
        list.appendChild(li);
      });
      empty.hidden = shown.length > 0;
      if (shown.length) { input.setAttribute('aria-activedescendant', 'cmd-' + sel); } else { input.removeAttribute('aria-activedescendant'); }
      var cur = list.children[sel]; if (cur && cur.scrollIntoView) { cur.scrollIntoView({ block: 'nearest' }); }
    }
    function run(i) { var it = shown[i]; if (!it) { return; } d.close(); it.run(); }
    input.addEventListener('input', function () {
      var q = input.value.trim().toLowerCase();
      shown = items.filter(function (it) { return !q || it.label.toLowerCase().indexOf(q) >= 0 || it.hint.toLowerCase().indexOf(q) >= 0; });
      sel = 0; draw();
    });
    input.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(shown.length - 1, sel + 1); draw(); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); sel = Math.max(0, sel - 1); draw(); }
      else if (e.key === 'Enter') { e.preventDefault(); run(sel); }
    });
    d = overlay('palette', t('ui.cmd.open'), h('div', { class: 'ov-body' }, h('div', { class: 'cmd-in' }, BAFT.icon('search'), input), list, empty), document.activeElement);
    draw(); input.focus();
  };

  /* Expandable section with a real button (aria-expanded) instead of a bare <details>. */
  U.section = function (title, body, openByDefault) {
    var d = h('details', { class: 'fold' }, h('summary', { text: title }), body);
    if (openByDefault) { d.setAttribute('open', ''); }
    return d;
  };
})();
