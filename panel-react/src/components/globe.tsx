import { useEffect, useMemo, useRef } from 'react';
import { LAND, MAP_H, MAP_S, MAP_TOP, MAP_W } from '../worldmap';

/** Land sample points (lon, lat) taken from the bundled land path, computed once in the browser. */
let LAND_PTS: [number, number][] | null = null;
function landPoints(): [number, number][] {
  if (LAND_PTS) return LAND_PTS;
  const c = document.createElement('canvas');
  c.width = MAP_W; c.height = MAP_H;
  const x = c.getContext('2d');
  const pts: [number, number][] = [];
  if (x) {
    x.fill(new Path2D(LAND));
    const px = x.getImageData(0, 0, MAP_W, MAP_H).data;
    const step = 2.2;
    for (let lat = -56; lat <= 82; lat += step) {
      for (let lon = -180; lon < 180; lon += step / Math.max(0.35, Math.cos((lat * Math.PI) / 180))) {
        const ix = Math.floor((lon + 180) * MAP_S), iy = Math.floor((MAP_TOP - lat) * MAP_S);
        if (ix >= 0 && ix < MAP_W && iy >= 0 && iy < MAP_H && (px[(iy * MAP_W + ix) * 4 + 3] ?? 0) > 40) pts.push([lon, lat]);
      }
    }
  }
  return (LAND_PTS = pts);
}

const R = Math.PI / 180;
type V3 = [number, number, number];
const toVec = (lon: number, lat: number): V3 => [Math.cos(lat * R) * Math.sin(lon * R), Math.sin(lat * R), Math.cos(lat * R) * Math.cos(lon * R)];

export interface GlobePin { lon: number; lat: number; label: string; kind: 'me' | 'peer' }

