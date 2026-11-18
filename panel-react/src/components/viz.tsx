import { useEffect, useMemo, useState } from 'react';
import { useApp } from '../app';
import { ago, conf, fmtBytes, fmtNum, stats, tunnelState, listenState, type Kind } from '../derive';
import type { Loaded } from '../api';
import { Card, Ltr, Pill } from './ui';
import { LAND, MAP_H, MAP_S, MAP_TOP, MAP_W } from '../worldmap';

/* ---------- operator-declared location (no GeoIP yet: ADR-0052 offline database is not built) ---------- */

// Country centroids (lat, lon). Names come from Intl.DisplayNames, so nothing is hard-coded per language.
const CENTROID: Record<string, [number, number]> = {
  IR: [32.4, 53.7], TR: [39.0, 35.2], AE: [24.3, 54.3], DE: [51.2, 10.4], NL: [52.2, 5.3], FR: [46.6, 2.3], GB: [54.0, -2.5],
  FI: [64.0, 26.0], SE: [62.0, 15.0], PL: [52.0, 19.4], RO: [45.9, 25.0], BG: [42.7, 25.5], CH: [46.8, 8.2], AT: [47.5, 14.5],
  IT: [42.8, 12.6], ES: [40.2, -3.6], PT: [39.6, -8.0], UA: [49.0, 31.4], RU: [60.0, 90.0], KZ: [48.0, 67.0], AM: [40.3, 44.9],
  AZ: [40.3, 47.7], GE: [42.2, 43.5], IQ: [33.0, 44.0], SA: [24.0, 45.0], QA: [25.3, 51.2], IN: [21.0, 78.0], SG: [1.35, 103.8],
  HK: [22.3, 114.2], JP: [36.5, 138.0], KR: [36.5, 127.8], US: [39.0, -98.0], CA: [56.0, -100.0], BR: [-10.0, -52.0],
  ZA: [-29.0, 24.0], AU: [-25.0, 134.0], IL: [31.5, 34.9], LT: [55.3, 23.9], LV: [56.9, 24.6], EE: [58.7, 25.5], CZ: [49.8, 15.5],
};
const CODES = Object.keys(CENTROID);

function readDecl(id: string): string {
  try { return localStorage.getItem('baft.geo.' + id) ?? ''; } catch { return ''; }
}
function useDeclared(id: string | undefined): [string, (c: string) => void] {
  const [v, setV] = useState(() => (id ? readDecl(id) : ''));
  useEffect(() => { setV(id ? readDecl(id) : ''); }, [id]);
  const set = (c: string) => {
    setV(c);
    try { if (id) { if (c) localStorage.setItem('baft.geo.' + id, c); else localStorage.removeItem('baft.geo.' + id); } } catch { /* storage may be unavailable */ }
  };
  return [v, set];
}

const proj = (lat: number, lon: number): [number, number] => [(lon + 180) * MAP_S, (MAP_TOP - lat) * MAP_S];

/* ---------- small pieces ---------- */

export function Spark({ pts, label, cls = '' }: { pts: (number | null)[]; label: string; cls?: string }) {
  const real = pts.filter((v): v is number => v != null);
  if (real.length < 2) return <span className="spark-none sub">—</span>;
  const W = 120, H = 32, P = 2;
  const lo = Math.min(...real), hi = Math.max(...real);
  const n = pts.length - 1;
  let d = '', pen = false;
  pts.forEach((v, i) => {
    if (v == null) { pen = false; return; }
    d += `${pen ? 'L' : 'M'}${(P + (i / n) * (W - 2 * P)).toFixed(1)} ${(H - P - ((v - lo) / (hi - lo || 1)) * (H - 2 * P)).toFixed(1)} `;
    pen = true;
  });
  return <svg className={`spark ${cls}`} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={label} preserveAspectRatio="none"><path d={d} /></svg>;
}

type LinkKind = 'flow' | 'steady' | 'wait' | 'down' | 'none';
function linkOf(d: Loaded): LinkKind {
  const k = d.status?.tunnel?.state;
  return k === 'carrying_traffic' ? 'flow' : k === 'session_authorized' ? 'steady' : k === 'running_no_session' ? 'wait' : k === 'down' ? 'down' : 'none';
}

function NodeBox({ role, id, sub, kind }: { role: string; id?: string | null; sub?: string; kind: Kind }) {
  const { t } = useApp();
  return (
    <div className={`vnode vn-${kind}`}>
      <span className="vnode-ic" aria-hidden="true"><svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="6" rx="1.5" /><rect x="3" y="14" width="18" height="6" rx="1.5" /><circle cx="7" cy="7" r=".8" /><circle cx="7" cy="17" r=".8" /></svg></span>
      <span className="vnode-role">{role}</span>
      <strong className="vnode-id">{id ? <Ltr>{id}</Ltr> : t('unknown')}</strong>
      {sub && <span className="sub">{sub}</span>}
    </div>
  );
}

