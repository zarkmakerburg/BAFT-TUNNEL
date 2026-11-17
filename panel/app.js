/* Panel core: DOM helpers, i18n, formatting, API client, page registry, application shell and the
   refresh loop. Components live in ui.js, pages in pages.js (BAFT.register({...})).
   Rules: text only (no HTML injection), no inline style attributes (strict CSP), nothing persisted
   outside sessionStorage, and no value is shown that an endpoint did not return. */
(function () {
  'use strict';
  var BAFT = (window.BAFT = { pages: [], lang: 'fa', token: '', theme: 'dark', nav: 'full', data: {}, fetchedAt: 0, holdUntil: 0, busy: 0 });
  var SVGNS = 'http://www.w3.org/2000/svg';

  function store(key, value) {
    try {
      if (value === undefined) { return sessionStorage.getItem(key); }
      if (value === null) { sessionStorage.removeItem(key); } else { sessionStorage.setItem(key, value); }
    } catch (e) { /* storage may be blocked */ }
    return null;
  }
  BAFT.store = store;
  function append(node, kids) {
    kids.forEach(function (c) {
      if (c == null || c === false) { return; }
      if (Array.isArray(c)) { append(node, c); return; }
      node.appendChild(c.nodeType ? c : document.createTextNode(String(c)));
    });
  }
  /* h('div', {class:'x', onclick:fn}, child, ...) - text only, never HTML. */
  BAFT.h = function (tag, attrs) {
    var n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      var v = attrs[k];
      if (v == null || v === false) { return; }
      if (k === 'class') { n.className = v; }
      else if (k === 'text') { n.textContent = v; }
      else if (k.slice(0, 2) === 'on') { n.addEventListener(k.slice(2), v); }
      else { n.setAttribute(k, v === true ? '' : v); }
    });
    append(n, [].slice.call(arguments, 2));
    return n;
  };
  var h = BAFT.h;
  BAFT.t = function (key, vars) {
    var s = (BAFT_I18N[BAFT.lang] || {})[key];
    if (s === undefined) { s = BAFT_I18N.en[key]; }
    if (s === undefined) { return key; }
    return s.replace(/\{(\w+)\}/g, function (_, k) { return vars && vars[k] !== undefined ? vars[k] : ''; });
  };
  var t = BAFT.t;
  function locale() { return BAFT.lang === 'fa' ? 'fa-IR' : 'en-GB'; }
  BAFT.fmtTime = function (sec) { return new Date(sec * 1000).toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' }); };
  BAFT.fmtDate = function (sec) { return new Date(sec * 1000).toLocaleString(locale(), { dateStyle: 'medium', timeStyle: 'medium' }); };
  /* Technical identifiers (IDs, addresses, ports, hashes, versions) keep left-to-right order in both languages. */
  BAFT.ltr = function (text) { return h('bdi', { class: 'ltr', dir: 'ltr', text: text == null ? '—' : String(text) }); };
  /* "5 seconds ago" in the UI language; never claims freshness it does not have. */
  BAFT.ago = function (sec) {
    if (!sec) { return t('never'); }
    var d = Math.max(0, Math.round(Date.now() / 1000 - sec));
    if (d < 3) { return t('ago.now'); }
    var rtf = new Intl.RelativeTimeFormat(BAFT.lang === 'fa' ? 'fa' : 'en', { numeric: 'auto' });
    return d < 60 ? rtf.format(-d, 'second') : d < 3600 ? rtf.format(-Math.round(d / 60), 'minute') : d < 172800 ? rtf.format(-Math.round(d / 3600), 'hour') : rtf.format(-Math.round(d / 86400), 'day');
  };
  BAFT.pct = function (n, digits) { return '\u2066' + BAFT.fmtNum(n, digits) + '%\u2069'; };
  BAFT.fmtNum = function (n, digits) {
    return Number(n).toLocaleString(BAFT.lang === 'fa' ? 'fa-IR' : 'en-US', { maximumFractionDigits: digits === undefined ? 1 : digits });
  };

  /* Icons: one family, 24x24 box, 1.75 stroke, round caps. Decorative: always aria-hidden. */
  var ICONS = {
    overview: ['M3 3h7v9H3z', 'M14 3h7v5h-7z', 'M14 12h7v9h-7z', 'M3 16h7v5H3z'],
    tunnels: ['M3 8h14', 'M13 4l4 4-4 4', 'M21 16H7', 'M11 12l-4 4 4 4'],
    servers: ['M3 4h18v7H3z', 'M3 13h18v7H3z', 'M7 7.5h.01', 'M7 16.5h.01', 'M11 7.5h6', 'M11 16.5h6'],
    monitoring: ['M3 3v18h18', 'M7 15l4-5 3 3 5-7'],
    setup: ['M12 3v4', 'M12 17v4', 'M3 12h4', 'M17 12h4', 'M12 8a4 4 0 100 8 4 4 0 000-8z'],
    diagnostics: ['M3 12h4l2-6 4 12 2-6h6'],
    activity: ['M8 6h13', 'M8 12h13', 'M8 18h13', 'M3.5 6h.01', 'M3.5 12h.01', 'M3.5 18h.01'],
    agent: ['M7 7h10v10H7z', 'M10 10h4v4h-4z', 'M9 3v4', 'M15 3v4', 'M9 17v4', 'M15 17v4', 'M3 9h4', 'M3 15h4', 'M17 9h4', 'M17 15h4'],
    settings: ['M4 6h10', 'M18 6h2', 'M4 12h2', 'M10 12h10', 'M4 18h10', 'M18 18h2', 'M16 4v4', 'M8 10v4', 'M16 16v4'],
    menu: ['M4 6h16', 'M4 12h16', 'M4 18h16'],
    close: ['M6 6l12 12', 'M18 6L6 18'],
    search: ['M11 4a7 7 0 100 14 7 7 0 000-14z', 'M20 20l-3.5-3.5'],
    bell: ['M6 9a6 6 0 0112 0c0 6 2 7 2 8H4c0-1 2-2 2-8z', 'M10 20a2 2 0 004 0'],
    sun: ['M12 8a4 4 0 100 8 4 4 0 000-8z', 'M12 2v2', 'M12 20v2', 'M2 12h2', 'M20 12h2', 'M5 5l1.5 1.5', 'M17.5 17.5L19 19', 'M19 5l-1.5 1.5', 'M6.5 17.5L5 19'],
    moon: ['M20 14.5A8 8 0 019.5 4 8 8 0 1020 14.5z'],
    globe: ['M12 3a9 9 0 100 18 9 9 0 000-18z', 'M3 12h18', 'M12 3c3 3 3 15 0 18', 'M12 3c-3 3-3 15 0 18'],
    panel: ['M3 4h18v16H3z', 'M9 4v16'],
    logout: ['M9 4H4v16h5', 'M14 8l4 4-4 4', 'M18 12H9'],
    refresh: ['M20 11a8 8 0 00-14.5-4.5L4 8', 'M4 4v4h4', 'M4 13a8 8 0 0014.5 4.5L20 16', 'M20 20v-4h-4'],
    chevron: ['M9 6l6 6-6 6'],
    plus: ['M12 5v14', 'M5 12h14'],
    alert: ['M12 4l9 16H3z', 'M12 10v4', 'M12 17h.01'],
    check: ['M5 12l5 5 9-10'],
    more: ['M5 12h.01', 'M12 12h.01', 'M19 12h.01']
  };
  BAFT.icon = function (name, cls) {
    var s = document.createElementNS(SVGNS, 'svg');
    s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('aria-hidden', 'true'); s.setAttribute('class', 'ic' + (cls ? ' ' + cls : ''));
    (ICONS[name] || []).forEach(function (d) {
      var p = document.createElementNS(SVGNS, 'path'); p.setAttribute('d', d); s.appendChild(p);
    });
    return s;
  };
  /* Brand mark: two interlaced strands (baft = weave) crossing between two endpoints. */
  BAFT.mark = function () {
    var s = document.createElementNS(SVGNS, 'svg');
    s.setAttribute('viewBox', '0 0 32 32'); s.setAttribute('aria-hidden', 'true'); s.setAttribute('class', 'brand-mark');
    [['M4 11c8 0 8 10 16 10h8', 'a'], ['M4 21c8 0 8-10 16-10h8', 'b']].forEach(function (d) {
      var p = document.createElementNS(SVGNS, 'path'); p.setAttribute('d', d[0]); p.setAttribute('class', d[1]); s.appendChild(p);
    });
    return s;
  };
  function brand() { return h('div', { class: 'brand' }, BAFT.mark(), h('span', { class: 'brand-name', text: t('app') })); }

  /* Page registry. group: operate | setup | troubleshoot | system. tab: shown in the mobile tab bar. */
  var GROUPS = ['operate', 'setup', 'troubleshoot', 'system'];
  BAFT.register = function (page) {
    BAFT.pages.push(page);
    BAFT.pages.sort(function (a, b) { return (a.order || 99) - (b.order || 99); });
  };

  /* ---- API ---- */
  BAFT.api = function (method, path, body) {
    var opts = { method: method, headers: { Authorization: 'Bearer ' + BAFT.token } };
    if (body !== undefined) { opts.body = JSON.stringify(body); opts.headers['Content-Type'] = 'application/json'; }
    return fetch(path, opts).then(function (r) {
      return r.json().catch(function () { return {}; }).then(function (j) {
        if (r.status === 401 && BAFT.token && !BAFT.signingIn) { BAFT.logout(); }
        return { status: r.status, body: j };
      });
    });
  };
  var ENDPOINTS = { status: '/v1/status', history: '/v1/history', security: '/v1/security', engines: '/v1/engines', desired: '/v1/desired', audit: '/v1/audit', traffic: '/v1/traffic', diagnostics: '/v1/diagnostics', update: '/v1/update', host: '/v1/host/tunnels' };
  /* Loaded on every page so the status badge and the notification count mean the same everywhere. */
  var BASE = ['security', 'history', 'traffic', 'audit', 'update'];
  /* Each endpoint ends as data (200), null + 'unavailable' (404: the agent does not offer it in this
     mode) or null + 'error' (anything else). Status goes first: an agent in setup mode has nothing else. */
  BAFT.load = function (needs) {
    var out = {}, meta = {};
    function get(k) {
      return BAFT.api('GET', ENDPOINTS[k]).then(function (r) {
        out[k] = r.status === 200 ? r.body : null;
        meta[k] = r.status === 200 ? 'ok' : r.status === 404 ? 'unavailable' : 'error';
      }, function () { out[k] = null; meta[k] = 'error'; });
    }
    // An agent in setup mode still serves engines, audit and the setup endpoints (the wizard needs them);
    // endpoints it does not serve answer 404/error and show as unavailable.
    return get('status').then(function () {
      if (!out.status) { return null; }
      return Promise.all(needs.filter(function (k) { return k !== 'status'; }).map(get));
    }).then(function () {
      if (out.status) { BAFT.fetchedAt = Math.floor(Date.now() / 1000); }
      out.meta = meta;
      return out;
    });
  };

  /* ---- theme + language + navigation width ---- */
  BAFT.applyTheme = function () {
    var root = document.documentElement;
    if (BAFT.theme === 'auto') { root.setAttribute('data-theme', 'auto'); } else { root.setAttribute('data-theme', BAFT.theme); }
    store('theme', BAFT.theme);
  };
  BAFT.applyLang = function () {
    var l = BAFT_LANGS[BAFT.lang];
    document.documentElement.lang = BAFT.lang;
    document.documentElement.dir = l.dir;
    store('lang', BAFT.lang);
  };
  BAFT.setNav = function (mode) {
    BAFT.nav = mode === 'rail' ? 'rail' : 'full';
    store('nav', BAFT.nav);
    if (ui.shell) { ui.shell.setAttribute('data-nav', BAFT.nav); }
    if (ui.collapse) { ui.collapse.setAttribute('aria-pressed', BAFT.nav === 'rail' ? 'true' : 'false'); }
  };
  BAFT.setLang = function (k) { BAFT.lang = k; BAFT.applyLang(); BAFT.start(); };
  BAFT.setTheme = function (k) { BAFT.theme = k; BAFT.applyTheme(); BAFT.start(); };
  BAFT.toast = function (text, kind) {
    var host = document.getElementById('toasts');
    if (!host) { host = h('div', { id: 'toasts', class: 'toasts', role: 'status', 'aria-live': 'polite' }); document.body.appendChild(host); }
    var n = h('div', { class: 'toast ' + (kind || ''), text: text });
    host.appendChild(n);
    setTimeout(function () { n.remove(); }, 3500);
  };

  /* ---- sign in ---- */
  function langButton() {
    var other = BAFT.lang === 'fa' ? 'en' : 'fa';
    return h('button', { class: 'ghost lang-btn', type: 'button', lang: other, 'aria-label': t('ui.lang.switch', { l: BAFT_LANGS[other].label }), onclick: function () { BAFT.setLang(other); } },
      BAFT.icon('globe'), h('span', { text: BAFT_LANGS[other].label }));
  }
  function renderLogin() {
    var root = document.getElementById('root');
    root.textContent = '';
    document.title = t('app');
    var input = h('input', { id: 'token', type: 'password', autocomplete: 'off', dir: 'ltr', 'aria-describedby': 'token-hint' });
    var err = h('p', { class: 'error', role: 'alert', hidden: true });
    var btn = h('button', { class: 'primary', id: 'connect', type: 'button', onclick: go, text: t('connect') });
    function go() {
      BAFT.token = input.value.trim();
      err.hidden = true;
      if (!BAFT.token) { err.textContent = t('ui.login.empty'); err.hidden = false; input.focus(); return; }
      BAFT.signingIn = true; btn.disabled = true;
      BAFT.api('GET', '/v1/status').then(function (r) { return r.status; }, function () { return 0; }).then(function (status) {
        BAFT.signingIn = false; btn.disabled = false;
        if (status === 200) { store('token', BAFT.token); BAFT.start(); }
        else { BAFT.token = ''; err.textContent = status === 401 ? t('badtoken') : status === 0 ? t('ui.login.offline') : t('fail'); err.hidden = false; }
      });
    }
    input.addEventListener('keydown', function (e) { if (e.key === 'Enter') { go(); } });
    root.appendChild(h('main', { class: 'login' }, h('div', { class: 'card login-card' },
      brand(),
      h('p', { class: 'sub', text: t('ui.login.lead') }),
      h('label', { class: 'field' }, h('span', { text: t('token') }), input),
      h('p', { class: 'sub', id: 'token-hint', text: t('ui.login.hint') }),
      btn, err,
      h('div', { class: 'login-foot' }, langButton()))));
    input.focus();
  }
  BAFT.logout = function () {
    BAFT.token = ''; store('token', null); BAFT.data = null; BAFT.wiz = null; BAFT.fetchedAt = 0;
    if (BAFT.ui && BAFT.ui.closeAll) { BAFT.ui.closeAll(); }
    renderLogin();
  };

  /* ---- shell: sidebar (full or icon rail), top bar, mobile tab bar + drawer ---- */
  var ui = {};
  function currentId() {
    var id = (location.hash || '').replace(/^#\//, '');
    return BAFT.pages.some(function (p) { return p.id === id; }) ? id : 'overview';
  }
  function navLink(p, cls) {
    return h('a', { href: '#/' + p.id, class: cls, 'data-id': p.id, title: t('nav.' + p.id) }, BAFT.icon(p.icon), h('span', { class: 'nav-l', text: t('nav.' + p.id) }));
  }
  /* The same grouped navigation is used by the sidebar and by the mobile drawer. */
  BAFT.navTree = function () {
    return GROUPS.map(function (g) {
      var items = BAFT.pages.filter(function (p) { return (p.group || 'system') === g; });
      if (!items.length) { return null; }
      return h('div', { class: 'nav-group' }, h('div', { class: 'nav-h', text: t('nav.g.' + g) }),
        h('ul', { class: 'nav' }, items.map(function (p) { return h('li', null, navLink(p)); })));
    });
  };
  function iconButton(icon, label, onclick, extra) {
    return h('button', Object.assign({ class: 'ghost icon-btn', type: 'button', 'aria-label': label, title: label, onclick: onclick }, extra || {}), BAFT.icon(icon));
  }
  function buildShell() {
    var root = document.getElementById('root');
    root.textContent = '';
    var U = BAFT.ui;
    ui.title = h('h1');
    ui.crumb = h('span', { class: 'crumb' });
    ui.badge = h('span', { class: 'badge unknown' });
    ui.fresh = h('span', { class: 'fresh' });
    ui.content = h('div', { class: 'content', id: 'main', tabindex: '-1' });
    ui.count = h('span', { class: 'count', hidden: true });
    ui.foot = h('div', { class: 'side-node' });
    ui.collapse = iconButton('panel', t('ui.nav.collapse'), function () { BAFT.setNav(BAFT.nav === 'rail' ? 'full' : 'rail'); }, { class: 'ghost icon-btn collapse', 'aria-pressed': BAFT.nav === 'rail' ? 'true' : 'false' });
    ui.bell = iconButton('bell', t('ui.notif'), function () { U.notifications(ui.bell); }, { class: 'ghost icon-btn bell', 'aria-haspopup': 'dialog' });
    ui.bell.appendChild(ui.count);
    var themeBtn = iconButton(BAFT.theme === 'light' ? 'moon' : 'sun', t('settings.theme'), function () { BAFT.setTheme(BAFT.theme === 'light' ? 'dark' : 'light'); });
    var searchBtn = h('button', { class: 'ghost search-btn', type: 'button', 'aria-label': t('ui.cmd.open'), 'aria-haspopup': 'dialog', onclick: function () { U.palette(); } },
      BAFT.icon('search'), h('span', { class: 'search-l', text: t('ui.cmd.open') }), h('kbd', { text: 'Ctrl K' }));
    var menuBtn = iconButton('menu', t('ui.nav.open'), function () { U.navDrawer(menuBtn); }, { class: 'ghost icon-btn menu-btn', 'aria-haspopup': 'dialog' });
    root.appendChild(h('a', { class: 'skip', href: '#main', text: t('skip'), onclick: function (e) { e.preventDefault(); ui.content.focus(); } }));
    ui.sidebar = h('aside', { class: 'sidebar' }, brand(),
      h('nav', { class: 'side-nav', 'aria-label': t('ui.nav.main') }, BAFT.navTree()),
      h('div', { class: 'side-foot' }, ui.foot, ui.collapse));
    var tabs = BAFT.pages.filter(function (p) { return p.tab; });
    ui.tabbar = h('nav', { class: 'tabbar', 'aria-label': t('ui.nav.tabs') },
      tabs.map(function (p) { return navLink(p, 'tab'); }),
      h('button', { class: 'tab tab-more', type: 'button', 'aria-haspopup': 'dialog', onclick: function (e) { U.navDrawer(e.currentTarget); } }, BAFT.icon('more'), h('span', { class: 'nav-l', text: t('nav.more') })));
    ui.shell = h('div', { class: 'shell', 'data-nav': BAFT.nav }, ui.sidebar,
      h('div', { class: 'main' },
        h('header', { class: 'topbar' }, menuBtn, h('div', { class: 'heading' }, ui.crumb, ui.title), ui.fresh, ui.badge, searchBtn, ui.bell, themeBtn, langButton()),
        h('main', null, ui.content)),
      ui.tabbar);
    root.appendChild(ui.shell);
  }
  function markActive(id) {
    [].forEach.call(document.querySelectorAll('[data-id]'), function (a) {
      if (a.getAttribute('data-id') === id) { a.setAttribute('aria-current', 'page'); } else { a.removeAttribute('aria-current'); }
    });
    var page = BAFT.pages.filter(function (p) { return p.id === id; })[0];
    var more = document.querySelector('.tab-more');
    if (more) { more.classList.toggle('on', !!page && !page.tab); }
  }
  BAFT.unreachable = function () {
    return h('div', { class: 'banner bad', role: 'alert' }, h('h2', { text: t('unreachable.title') }), h('p', { text: t('unreachable.body') }), h('p', { class: 'sub', text: t('unreachable.next') }),
      h('button', { type: 'button', onclick: function () { render(true); } }, BAFT.icon('refresh'), t('ui.retry')));
  };
  BAFT.setupPending = function (s) {
    return h('div', { class: 'banner info', role: 'status' }, h('h2', { text: t('setup.pending.title') }),
      h('p', { text: t('setup.pending.body', { inst: s.instance || '' }) }), h('a', { class: 'btn primary', href: '#/setup', text: t('nav.setup') }));
  };
  BAFT.GLYPH = { ok: '✓', warn: '!', bad: '✕', unknown: '?', info: 'i', off: '○', conn: '◌' };
  /* Data older than three refresh intervals is marked stale instead of looking current. */
  function tickFresh() {
    if (!ui.fresh) { return; }
    var age = BAFT.fetchedAt ? Date.now() / 1000 - BAFT.fetchedAt : 0;
    ui.fresh.textContent = BAFT.fetchedAt ? t('updated', { ago: BAFT.ago(BAFT.fetchedAt) }) : '';
    ui.fresh.classList.toggle('stale', age > 15);
    if (ui.shell) { ui.shell.classList.toggle('is-stale', age > 15); }
  }
  function setBadge(s) {
    var U = BAFT.ui, st = !s ? { k: 'unknown', l: t('state.unknown') } : s.setup_pending ? { k: 'warn', l: t('state.setup') } : s.tunnel ? U.tunnelState(s.tunnel) : U.listenState(s);
    ui.badge.className = 'badge ' + st.k;
    ui.badge.replaceChildren(h('span', { class: 'glyph', 'aria-hidden': 'true', text: BAFT.GLYPH[st.k] }), h('span', { class: 'btxt', text: st.l }));
  }
  function partialNotice(page, meta) {
    var bad = Object.keys(meta).filter(function (k) { return meta[k] === 'error' && k !== 'status'; });
    if (!bad.length) { return null; }
    return h('div', { class: 'banner partial', role: 'status' }, h('p', null, h('strong', { text: t('ui.partial.title') + ' ' }), t('ui.partial.body', { l: bad.map(function (k) { return t('ui.src.' + k); }).join(BAFT.lang === 'fa' ? '، ' : ', ') })),
      h('button', { type: 'button', onclick: function () { render(true); } }, BAFT.icon('refresh'), t('ui.retry')));
  }
  var timer = null, renderSeq = 0;
  /* force=true: navigation or an explicit refresh. force=false: the 5 s loop, which never interrupts
     an open overlay, a running action, a chart being read, or a page that manages its own state. */
  function render(force) {
    var id = currentId(), U = BAFT.ui, seq;
    var page = BAFT.pages.filter(function (p) { return p.id === id; })[0];
    var sameView = ui.content.dataset.page === id;
    var ae = document.activeElement;
    if (!force && (U.overlayOpen() || BAFT.busy > 0 || Date.now() < BAFT.holdUntil || (ae && ui.content.contains(ae) && /^(BUTTON|SELECT|INPUT|TEXTAREA|SUMMARY|A)$/.test(ae.tagName)))) { return; }
    seq = ++renderSeq;
    markActive(id);
    ui.title.textContent = t('nav.' + id);
    ui.crumb.textContent = t('nav.g.' + (page.group || 'system'));
    document.title = ui.title.textContent + ' - BAFT-TUNNEL';
    if (!force && page.refresh === false && sameView) { BAFT.load([]).then(function (d) { if (seq === renderSeq && BAFT.token) { setBadge(d.status); tickFresh(); } }); return; }
    if (!sameView) { ui.content.dataset.page = id; ui.content.setAttribute('aria-busy', 'true'); ui.content.replaceChildren(U.skeleton()); }
    var needs = BASE.concat(page.needs || []).filter(function (k, i, a) { return a.indexOf(k) === i; });
    BAFT.load(needs).then(function (data) {
      if (seq !== renderSeq || !BAFT.token) { return; }
      BAFT.data = data;
      var s = data.status, setup = !!(s && s.setup_pending);
      setBadge(s);
      tickFresh();
      var ws = s && !setup ? U.warnings(data) : [], n = ws.filter(function (w) { return w.sev !== 'info'; }).length;
      ui.count.hidden = !n; ui.count.textContent = BAFT.fmtNum(n, 0);
      ui.bell.setAttribute('aria-label', n ? t('ui.notif.n', { n: BAFT.fmtNum(n, 0) }) : t('ui.notif'));
      var c = U.conf(data);
      ui.foot.replaceChildren(h('span', { class: 'side-id' }, c.local_identity ? BAFT.ltr(c.local_identity) : (s && s.instance ? BAFT.ltr(s.instance) : t('unknown'))),
        h('span', { class: 'side-ver' }, s ? BAFT.ltr('v' + s.version) : '—'));
      // Setup mode (no configuration yet): only the wizard and settings make sense.
      var node;
      try {
        node = setup && id !== 'setup' && id !== 'settings' ? BAFT.setupPending(s)
          : s || id === 'settings' ? page.render({ data: data, h: h, t: t }) : BAFT.unreachable();
      } catch (err) { node = h('div', { class: 'banner bad', role: 'alert' }, h('p', { text: t('ui.err.render') }), h('button', { type: 'button', onclick: function () { render(true); } }, t('ui.retry'))); }
      var keep = document.activeElement && ui.content.contains(document.activeElement) ? document.activeElement.id : '';
      var wrap = h('div', { class: sameView ? 'view' : 'view enter' }, s && !setup ? partialNotice(page, data.meta) : null, node);
      ui.content.removeAttribute('aria-busy');
      ui.content.replaceChildren(wrap);
      if (keep) { var again = document.getElementById(keep); if (again) { again.focus(); } }
    });
  }
  BAFT.refresh = render;
  BAFT.go = function (id) { if (currentId() === id) { render(true); } else { location.hash = '#/' + id; } };
  BAFT.start = function () {
    if (BAFT.ui && BAFT.ui.closeAll) { BAFT.ui.closeAll(); }
    if (!BAFT.token) { renderLogin(); return; }
    buildShell();
    if (timer) { clearInterval(timer); }
    timer = setInterval(function () { if (document.visibilityState === 'visible' && BAFT.token) { render(false); } }, 5000);
    if (BAFT.freshTimer) { clearInterval(BAFT.freshTimer); }
    BAFT.freshTimer = setInterval(tickFresh, 1000);
    render(true);
  };
  window.addEventListener('hashchange', function () {
    if (!BAFT.token) { return; }
    BAFT.ui.closeAll();
    render(true);
    window.scrollTo(0, 0);
    ui.content.focus({ preventScroll: true });
  });
  document.addEventListener('keydown', function (e) {
    if (!BAFT.token || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) { return; }
    var k = e.code === 'KeyK' ? 'k' : e.code === 'KeyB' ? 'b' : '';
    if (k === 'k') { e.preventDefault(); BAFT.ui.palette(); }
    else if (k === 'b') { e.preventDefault(); BAFT.setNav(BAFT.nav === 'rail' ? 'full' : 'rail'); }
  });

  BAFT.init = function () {
    var l = store('lang');
    BAFT.lang = BAFT_LANGS[l] ? l : 'fa';
    BAFT.theme = store('theme') || 'dark';
    BAFT.nav = store('nav') === 'rail' ? 'rail' : 'full';
    BAFT.token = store('token') || '';
    BAFT.applyLang(); BAFT.applyTheme();
    BAFT.start();
  };
})();
