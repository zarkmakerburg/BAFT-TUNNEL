import { useEffect, useRef } from 'react';
import type { Kind } from '../derive';

/**
 * The mesh: 44 decorative nodes on a soft ellipsoid, with edges between nearby nodes.
 * Warm accents are a fixed visual treatment only; they do not identify live traffic or Agent nodes.
 *
 * Drawn with Canvas 2D — the same picture as the design lab, without WebGL or a 3D library.
 * Colours come from CSS tokens (--mark-warm / --mark-cool / --mark-edge / --bg), so both themes
 * and any future theme are honoured without this file knowing about them.
 */
export type MarkState = Extract<Kind, 'ok' | 'warn' | 'bad' | 'unknown'>;

type V3 = [number, number, number];
type RGB = [number, number, number];

const TAU = Math.PI * 2;

const NODES: V3[] = (() => {
  const n = 44, out: V3[] = [];
  for (let i = 0; i < n; i++) {
    const y = 1 - (i / (n - 1)) * 2;
    const r = Math.sqrt(Math.max(0, 1 - y * y));
    const a = i * 2.399963;
    out.push([Math.cos(a) * r * 1.18, y * 0.92 * 1.18, Math.sin(a) * r * 1.18]);
  }
  return out;
})();

const EDGES: [number, number, number][] = (() => {
  const out: [number, number, number][] = [];
  for (let i = 0; i < NODES.length; i++) {
    for (let j = i + 1; j < NODES.length; j++) {
      const a = NODES[i]!, b = NODES[j]!;
      const d = Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
      if (d < 0.92) out.push([i, j, d]);
    }
  }
  return out;
})();

// Fixed decorative accents; intentionally independent of telemetry.
const ACCENT = NODES.map((_, i) => i % 7 === 0);