/* ---------- topology infographic ---------- */

export function Topology({ d }: { d: Loaded }) {
  const { t, lang } = useApp();
  const c = conf(d);
  const s = d.status;
  const lk = linkOf(d);
  const tr = d.traffic?.available ? d.traffic : null;
  const st = stats(d);
  const rate = tr?.fresh !== false ? tr?.rate : null;
  const dirKey = c.direction === 'reverse' ? 'reverse' : 'direct';
  const state = s?.tunnel ? tunnelState(s.tunnel, t) : listenState(s, t);
  const bps = (n: number | null | undefined) => (n == null ? t('unknown') : `${fmtBytes(lang, n)}/s`);
  const lastRtt = [...st.hist].reverse().find((x) => x.up && x.rtt_ms != null)?.rtt_ms ?? null;
  const rtts = st.hist.map((x) => (x.up ? x.rtt_ms ?? null : null));
  const rateHist: any[] = tr?.history ?? [];
  const aria = `${c.local_identity ?? t('unknown')} → ${c.peer_identity ?? t('unknown')}: ${state.l}`;
  return (
    <Card title={t('viz.topo.title')} action={<Pill kind={state.k}>{state.l}</Pill>}>
      <div className={`topo link-${lk}`} role="group" aria-label={aria}>
        <NodeBox role={t('viz.this')} id={c.local_identity} sub={c.mode ? t((c.direction === 'reverse' ? 'role.rev.' : 'role.') + c.mode) : undefined} kind={s ? 'ok' : 'unknown'} />
        <div className="wire" aria-hidden="true">
          <span className="wire-tag">{t('dir.' + dirKey + '.t')}{dirKey === 'reverse' ? ` · ${t('dir.preview')}` : ''}</span>
          <svg viewBox="0 0 200 24" preserveAspectRatio="none"><line x1="0" y1="12" x2="200" y2="12" className="wire-line" /></svg>
          <span className="wire-state">{state.l}</span>
        </div>
        <NodeBox role={t('viz.peer')} id={c.peer_identity} sub={t('viz.peer.note')} kind="unknown" />
      </div>
      <div className="vitals">
        <div className="vital"><span className="sub">{t('viz.rate.in')}</span><strong><Ltr>{bps(rate?.from_peer_bps)}</Ltr></strong><Spark pts={rateHist.map((x) => x.from_peer_bps ?? null)} label={t('viz.rate.in')} /></div>
        <div className="vital"><span className="sub">{t('viz.rate.out')}</span><strong><Ltr>{bps(rate?.to_peer_bps)}</Ltr></strong><Spark pts={rateHist.map((x) => x.to_peer_bps ?? null)} label={t('viz.rate.out')} cls="alt" /></div>
        <div className="vital"><span className="sub">{t('viz.rtt')}</span><strong>{lastRtt == null ? t('unknown') : <Ltr>{fmtNum(lang, lastRtt, 1)} ms</Ltr>}</strong><Spark pts={rtts} label={t('viz.rtt')} /></div>
        <div className="vital"><span className="sub">{t('viz.streams')}</span><strong>{tr?.streams_total == null ? t('unknown') : fmtNum(lang, tr.streams_total, 0)}</strong><span className="sub">{tr?.sessions_total == null ? '' : `${t('viz.sessions')}: ${fmtNum(lang, tr.sessions_total, 0)}`}</span></div>
        <div className="vital"><span className="sub">{t('ov.avail')}</span><strong>{st.avail == null ? t('unknown') : <Ltr>{fmtNum(lang, st.avail, 1)}%</Ltr>}</strong><span className="sub">{tr?.sampled_unix ? t('kpi.traffic.live', { ago: ago(lang, t, tr.sampled_unix) }) : ''}</span></div>
      </div>
      <p className="sub">{t('viz.honest')}</p>
    </Card>
  );
}

/* ---------- geographic view ---------- */

function CountrySelect({ label, value, onChange, detected }: { label: string; value: string; onChange: (c: string) => void; detected: string | null }) {
  const { lang, t } = useApp();
  const names = useMemo(() => {
    let dn: Intl.DisplayNames | null = null;
    try { dn = new Intl.DisplayNames([lang], { type: 'region' }); } catch { /* fall back to codes */ }
    return CODES.map((k) => ({ k, n: dn?.of(k) ?? k })).sort((a, b) => a.n.localeCompare(b.n, lang));
  }, [lang]);
  const nm = (k: string) => names.find((x) => x.k === k)?.n ?? k;
  return (
    <label className="geo-sel"><span className="sub">{label}</span>
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{detected ? t('viz.geo.auto', { c: nm(detected) }) : t('viz.geo.auto.none')}</option>
        {names.map((x) => <option key={x.k} value={x.k}>{x.n}</option>)}
      </select>
    </label>
  );
}

