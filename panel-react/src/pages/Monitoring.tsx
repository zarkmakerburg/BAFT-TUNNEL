import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { fmtBytes, fmtNum, fmtTime, stats, warnings } from '../derive';
import { Card, Empty, KV, Ltr, Pill, Segmented } from '../components/ui';
import { LineChart, StatusStrip } from '../components/charts';
import { TrafficBody, WarnList, nv } from './parts';

/** Only ranges the backend really keeps: the last hour at 5 s and 24 h at one minute. Nothing is interpolated. */
export function Monitoring() {
  const { t, lang } = useApp();
  const { data: d } = useData();
  const [range, setRange] = useState<'1h' | '24h'>('1h');
  if (!d?.status) return null;
  const st = stats(d), hist = st.hist;
  const histEmpty = d.meta.history === 'unavailable' ? t('ui.unavail.history') : t('empty.history');
  const b = (n: number) => fmtBytes(lang, n);

  const longBody = () => {
    const lh = d.history?.long;
    const sm: any[] = lh?.samples ?? [];
    if (!sm.length) return <Empty>{t('empty.history')}</Empty>;
    const tot = lh.totals ?? {};
    const pct = tot.probes ? (tot.up / tot.probes) * 100 : null;
    return (
      <div>
        <KV rows={[[t('ov.avail'), pct == null ? '—' : <Ltr key="a">{fmtNum(lang, pct, 1)}%</Ltr>], [t('tr.from'), <Ltr key="f">{b(tot.from_peer_bytes)}</Ltr>], [t('tr.to'), <Ltr key="t">{b(tot.to_peer_bytes)}</Ltr>]]} />
        <h3>{t('ov.avail')}</h3>
        <LineChart label={`${t('ov.avail')} (%)`} pts={sm.map((x) => ({ x: x.t, y: x.probes ? (x.up / x.probes) * 100 : null }))} fmtY={(v) => fmtNum(lang, v, 0)} />
        <h3>{t('tr.bytes.min')}</h3>
        <LineChart label={t('tr.bytes.min')} pts={sm.map((x) => ({ x: x.t, y: (x.from_peer_bytes ?? 0) + (x.to_peer_bytes ?? 0) }))} fmtY={(v) => b(v) ?? ''} />
        <p className="sub">{t('lh.src', { m: fmtNum(lang, (lh.bucket_s ?? 60) / 60, 0), h: fmtNum(lang, (lh.retention_s ?? 0) / 3600, 0) })}</p>
        {!lh.persisted && <p className="sub">{t('lh.nopersist')}</p>}
        {(lh.load_error || lh.write_error) && <p className="sub">{t('lh.error')}: {lh.write_error ?? lh.load_error}</p>}
      </div>
    );
  };

  let body;
  if (d.meta.history === 'error') body = <Empty kind="error">{t('ui.err.history')}</Empty>;
  else if (range === '24h') body = <Card title={t('lh.title')}>{longBody()}</Card>;
  else {
    body = (
      <div className="stack">
        <Card title={t('ov.health')}>
          {hist.length ? (
            <>
              <StatusStrip samples={hist} label={t('ov.health')} />
              <KV rows={[
                [t('ov.avail'), st.avail == null ? '—' : <Ltr key="a">{fmtNum(lang, st.avail, 1)}%</Ltr>],
                [t('f.latency'), st.med == null ? '—' : <Ltr key="l">{fmtNum(lang, st.med, 2)} ms</Ltr>],
                [t('ov.window', { n: fmtNum(lang, hist.length, 0) }), `${fmtTime(lang, hist[0]!.t)} – ${fmtTime(lang, hist[hist.length - 1]!.t)}`],
              ]} />
              <p className="sub">{t('ov.health.src', { s: nv(lang, d.history?.interval_s), n: nv(lang, d.history?.capacity) })}</p>
            </>
          ) : <Empty>{histEmpty}</Empty>}
        </Card>
        <Card title={t('ov.latency')}>
          {st.med != null
            ? <LineChart label={t('ov.latency')} pts={hist.map((x) => ({ x: x.t, y: x.up ? x.rtt_ms ?? null : null }))} fmtY={(v) => fmtNum(lang, v, 1)} />
            : <Empty>{hist.length ? t('ov.latency.none') : histEmpty}</Empty>}
        </Card>
        <Card title={t('ov.traffic.title')}><TrafficBody d={d} /></Card>
      </div>
    );
  }
  const runs: any[] = d.history?.diagnostics_runs ?? [];
  return (
    <div className="stack">
      <div className="toolbar">
        <Segmented value={range} onChange={setRange} label={t('mon.range')} options={[['1h', t('mon.1h')], ['24h', t('mon.24h')]]} />
        <span className="sub">{t('mon.note')}</span>
      </div>
      {body}
      <Card title={t('ov.warnings')} className="compact"><WarnList ws={warnings(d, lang, t).filter((w) => w.href === '#/monitoring')} /></Card>
      {runs.length > 0 && (
        <Card title={t('lh.runs')}>
          <ul className="list">
            {runs.slice(-5).reverse().map((r, i) => (
              <li key={i}>
                <div className="top"><Pill kind={r.verified ? 'ok' : 'warn'}>{t(r.verified ? 'lh.verified' : 'lh.notverified')}</Pill><span className="sub">{fmtTime(lang, r.at_unix)}</span></div>
                <div className="sub"><Ltr>{(r.tiers ?? []).map((x: [string, string]) => `${x[0]}: ${x[1]}`).join(' · ')}</Ltr></div>
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}
