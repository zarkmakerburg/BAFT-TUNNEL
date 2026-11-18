import { useApp } from '../app';
import { fmtTime } from '../derive';

export interface Pt { x: number; y: number | null }

/** Only real samples are drawn; a null value breaks the line instead of being interpolated. */
export function LineChart({ pts, label, fmtY }: { pts: Pt[]; label: string; fmtY: (v: number) => string }) {
  const { lang } = useApp();
  const real = pts.filter((p) => p.y != null) as { x: number; y: number }[];
  if (real.length < 2) return null;
  const W = 600, H = 140, P = 6;
  const xs = real.map((p) => p.x), ys = real.map((p) => p.y);
  const x0 = Math.min(...xs), x1 = Math.max(...xs), y0 = Math.min(0, ...ys), y1 = Math.max(...ys) || 1;
  const sx = (x: number) => P + ((x - x0) / (x1 - x0 || 1)) * (W - 2 * P);
  const sy = (y: number) => H - P - ((y - y0) / (y1 - y0 || 1)) * (H - 2 * P);
  let d = '';
  let pen = false;
  for (const p of pts) {
    if (p.y == null) { pen = false; continue; }
    d += `${pen ? 'L' : 'M'}${sx(p.x).toFixed(1)} ${sy(p.y).toFixed(1)} `;
    pen = true;
  }
  return (
    <figure className="chart">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${label}: ${fmtY(y0)} – ${fmtY(y1)}`} preserveAspectRatio="none">
        {[0.25, 0.5, 0.75].map((f) => <line key={f} className="grid" x1={P} x2={W - P} y1={H * f} y2={H * f} />)}
        <path className="line" d={d} />
      </svg>
      <figcaption className="legend"><span>{fmtTime(lang, x0)}</span><span dir="ltr">{fmtY(y1)} max</span><span>{fmtTime(lang, x1)}</span></figcaption>
    </figure>
  );
}

/** One cell per real probe: up, down; nothing is filled in for time without samples. */
export function StatusStrip({ samples, label }: { samples: { t: number; up: boolean }[]; label: string }) {
  const { lang } = useApp();
  if (!samples.length) return null;
  const n = samples.length, W = 600, H = 28, cell = Math.min(W / n, 8), gap = cell > 3 ? 1 : 0, w = Math.max(1, cell - gap);
  const up = samples.filter((s) => s.up).length;
  return (
    <figure className="chart">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${label}: ${up}/${n}`} preserveAspectRatio="none">
        {samples.map((s, i) => <rect key={i} className={s.up ? 'up' : 'down'} x={i * cell} y={0} width={w} height={H} />)}
      </svg>
      <figcaption className="legend"><span>{fmtTime(lang, samples[0]!.t)}</span><span>{fmtTime(lang, samples[n - 1]!.t)}</span></figcaption>
    </figure>
  );
}