export function GeoMap({ d }: { d: Loaded }) {
  const { t, lang } = useApp();
  const c = conf(d);
  const lk = linkOf(d);
  const [mine, setMine] = useDeclared(c.local_identity);
  const [theirs, setTheirs] = useDeclared(c.peer_identity);
  // Detected values come from the Agent's offline database (GET /v1/geo); a user choice always wins.
  const det = (k: 'local' | 'peer'): string | null => {
    const v = d.geo?.[k]?.country;
    return typeof v === 'string' && /^[A-Za-z]{2}$/.test(v) ? v.toUpperCase() : null;
  };
  const dMine = det('local'), dTheirs = det('peer');
  const effMine = mine || dMine || '', effTheirs = theirs || dTheirs || '';
  const a = effMine && CENTROID[effMine] ? proj(...CENTROID[effMine]) : null;
  const b0 = effTheirs && CENTROID[effTheirs] ? proj(...CENTROID[effTheirs]) : null;
  const same = !!a && !!b0 && effMine === effTheirs;
  const b: [number, number] | null = b0 && same && a ? [a[0] + 10, a[1] + 6] : b0;
  const dn = (() => { try { return new Intl.DisplayNames([lang], { type: 'region' }); } catch { return null; } })();
  const nm = (k: string) => dn?.of(k) ?? k;
  let arc = '';
  if (a && b) {
    const mx = (a[0] + b[0]) / 2, my = (a[1] + b[1]) / 2 - Math.max(18, Math.hypot(a[0] - b[0], a[1] - b[1]) * 0.28);
    arc = `M${a[0].toFixed(1)} ${a[1].toFixed(1)} Q${mx.toFixed(1)} ${my.toFixed(1)} ${b[0].toFixed(1)} ${b[1].toFixed(1)}`;
  }
  // Frame the map around the declared endpoints when both are placed.
  const view = (() => {
    if (!a || !b) return `0 0 ${MAP_W} ${MAP_H}`;
    const pad = 70, x0 = Math.min(a[0], b[0]) - pad, x1 = Math.max(a[0], b[0]) + pad, y0 = Math.min(a[1], b[1]) - pad * 1.2, y1 = Math.max(a[1], b[1]) + pad * 0.8;
    const w = Math.max(x1 - x0, 260), h = w * (MAP_H / MAP_W) * 1.15;
    const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
    const vx = Math.max(0, Math.min(MAP_W - w, cx - w / 2)), vy = Math.max(0, Math.min(MAP_H - h, cy - h / 2));
    return `${vx.toFixed(0)} ${vy.toFixed(0)} ${w.toFixed(0)} ${Math.min(h, MAP_H).toFixed(0)}`;
  })();
  const pin = (p: [number, number] | null, kind: 'me' | 'peer', code: string) => p && (
    <g className={`pin pin-${kind}`}>
      <circle cx={p[0]} cy={p[1]} r="9" className="pin-halo" /><circle cx={p[0]} cy={p[1]} r="3.6" className="pin-dot" />
      <title>{`${kind === 'me' ? t('viz.this') : t('viz.peer')}: ${nm(code)}`}</title>
    </g>
  );
  return (
    <Card title={t('viz.geo.title')} action={<Pill kind={mine || theirs ? 'info' : dMine || dTheirs ? 'ok' : 'unknown'}>{mine || theirs ? t('viz.geo.declared.pill') : dMine || dTheirs ? t('viz.geo.detected.pill') : t('viz.geo.none.pill')}</Pill>}>
      <figure className={`geomap link-${lk}`}>
        <svg viewBox={view} role="img" aria-label={a && b ? t('viz.geo.aria', { a: nm(effMine), b: nm(effTheirs) }) : t('viz.geo.aria.none')} preserveAspectRatio="xMidYMid meet">
          <path d={LAND} className="land" />
          {arc && <path d={arc} className="arc" />}
          {pin(a, 'me', effMine)}{pin(b, 'peer', effTheirs)}
        </svg>
        {!(a && b) && <figcaption className="geo-hint sub">{t('viz.geo.unset.hint')}</figcaption>}
      </figure>
      <div className="geo-form">
        <CountrySelect label={`${t('viz.geo.me')}${c.local_identity ? ' · ' + c.local_identity : ''}`} value={mine} onChange={setMine} detected={dMine} />
        <CountrySelect label={`${t('viz.geo.peer')}${c.peer_identity ? ' · ' + c.peer_identity : ''}`} value={theirs} onChange={setTheirs} detected={dTheirs} />
      </div>
      <p className="sub">{d.meta.geo === 'ok' ? t('viz.geo.note.auto') : t('viz.geo.note')}</p>
      {(dMine || dTheirs) && typeof d.geo?.attribution === 'string' && <p className="sub" dir="ltr">{d.geo.attribution}</p>}
      {(d.geo?.local?.reason || d.geo?.peer?.reason) && <p className="sub">{[d.geo?.local?.reason, d.geo?.peer?.reason].filter(Boolean).join(' · ')}</p>}
    </Card>
  );
}
