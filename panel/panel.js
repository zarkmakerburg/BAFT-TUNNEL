(function () {
  'use strict';
  var I18N = {
    fa: {
      token: 'توکن', connect: 'اتصال', status: 'وضعیت', desired: 'تنظیمات مطلوب ذخیره‌شده',
      wizard: 'ویزارد راه‌اندازی', engine: 'موتور', direction: 'جهت', direct: 'مستقیم', reverse: 'معکوس',
      role: 'این میزبان', serve: 'سرویس را میزبانی می‌کند', forward: 'کاربران اینجا وصل می‌شوند',
      port: 'پورت سرویس', peer_host: 'آدرس IP طرف مقابل', server_name: 'نام گواهی طرف مقابل',
      advanced: 'پیشرفته (پیش‌فرض‌ها کافی است)', plan: 'نمایش برنامه', commands: 'این فرمان‌ها را روی همین میزبان اجرا کنید',
      files: 'فایل‌ها', pending: 'هنوز لازم است', up: 'تونل در حال پذیرش', down: 'شنونده فعال نیست',
      none: 'چیزی ذخیره نشده', stored: 'ذخیره شد (اعمال نشده)', badtoken: 'توکن نادرست است', fail: 'خطا',
      mode: 'حالت', identity: 'هویت', peer: 'همتا', tunnel: 'تونل', version: 'نسخه', listener: 'شنونده'
    },
    en: {
      token: 'Token', connect: 'Connect', status: 'Status', desired: 'Stored desired state',
      wizard: 'Setup wizard', engine: 'Engine', direction: 'Direction', direct: 'Direct', reverse: 'Reverse',
      role: 'This host', serve: 'Hosts the service', forward: 'Users connect here',
      port: 'Service port', peer_host: 'Peer IP address', server_name: 'Peer certificate name',
      advanced: 'Advanced (defaults are fine)', plan: 'Show plan', commands: 'Run these commands on this host',
      files: 'Files', pending: 'Still needed', up: 'Tunnel listener accepting', down: 'Listener not accepting',
      none: 'Nothing stored', stored: 'Stored (not applied)', badtoken: 'Wrong token', fail: 'Error',
      mode: 'Mode', identity: 'Identity', peer: 'Peer', tunnel: 'Tunnel', version: 'Version', listener: 'Listener'
    }
  };
  var lang = 'fa', token = '';
  try { lang = sessionStorage.getItem('lang') || 'fa'; token = sessionStorage.getItem('token') || ''; } catch (e) {}
  function $(id) { return document.getElementById(id); }
  function t(k) { return I18N[lang][k] || k; }
  function applyLang() {
    document.documentElement.lang = lang;
    document.documentElement.dir = lang === 'fa' ? 'rtl' : 'ltr';
    $('lang').textContent = lang === 'fa' ? 'English' : 'فارسی';
    document.querySelectorAll('[data-i18n]').forEach(function (n) { n.textContent = t(n.getAttribute('data-i18n')); });
    try { sessionStorage.setItem('lang', lang); } catch (e) {}
  }
  function api(method, path, body) {
    var opts = { method: method, headers: { Authorization: 'Bearer ' + token } };
    if (body !== undefined) { opts.body = JSON.stringify(body); opts.headers['Content-Type'] = 'application/json'; }
    return fetch(path, opts).then(function (r) {
      return r.json().then(function (j) { return { status: r.status, body: j }; });
    });
  }
  function row(dl, k, v) {
    var dt = document.createElement('dt'), dd = document.createElement('dd');
    dt.textContent = t(k); dd.textContent = String(v);
    dl.appendChild(dt); dl.appendChild(dd);
  }
  function showStatus(j) {
    var c = (j.config || {}).configuration || {};
    var badge = $('badge');
    badge.textContent = j.listener_accepting ? t('up') : t('down');
    badge.className = 'badge ' + (j.listener_accepting ? 'ok' : 'bad');
    var dl = $('status-list'); dl.textContent = '';
    row(dl, 'version', j.version);
    row(dl, 'engine', j.engine);
    row(dl, 'mode', c.mode);
    row(dl, 'direction', c.direction);
    row(dl, 'identity', c.local_identity);
    row(dl, 'peer', c.peer_identity);
    row(dl, 'tunnel', c.tunnel_id);
  }
  function refresh() {
    return api('GET', '/v1/status').then(function (r) {
      if (r.status === 401) { throw new Error(t('badtoken')); }
      if (r.status !== 200) { throw new Error(t('fail')); }
      $('login').hidden = true; $('app').hidden = false;
      showStatus(r.body);
      try { sessionStorage.setItem('token', token); } catch (e) {}
      return api('GET', '/v1/desired');
    }).then(function (r) {
      $('desired-text').textContent = r.status === 200 ? t('stored') + ' (' + r.body.sha256.slice(0, 12) + ')' : t('none');
    });
  }
  function syncWizard() {
    var f = $('wizard');
    var forward = f.role.value === 'forward';
    $('peer-host-row').hidden = !forward;
    $('server-name-row').hidden = !(forward && f.engine.value === 'tcp-tls');
  }
  $('lang').addEventListener('click', function () { lang = lang === 'fa' ? 'en' : 'fa'; applyLang(); if (!$('app').hidden) { refresh().catch(function () {}); } });
  $('connect').addEventListener('click', function () {
    token = $('token').value.trim();
    $('login-error').hidden = true;
    refresh().catch(function (e) { $('login-error').textContent = e.message; $('login-error').hidden = false; });
  });
  ['engine', 'role'].forEach(function (n) { $('wizard')[n].addEventListener('change', syncWizard); });
  $('wizard').addEventListener('submit', function (ev) {
    ev.preventDefault();
    var f = $('wizard'), b = {
      engine: f.engine.value, direction: f.direction.value, role: f.role.value,
      node_id: f.node_id.value, peer_id: f.peer_id.value, tunnel_id: f.tunnel_id.value,
      service_port: Number(f.service_port.value), tunnel_port: Number(f.tunnel_port.value), dir: f.dir.value
    };
    if (f.role.value === 'forward') {
      b.peer_host = f.peer_host.value.trim();
      if (f.engine.value === 'tcp-tls') { b.server_name = f.server_name.value.trim(); }
    }
    $('wizard-error').hidden = true; $('plan').hidden = true;
    api('POST', '/v1/setup/plan', b).then(function (r) {
      if (r.status !== 200) { throw new Error(r.body.detail || r.body.error || t('fail')); }
      $('plan-commands').textContent = r.body.commands.join('\n');
      $('plan-files').textContent = r.body.files.map(function (x) { return x.path + ' [' + x.mode + '] ' + x.note; }).join('\n');
      $('plan-pending').textContent = r.body.pending.join('\n');
      $('plan').hidden = false;
    }).catch(function (e) { $('wizard-error').textContent = e.message; $('wizard-error').hidden = false; });
  });
  applyLang(); syncWizard();
  if (token) { refresh().catch(function () { token = ''; }); }
})();
