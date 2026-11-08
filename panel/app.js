/* Panel core: DOM helpers, i18n, theme, API client, page registry, shell (sidebar on desktop,
   bottom tab bar on mobile) and refresh loop. Add a page with BAFT.register({...}) in pages.js. */
(function () {
  'use strict';
  var BAFT = (window.BAFT = { pages: [], lang: 'fa', token: '', theme: 'dark', data: {}, fetchedAt: 0 });
  var SVGNS = 'http://www.w3.org/2000/svg';

  function store(key, value) {
    try {
      if (value === undefined) { return sessionStorage.getItem(key); }
      if (value === null) { sessionStorage.removeItem(key); } else { sessionStorage.setItem(key, value); }
    } catch (e) { /* storage may be blocked */ }
    return null;
  }
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
  BAFT.fmtTime = function (sec) {
    return new Date(sec * 1000).toLocaleTimeString(BAFT.lang === 'fa' ? 'fa-IR' : 'en-GB', { hour: '2-digit', minute: '2-digit' });
  };
  BAFT.ltr = function (text) { return h('bdi', { class: 'ltr', dir: 'ltr', text: text == null ? '—' : String(text) }); };
  /* "5 seconds ago" in the UI language; never claims freshness it does not have. */
  BAFT.ago = function (sec) {
    if (!sec) { return t('never'); }
    var d = Math.max(0, Math.round(Date.now() / 1000 - sec));
    if (d < 3) { return t('ago.now'); }
    var rtf = new Intl.RelativeTimeFormat(BAFT.lang === 'fa' ? 'fa' : 'en', { numeric: 'auto' });
    return d < 60 ? rtf.format(-d, 'second') : d < 3600 ? rtf.format(-Math.round(d / 60), 'minute') : rtf.format(-Math.round(d / 3600), 'hour');
  };
  BAFT.fmtNum = function (n, digits) {
    return Number(n).toLocaleString(BAFT.lang === 'fa' ? 'fa-IR' : 'en-US', { maximumFractionDigits: digits === undefined ? 1 : digits });
  };

  /* icons: stroke paths in a 24x24 box */
  var ICONS = {
    overview: ['M4 4h6v6H4z', 'M14 4h6v6h-6z', 'M4 14h6v6H4z', 'M14 14h6v6h-6z'],
    servers: ['M4 5h16v6H4z', 'M4 13h16v6H4z', 'M8 8h.01', 'M8 16h.01'],
    agent: ['M12 3v3', 'M5 8h14v10H5z', 'M9 13h.01', 'M15 13h.01', 'M9 21h6'],
    tunnels: ['M4 8h13', 'M14 5l3 3-3 3', 'M20 16H7', 'M10 13l-3 3 3 3'],
    engines: ['M6 6h12v12H6z', 'M9 2v4', 'M15 2v4', 'M9 18v4', 'M15 18v4', 'M2 9h4', 'M2 15h4', 'M18 9h4', 'M18 15h4'],
    security: ['M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z', 'M9 12l2 2 4-4'],
    activity: ['M4 6h16', 'M4 12h16', 'M4 18h10'],
    setup: ['M4 21v-7', 'M4 10V3', 'M12 21v-9', 'M12 8V3', 'M20 21v-5', 'M20 12V3', 'M1 14h6', 'M9 8h6', 'M17 16h6'],
    settings: ['M12 15a3 3 0 100-6 3 3 0 000 6z', 'M12 2v3', 'M12 19v3', 'M2 12h3', 'M19 12h3', 'M5 5l2 2', 'M17 17l2 2', 'M19 5l-2 2', 'M7 17l-2 2'],
    theme: ['M12 3a9 9 0 000 18a9 9 0 000-18z', 'M12 3v18'],
    more: ['M5 12h.01', 'M12 12h.01', 'M19 12h.01']
  };
  BAFT.icon = function (name) {
    var s = document.createElementNS(SVGNS, 'svg');
    s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('aria-hidden', 'true');
    (ICONS[name] || []).forEach(function (d) {
      var p = document.createElementNS(SVGNS, 'path'); p.setAttribute('d', d); s.appendChild(p);
    });
    return s;
  };

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
        if (r.status === 401 && BAFT.token) { BAFT.logout(); }
        return { status: r.status, body: j };
      });
    });
  };
  var ENDPOINTS = { status: '/v1/status', history: '/v1/history', security: '/v1/security', engines: '/v1/engines', desired: '/v1/desired', audit: '/v1/audit' };
  BAFT.load = function (needs) {
    var out = {};
    return Promise.all((needs || []).map(function (k) {
      return BAFT.api('GET', ENDPOINTS[k]).then(function (r) { out[k] = r.status === 200 ? r.body : null; }, function () { out[k] = null; });
    })).then(function () { if (out.status) { BAFT.fetchedAt = Math.floor(Date.now() / 1000); } return out; });
  };

  /* ---- theme + language ---- */
  BAFT.applyTheme = function () {
    var root = document.documentElement;
    if (BAFT.theme === 'auto') { root.removeAttribute('data-theme'); } else { root.setAttribute('data-theme', BAFT.theme); }
    store('theme', BAFT.theme);
  };
  BAFT.applyLang = function () {
    var l = BAFT_LANGS[BAFT.lang];
    document.documentElement.lang = BAFT.lang;
    document.documentElement.dir = l.dir;
    store('lang', BAFT.lang);
  };
  BAFT.toast = function (text) {
    var n = h('div', { class: 'toast', role: 'status', text: text });
    document.body.appendChild(n);
    setTimeout(function () { n.remove(); }, 3500);
  };

  /* ---- shell ---- */
  var ui = {};
  function currentId() {
    var id = (location.hash || '').replace(/^#\//, '');
    return BAFT.pages.some(function (p) { return p.id === id; }) || id === 'more' ? id : 'overview';
  }
  function navLink(p, cls) {
    return h('a', { href: '#/' + p.id, class: cls, 'data-id': p.id }, BAFT.icon(p.icon), h('span', { text: t('nav.' + p.id) }));
  }
  function renderLogin() {
    var root = document.getElementById('root');
    root.textContent = '';
    var input = h('input', { id: 'token', type: 'password', autocomplete: 'off', 'aria-label': t('token') });
    var err = h('p', { class: 'error', role: 'alert', hidden: true });
    function go() {
      BAFT.token = input.value.trim();
      err.hidden = true;
      BAFT.api('GET', '/v1/status').then(function (r) {
        if (r.status === 200) { store('token', BAFT.token); BAFT.start(); }
        else { BAFT.token = ''; err.textContent = r.status === 401 ? t('badtoken') : t('fail'); err.hidden = false; }
      });
    }
    input.addEventListener('keydown', function (e) { if (e.key === 'Enter') { go(); } });
    root.appendChild(h('main', { class: 'login' }, h('div', { class: 'card' },
      h('div', { class: 'brand' }, h('span', { class: 'brand-mark', text: 'B' }), h('span', { text: t('app') })),
      h('label', { class: 'field' }, h('span', { text: t('token') }), input),
      h('button', { class: 'primary', id: 'connect', onclick: go, text: t('connect') }), err,
      h('p', { class: 'muted' }, langButton()))));
  }
  function langButton() {
    var other = BAFT.lang === 'fa' ? 'en' : 'fa';
    return h('button', { class: 'ghost', type: 'button', 'aria-label': 'language', onclick: function () { BAFT.lang = other; BAFT.applyLang(); BAFT.start(); }, text: BAFT_LANGS[other].label });
  }
  BAFT.logout = function () { BAFT.token = ''; store('token', null); renderLogin(); };

  function buildShell() {
    var root = document.getElementById('root');
    root.textContent = '';
    var primary = BAFT.pages.filter(function (p) { return p.primary; });
    var secondary = BAFT.pages.filter(function (p) { return !p.primary; });
    ui.title = h('h1');
    ui.badge = h('span', { class: 'badge' });
    ui.fresh = h('span', { class: 'fresh', 'aria-live': 'off' });
    ui.content = h('div', { class: 'content', id: 'main', tabindex: '-1', 'aria-live': 'polite' });
    var themeBtn = h('button', { class: 'ghost', type: 'button', 'aria-label': t('settings.theme'), onclick: function () {
      BAFT.theme = BAFT.theme === 'dark' ? 'light' : 'dark'; BAFT.applyTheme(); } }, BAFT.icon('theme'));
    root.appendChild(h('a', { class: 'skip', href: '#main', text: t('skip'), onclick: function (e) { e.preventDefault(); ui.content.focus(); } }));
    ui.sidebar = h('aside', { class: 'sidebar' },
      h('div', { class: 'brand' }, h('span', { class: 'brand-mark', text: 'B' }), h('span', { text: t('app') })),
      h('nav', { 'aria-label': 'main' }, h('ul', { class: 'nav' }, BAFT.pages.map(function (p) { return h('li', null, navLink(p)); }))),
      h('div', { class: 'side-foot', id: 'side-foot' }));
    ui.tabbar = h('nav', { class: 'tabbar', 'aria-label': 'tabs' },
      primary.map(function (p) { return navLink(p, 'tab'); }),
      secondary.length ? h('a', { href: '#/more', class: 'tab', 'data-id': 'more' }, BAFT.icon('more'), h('span', { text: t('nav.more') })) : null);
    root.appendChild(h('div', { class: 'shell' }, ui.sidebar,
      h('div', { class: 'main' },
        h('header', { class: 'topbar' }, ui.title, ui.fresh, ui.badge, themeBtn, langButton()),
        h('main', null, ui.content)),
      ui.tabbar));
  }
  function markActive(id) {
    [].forEach.call(document.querySelectorAll('[data-id]'), function (a) {
      if (a.getAttribute('data-id') === id) { a.setAttribute('aria-current', 'page'); } else { a.removeAttribute('aria-current'); }
    });
    // On mobile a secondary page highlights "More".
    var page = BAFT.pages.filter(function (p) { return p.id === id; })[0];
    if (page && !page.primary) {
      var more = document.querySelector('.tabbar [data-id=more]');
      if (more) { more.setAttribute('aria-current', 'page'); }
    }
  }
  function moreNode() {
    var secondary = BAFT.pages.filter(function (p) { return !p.primary; });
    return h('div', { class: 'card' }, h('header', null, h('h2', { text: t('more.title') })),
      h('ul', { class: 'nav' }, secondary.map(function (p) { return h('li', null, navLink(p)); })));
  }
  BAFT.unreachable = function () {
    return h('div', { class: 'banner bad', role: 'alert' }, h('h2', { text: t('unreachable.title') }), h('p', { text: t('unreachable.body') }), h('p', { class: 'sub', text: t('unreachable.next') }));
  };
  var GLYPH = BAFT.GLYPH = { ok: '✓', warn: '!', bad: '✕', unknown: '?', info: 'i' };
  function tickFresh() { if (ui.fresh) { ui.fresh.textContent = BAFT.fetchedAt ? t('updated', { ago: BAFT.ago(BAFT.fetchedAt) }) : ''; } }
  var timer = null, renderSeq = 0;
  function render(force) {
    var id = currentId(), seq = ++renderSeq;
    var page = BAFT.pages.filter(function (p) { return p.id === id; })[0];
    markActive(id);
    ui.title.textContent = id === 'more' ? t('nav.more') : t('nav.' + id);
    document.title = ui.title.textContent + ' - BAFT-TUNNEL';
    if (id === 'more') { ui.content.textContent = ''; ui.content.appendChild(moreNode()); return; }
    if (!force && page.refresh === false && ui.content.dataset.page === id) { return; }
    var needs = ['status'].concat(page.needs || []).filter(function (k, i, a) { return a.indexOf(k) === i; });
    BAFT.load(needs).then(function (data) {
      if (seq !== renderSeq || !BAFT.token) { return; }
      BAFT.data = data;
      var s = data.status;
      var kind = s ? (s.listener_accepting ? 'ok' : 'bad') : 'unknown';
      ui.badge.className = 'badge ' + kind;
      ui.badge.textContent = '';
      ui.badge.appendChild(h('span', { class: 'glyph', 'aria-hidden': 'true', text: GLYPH[kind] }));
      ui.badge.appendChild(document.createTextNode(s ? (s.listener_accepting ? t('state.listening') : t('state.notlistening')) : t('state.unknown')));
      tickFresh();
      var foot = document.getElementById('side-foot');
      if (foot && s) { foot.textContent = 'v' + s.version; }
      var node = s || id === 'settings' ? page.render({ data: data, h: h, t: t }) : BAFT.unreachable();
      ui.content.dataset.page = id;
      ui.content.textContent = '';
      ui.content.appendChild(node);
    });
  }
  BAFT.refresh = render;
  BAFT.start = function () {
    if (!BAFT.token) { renderLogin(); return; }
    buildShell();
    if (timer) { clearInterval(timer); }
    timer = setInterval(function () { if (document.visibilityState === 'visible' && BAFT.token) { render(false); } }, 5000);
    if (BAFT.freshTimer) { clearInterval(BAFT.freshTimer); }
    BAFT.freshTimer = setInterval(tickFresh, 1000);
    render(true);
  };
  window.addEventListener('hashchange', function () { if (BAFT.token) { render(true); } });

  BAFT.init = function () {
    var l = store('lang');
    BAFT.lang = BAFT_LANGS[l] ? l : 'fa';
    BAFT.theme = store('theme') || 'dark';
    BAFT.token = store('token') || '';
    BAFT.applyLang(); BAFT.applyTheme();
    BAFT.start();
  };
})();