function parse(v: string, fallback: RGB): RGB {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(v.trim());
  if (!m) return fallback;
  const h = m[1]!.length === 3 ? m[1]!.split('').map((c) => c + c).join('') : m[1]!;
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}
const rgba = (c: RGB, a: number) => `rgba(${c[0]},${c[1]},${c[2]},${Math.max(0, Math.min(1, a)).toFixed(3)})`;
const luma = (c: RGB) => (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255;

export function NetMark({ state, label }: { state: MarkState; label: string }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const ctx = cv.getContext('2d');
    if (!ctx) return;

    const root = document.documentElement;
    const motion = matchMedia('(prefers-reduced-motion: reduce)');
    let reduce = motion.matches;
    // A 30 Hz ceiling is visually smooth for this slow mesh and avoids wasting cycles on
    // 60/120/144 Hz displays. Hidden tabs and reduced-motion users are paused entirely.
    const frameInterval = 1000 / 30;
    let raf = 0, t0 = performance.now(), lastFrame = 0, running = false, inView = false;
    let warm: RGB = [224, 178, 100], cool: RGB = [174, 187, 203], edge: RGB = [126, 136, 152], light = false;

    const read = () => {
      const cs = getComputedStyle(root);
      const get = (n: string, f: string) => cs.getPropertyValue(n).trim() || f;
      warm = parse(get('--mark-warm', '#e0b264'), warm);
      cool = parse(get('--mark-cool', '#aebbcb'), cool);
      edge = parse(get('--mark-edge', '#7e8898'), edge);
      light = luma(parse(get('--bg', '#0a0a0b'), [10, 10, 11])) > 0.5;
    };
    read();

    // State transitions ease instead of snapping: the mesh opens up when the link goes down.
    const cur = { spread: 0, dim: 0, jitter: 0 };
    const goal = () => ({
      spread: state === 'bad' ? 0.16 : 0,
      dim: state === 'bad' ? 0.55 : state === 'unknown' ? 0.22 : 0,
      jitter: state === 'warn' ? 1 : 0,
    });

    const draw = (now: number) => {
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      const w = cv.clientWidth || 1, h = cv.clientHeight || 1;
      if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
        cv.width = Math.round(w * dpr);
        cv.height = Math.round(h * dpr);
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      const g = goal();
      const ease = reduce ? 1 : 0.07;
      cur.spread += (g.spread - cur.spread) * ease;
      cur.dim += (g.dim - cur.dim) * ease;
      cur.jitter += (g.jitter - cur.jitter) * ease;

      const t = reduce ? 0.42 : (now - t0) / 1000;
      const spin = t * 0.14;
      const cs = Math.cos(spin), sn = Math.sin(spin);
      const tx = Math.cos(-0.24), tz = Math.sin(-0.24);
      const S = Math.min(w, h) * 0.44, cx = w / 2, cy = h / 2;

      // soft core light, warm in dark and a quiet wash on sand
      const halo = ctx.createRadialGradient(cx, cy, 0, cx, cy, Math.min(w, h) * 0.40);
      halo.addColorStop(0, rgba(warm, light ? 0.10 : 0.16));
      halo.addColorStop(1, rgba(warm, 0));
      ctx.fillStyle = halo;
      ctx.fillRect(0, 0, w, h);

      const pts: [number, number, number][] = [];
      const r = 1 + cur.spread;
      for (let i = 0; i < NODES.length; i++) {
        const p = NODES[i]!;
        const j = cur.jitter * Math.sin(t * 2.2 + i * 1.7) * 0.014;
        const x0 = p[0] * r, y0 = p[1] * r, z0 = p[2] * r;
        const x1 = x0 * cs + z0 * sn;
        const z1 = -x0 * sn + z0 * cs;
        const y2 = y0 * tx - z1 * tz;
        const z2 = y0 * tz + z1 * tx;
        const k = 1 / (1 + z2 * 0.18);
        pts.push([cx + (x1 + j) * S * k, cy - (y2 + j) * S * k, k]);
      }

      ctx.globalCompositeOperation = light ? 'source-over' : 'lighter';
      ctx.lineWidth = 1;
      const edgeBase = light ? 1.45 : 1;
      for (const [i, j, d] of EDGES) {
        const a = pts[i]!, b = pts[j]!;
        const k = (a[2] + b[2]) / 2;
        const al = (0.28 - d * 0.22) * (0.52 + 0.48 * k) * (1 - cur.dim) * edgeBase;
        if (al <= 0.01) continue;
        ctx.strokeStyle = rgba(edge, al);
        ctx.beginPath();
        ctx.moveTo(a[0], a[1]);
        ctx.lineTo(b[0], b[1]);
        ctx.stroke();
      }

      for (let i = 0; i < pts.length; i++) {
        const [x, y, k] = pts[i]!;
        const isWarm = ACCENT[i]!;
        const col = isWarm ? warm : cool;
        const base = isWarm ? 0.58 + 0.34 * k : 0.30 + 0.44 * k;
        const al = base * (1 - cur.dim * (isWarm ? 0.75 : 0.35));
        if (isWarm && !light) {
          ctx.fillStyle = rgba(col, al * 0.16);
          ctx.beginPath();
          ctx.arc(x, y, (2.2 + 2.4 * k) * 2.4, 0, TAU);
          ctx.fill();
        }
        ctx.fillStyle = rgba(col, al);
        ctx.beginPath();
        ctx.arc(x, y, (isWarm ? 1.55 : 1.05) * (0.82 + 0.5 * k), 0, TAU);
        ctx.fill();
      }
      ctx.globalCompositeOperation = 'source-over';
    };

    const stop = () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      running = false;
    };

    const frame = (now: number) => {
      if (reduce || !inView || document.visibilityState !== 'visible') {
        stop();
        return;
      }
      if (now - lastFrame >= frameInterval) {
        lastFrame = now;
        draw(now);
      }
      raf = requestAnimationFrame(frame);
    };

    const start = () => {
      if (reduce || !inView || document.visibilityState !== 'visible' || running) return;
      t0 = performance.now();
      lastFrame = 0;
      running = true;
      raf = requestAnimationFrame(frame);
    };

    const onVisibility = () => {
      if (document.visibilityState === 'visible') {
        if (reduce) draw(performance.now());
        else start();
      } else {
        stop();
      }
    };
    const onMotion = (event: MediaQueryListEvent) => {
      reduce = event.matches;
      if (reduce) {
        stop();
        if (document.visibilityState === 'visible') draw(performance.now());
      } else {
        start();
      }
    };

    const ro = new ResizeObserver(() => { if (reduce && inView) draw(performance.now()); });
    ro.observe(cv);
    // Stop the animation when the mark scrolls out of view; a hidden canvas needs no frames.
    const io = new IntersectionObserver((entries) => {
      inView = entries.some((entry) => entry.isIntersecting);
      if (!inView) stop();
      else if (reduce) draw(performance.now());
      else start();
    }, { threshold: 0.01 });
    io.observe(cv);
    const mo = new MutationObserver(() => { read(); if (reduce) draw(performance.now()); });
    mo.observe(root, { attributes: true, attributeFilter: ['data-theme'] });
    const scheme = matchMedia('(prefers-color-scheme: light)');
    const onScheme = () => { read(); if (reduce) draw(performance.now()); };
    scheme.addEventListener('change', onScheme);
    motion.addEventListener('change', onMotion);
    document.addEventListener('visibilitychange', onVisibility);

    if (reduce) draw(performance.now());
    else start();

    return () => {
      stop();
      ro.disconnect();
      io.disconnect();
      mo.disconnect();
      scheme.removeEventListener('change', onScheme);
      motion.removeEventListener('change', onMotion);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [state]);

  return <canvas ref={ref} className="mark-canvas" role="img" aria-label={label} />;
}
