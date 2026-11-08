/* Dependency-free SVG chart primitives. Every chart is role="img" with a text alternative. */
(function () {
  'use strict';
  var NS = 'http://www.w3.org/2000/svg';
  function el(tag, attrs, text) {
    var n = document.createElementNS(NS, tag);
    Object.keys(attrs || {}).forEach(function (k) { n.setAttribute(k, attrs[k]); });
    if (text !== undefined) { n.textContent = text; }
    return n;
  }
  function root(w, h, label) {
    return el('svg', { viewBox: '0 0 ' + w + ' ' + h, class: 'chart', role: 'img', 'aria-label': label, preserveAspectRatio: 'xMidYMid meet' });
  }
  function niceMax(v) {
    if (v <= 0) { return 1; }
    var p = Math.pow(10, Math.floor(Math.log10(v))), f = v / p;
    return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * p;
  }
  /* points: [{x:number, y:number|null}], opts: {label, fmtX(x), fmtY(y), height} */
  function line(points, opts) {
    opts = opts || {};
    var W = 600, H = opts.height || 200, L = 44, R = 8, T = 8, B = 22;
    var svg = root(W, H, opts.label || 'chart');
    var ys = points.filter(function (p) { return p.y != null; }).map(function (p) { return p.y; });
    if (!points.length) { return svg; }
    var max = niceMax(Math.max.apply(null, ys.length ? ys : [1]));
    var x0 = points[0].x, x1 = points[points.length - 1].x, span = Math.max(1, x1 - x0);
    function px(x) { return L + (x - x0) / span * (W - L - R); }
    function py(y) { return T + (1 - y / max) * (H - T - B); }
    var fmtY = opts.fmtY || function (v) { return String(Math.round(v * 10) / 10); };
    for (var i = 0; i <= 4; i++) {
      var gy = T + i * (H - T - B) / 4;
      svg.appendChild(el('line', { x1: L, x2: W - R, y1: gy, y2: gy, class: 'grid-line' }));
      svg.appendChild(el('text', { x: L - 6, y: gy + 3, 'text-anchor': 'end' }, fmtY(max * (1 - i / 4))));
    }
    var fmtX = opts.fmtX || String;
    svg.appendChild(el('text', { x: L, y: H - 6, 'text-anchor': 'start' }, fmtX(x0)));
    svg.appendChild(el('text', { x: W - R, y: H - 6, 'text-anchor': 'end' }, fmtX(x1)));
    var d = '', run = [];
    function flush() {
      if (run.length > 1) {
        d += 'M' + run.map(function (p) { return px(p.x).toFixed(1) + ' ' + py(p.y).toFixed(1); }).join('L');
        svg.appendChild(el('path', { d: 'M' + px(run[0].x).toFixed(1) + ' ' + py(0) + 'L' + run.map(function (p) { return px(p.x).toFixed(1) + ' ' + py(p.y).toFixed(1); }).join('L') + 'L' + px(run[run.length - 1].x).toFixed(1) + ' ' + py(0) + 'Z', class: 'area' }));
      } else if (run.length === 1) {
        svg.appendChild(el('circle', { cx: px(run[0].x), cy: py(run[0].y), r: 2.5, fill: 'var(--chart-1)' }));
      }
      run = [];
    }
    points.forEach(function (p) { if (p.y == null) { flush(); } else { run.push(p); } });
    flush();
    if (d) { svg.appendChild(el('path', { d: d, class: 'line' })); }
    return svg;
  }
  /* samples: [{up:boolean}] -> one bar per sample, green/red */
  function strip(samples, opts) {
    var W = 600, H = 36, n = Math.max(1, samples.length), gap = n > 200 ? 0 : 1, bw = W / n;
    var svg = root(W, H, (opts && opts.label) || 'timeline');
    samples.forEach(function (s, i) {
      svg.appendChild(el('rect', { x: (i * bw).toFixed(2), y: 0, width: Math.max(0.5, bw - gap).toFixed(2), height: H, rx: 1, class: s.up ? 'bar-ok' : 'bar-bad' }));
    });
    if (!samples.length) { svg.appendChild(el('rect', { x: 0, y: 0, width: W, height: H, rx: 4, class: 'bar-none' })); }
    return svg;
  }
  /* ring: frac 0..1, center text, tone ok|warn|bad|info */
  function ring(frac, center, tone, label) {
    var S = 120, r = 46, c = 2 * Math.PI * r, f = Math.max(0, Math.min(1, frac || 0));
    var svg = root(S, S, label || center);
    svg.setAttribute('style', 'max-width:140px;margin-inline:auto');
    svg.appendChild(el('circle', { cx: 60, cy: 60, r: r, class: 'track' }));
    var arc = el('circle', { cx: 60, cy: 60, r: r, class: 'arc ' + (tone || 'info'), 'stroke-dasharray': (f * c).toFixed(1) + ' ' + c.toFixed(1), transform: 'rotate(-90 60 60)' });
    svg.appendChild(arc);
    svg.appendChild(el('text', { x: 60, y: 66, 'text-anchor': 'middle', class: 'big' }, center));
    return svg;
  }
  window.Charts = { line: line, strip: strip, ring: ring };
})();