/** Orthographic 3D globe on a canvas: drag to rotate, great-circle arc lifted off the surface. No WebGL, no network. */
export function Globe({ pins, link, label, reset }: { pins: GlobePin[]; link: 'flow' | 'steady' | 'wait' | 'down' | 'none'; label: string; reset: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const rot = useRef({ lon: 50, lat: 28 });
  const pts = useMemo(() => (typeof document === 'undefined' ? [] : landPoints()), []);
  const pinKey = pins.map((p) => `${p.lon},${p.lat}`).join('|');

  // Face the endpoints whenever they change.
  useEffect(() => {
    if (!pins.length) return;
    const lon = pins.reduce((a, p) => a + p.lon, 0) / pins.length, lat = pins.reduce((a, p) => a + p.lat, 0) / pins.length;
    rot.current = { lon, lat: Math.max(-50, Math.min(60, lat)) };
  }, [pinKey]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const ctx = cv.getContext('2d');
    if (!ctx) return;
    const cs = getComputedStyle(document.documentElement);
    const col = (n: string, f: string) => cs.getPropertyValue(n).trim() || f;
    let raf = 0, dragging = false, lx = 0, ly = 0, t0 = performance.now();
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;

    const draw = (now: number) => {
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      const w = cv.clientWidth, h = cv.clientHeight;
      if (cv.width !== Math.round(w * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      const r = Math.min(w, h) * 0.46, cx = w / 2, cy = h / 2;
      const { lon: L0, lat: L1 } = rot.current;
      const sl = Math.sin(L1 * R), cl = Math.cos(L1 * R);
      // Rotate a unit vector so (L0, L1) faces the viewer; returns screen x,y and depth z.
      const proj = (lon: number, lat: number, lift = 0): [number, number, number] => {
        const v = toVec(lon - L0, lat);
        const y2 = v[1] * cl - v[2] * sl, z2 = v[1] * sl + v[2] * cl;
        const k = 1 + lift;
        return [cx + v[0] * r * k, cy - y2 * r * k, z2];
      };
      const accent = col('--accent', '#5cc8ff'), muted = col('--muted', '#a3aec9'), ok = col('--ok', '#4cd08f'), warn = col('--warn', '#f5b94c'), bad = col('--bad', '#ff8478'), unk = col('--unknown', '#a3aec9'), info = col('--info', '#9db4ff');
      // Sphere with a soft lit edge.
      const g = ctx.createRadialGradient(cx - r * 0.35, cy - r * 0.4, r * 0.1, cx, cy, r * 1.05);
      g.addColorStop(0, col('--surface-3', '#1f2c58')); g.addColorStop(1, col('--surface', '#101933'));
      ctx.beginPath(); ctx.arc(cx, cy, r, 0, Math.PI * 2); ctx.fillStyle = g; ctx.fill();
      ctx.lineWidth = 1.2; ctx.strokeStyle = accent; ctx.globalAlpha = 0.45; ctx.stroke(); ctx.globalAlpha = 1;
      // Graticule meridians/parallels.
      ctx.strokeStyle = muted; ctx.globalAlpha = 0.12; ctx.lineWidth = 0.6;
      for (let lat = -60; lat <= 60; lat += 30) { ctx.beginPath(); let pen = false; for (let lon = -180; lon <= 180; lon += 5) { const p = proj(lon, lat); if (p[2] > 0) { pen ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]); pen = true; } else pen = false; } ctx.stroke(); }
      for (let lon = -180; lon < 180; lon += 30) { ctx.beginPath(); let pen = false; for (let lat = -80; lat <= 80; lat += 5) { const p = proj(lon, lat); if (p[2] > 0) { pen ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]); pen = true; } else pen = false; } ctx.stroke(); }
      // Land dots, brighter toward the viewer.
      ctx.fillStyle = muted;
      for (const [lon, lat] of pts) {
        const p = proj(lon, lat);
        if (p[2] <= 0.02) continue;
        ctx.globalAlpha = 0.25 + 0.6 * p[2];
        ctx.fillRect(p[0] - 0.9, p[1] - 0.9, 1.8, 1.8);
      }
      ctx.globalAlpha = 1;
      // Arc between the two pins, lifted by a sine so it floats above the surface.
      const col2 = link === 'flow' || link === 'steady' ? ok : link === 'wait' ? warn : link === 'down' ? bad : unk;
      if (pins.length === 2) {
        const a = toVec(pins[0]!.lon, pins[0]!.lat), b = toVec(pins[1]!.lon, pins[1]!.lat);
        const dot = Math.max(-1, Math.min(1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2])), om = Math.acos(dot) || 1e-6;
        const path: [number, number, number][] = [];
        for (let i = 0; i <= 48; i++) {
          const f = i / 48, s1 = Math.sin((1 - f) * om) / Math.sin(om), s2 = Math.sin(f * om) / Math.sin(om);
          const v: V3 = [s1 * a[0] + s2 * b[0], s1 * a[1] + s2 * b[1], s1 * a[2] + s2 * b[2]];
          const lat = Math.asin(Math.max(-1, Math.min(1, v[1]))) / R, lon = Math.atan2(v[0], v[2]) / R;
          path.push(proj(lon, lat, Math.sin(f * Math.PI) * 0.3 * Math.min(1, om)));
        }
        ctx.lineWidth = 2; ctx.strokeStyle = col2; ctx.lineCap = 'round';
        ctx.setLineDash(link === 'steady' ? [] : link === 'flow' ? [7, 6] : [3, 7]);
        ctx.lineDashOffset = link === 'flow' && !reduce ? -((now - t0) / 40) : 0;
        ctx.beginPath(); let pen = false;
        for (const p of path) { if (p[2] > -0.05) { pen ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]); pen = true; } else pen = false; }
        ctx.stroke(); ctx.setLineDash([]);
        if (link === 'flow' && !reduce) {
          const q = path[Math.floor((((now - t0) / 1800) % 1) * path.length)];
          if (q && q[2] > -0.05) { ctx.fillStyle = col2; ctx.beginPath(); ctx.arc(q[0], q[1], 3.2, 0, Math.PI * 2); ctx.fill(); }
        }
      }
      // Pins and labels (only on the visible hemisphere).
      ctx.font = `600 ${Math.max(11, Math.min(14, w / 30))}px system-ui, sans-serif`; ctx.textAlign = 'center';
      for (const pin of pins) {
        const p = proj(pin.lon, pin.lat);
        if (p[2] <= 0) continue;
        const c = pin.kind === 'me' ? accent : info;
        ctx.fillStyle = c; ctx.globalAlpha = 0.25; ctx.beginPath(); ctx.arc(p[0], p[1], 10, 0, Math.PI * 2); ctx.fill();
        ctx.globalAlpha = 1; ctx.beginPath(); ctx.arc(p[0], p[1], 4, 0, Math.PI * 2); ctx.fill();
        ctx.lineWidth = 3.5; ctx.strokeStyle = col('--bg', '#080d1b'); ctx.lineJoin = 'round'; ctx.strokeText(pin.label, p[0], p[1] - 14);
        ctx.fillStyle = col('--fg', '#e9eef9'); ctx.fillText(pin.label, p[0], p[1] - 14);
      }
      if (link === 'flow' && !reduce) raf = requestAnimationFrame(draw);
    };
    const kick = () => { cancelAnimationFrame(raf); raf = requestAnimationFrame(draw); };
    const down = (e: PointerEvent) => { dragging = true; lx = e.clientX; ly = e.clientY; cv.setPointerCapture(e.pointerId); };
    const move = (e: PointerEvent) => {
      if (!dragging) return;
      rot.current = { lon: rot.current.lon - (e.clientX - lx) * 0.4, lat: Math.max(-80, Math.min(80, rot.current.lat + (e.clientY - ly) * 0.3)) };
      lx = e.clientX; ly = e.clientY; kick();
    };
    const up = () => { dragging = false; };
    cv.addEventListener('pointerdown', down); cv.addEventListener('pointermove', move); cv.addEventListener('pointerup', up); cv.addEventListener('pointercancel', up);
    const ro = new ResizeObserver(kick); ro.observe(cv);
    const reRot = () => kick();
    cv.addEventListener('baft:rot', reRot);
    kick();
    return () => { cancelAnimationFrame(raf); ro.disconnect(); cv.removeEventListener('pointerdown', down); cv.removeEventListener('pointermove', move); cv.removeEventListener('pointerup', up); cv.removeEventListener('pointercancel', up); cv.removeEventListener('baft:rot', reRot); };
  }, [pts, pinKey, link]); // eslint-disable-line react-hooks/exhaustive-deps

  const recentre = () => {
    if (pins.length) rot.current = { lon: pins.reduce((a, p) => a + p.lon, 0) / pins.length, lat: Math.max(-50, Math.min(60, pins.reduce((a, p) => a + p.lat, 0) / pins.length)) };
    ref.current?.dispatchEvent(new Event('baft:rot'));
  };
  return (
    <div className="globe-wrap">
      <canvas ref={ref} className="globe" role="img" aria-label={label} />
      <button type="button" className="ghost globe-reset" onClick={recentre}>{reset}</button>
    </div>
  );
}
