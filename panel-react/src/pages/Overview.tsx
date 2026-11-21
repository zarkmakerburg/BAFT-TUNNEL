import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { ago, conf, fmtBytes, fmtNum, fmtTime, listenState, stats, tunnelState, warnings } from '../derive';
import { Banner, Card, Empty, Ltr, Pill, Segmented, Table } from '../components/ui';
import { Icon } from '../components/icons';
import { LineChart, StatusStrip } from '../components/charts';
import { GeoMap, Topology } from '../components/viz';
import { RecentOps, WarnList } from './parts';

type Direction = 'in' | 'out';

export function Overview() {
  const { t, lang } = useApp();
  const { data: d, fetchedAt } = useData();
  const [direction, setDirection] = useState<Direction>('in');
  if (!d?.status) return null;

  const s = d.status;
  const c = conf(d);
  const shortNode = (value?: string) => value?.startsWith('urn:baft-tunnel:node:') ? value.slice('urn:baft-tunnel:node:'.length) : value;
  const localName = s.instance ?? shortNode(c.local_identity) ?? t('unknown');
  const peerName = shortNode(c.peer_identity) ?? t('unknown');
  const st = stats(d);
  const ws = warnings(d, lang, t).filter((w) => w.sev !== 'info');
  const critical = ws.filter((w) => w.sev === 'bad').length;
  const warning = ws.filter((w) => w.sev === 'warn').length;
  const tr = d.traffic?.available ? d.traffic : null;
  const ts = s.tunnel ? tunnelState(s.tunnel, t) : listenState(s, t);
  const fresh = !c.tunnel_id;
  const trafficHistory: any[] = tr?.history ?? [];
  const rate = tr?.rate;
  const totalRate = rate && Number.isFinite(rate.from_peer_bps) && Number.isFinite(rate.to_peer_bps)
    ? rate.from_peer_bps + rate.to_peer_bps
    : null;
  const rateText = totalRate == null ? t('unknown') : `${fmtBytes(lang, totalRate)}/s`;
  const points = trafficHistory.map((x) => ({
    x: Number(x.t),
    y: direction === 'in' ? (x.from_peer_bps ?? null) : (x.to_peer_bps ?? null),
  }));
  const chartReady = points.filter((x) => x.y != null && Number.isFinite(x.y)).length >= 2;
  const histEmpty = d.meta.history === 'unavailable' ? t('ui.unavail.history') : t('empty.history');
  const unknown = <Pill kind="unknown">{t('unknown')}</Pill>;
  const tunnelPill = s.tunnel ? <Pill kind={ts.k}>{ts.l}</Pill> : unknown;
  const firewallNote = t('dash.table.note');

  return (
    <div className="enterprise-dashboard">
      {fresh && <Banner kind="info" title={t('ov.start.title')} action={<a className="btn primary" href="#/setup">{t('ov.start.cta')}</a>}><p>{t('ov.start.body')}</p></Banner>}

      <header className="dash-intro">
        <div>
          <p className="dash-eyebrow">{t('dash.eyebrow')} · BAFT-TUNNEL</p>
          <h2>{t('dash.title')}</h2>
          <p className="dash-subtitle">{t('dash.subtitle')}</p>
        </div>
        <div className="dash-meta">
          <Pill kind={ts.k}>{ts.l}</Pill>
          <span>{fetchedAt ? t('dash.updated', { ago: ago(lang, t, fetchedAt) }) : t('unknown')}</span>
        </div>
      </header>

      <section className="dash-grid" aria-label={t('dash.title')}>
        <article className="dash-kpi dash-span-3 tone-cyan">
          <div className="dash-kpi-top"><span className="dash-kpi-icon"><Icon name="servers" /></span><span className="dash-kpi-label">{t('dash.kpi.node')}</span></div>
          <strong className="dash-kpi-value"><Ltr>{localName}</Ltr></strong>
          <div className="dash-kpi-foot"><Pill kind={ts.k}>{ts.l}</Pill><span>{t('dash.kpi.version')} <Ltr>v{s.version ?? '—'}</Ltr></span></div>
        </article>

        <article className="dash-kpi dash-span-3 tone-green">
          <div className="dash-kpi-top"><span className="dash-kpi-icon"><Icon name="monitoring" /></span><span className="dash-kpi-label">{t('dash.kpi.latency')}</span></div>
          <strong className="dash-kpi-value">{st.med == null ? t('unknown') : <Ltr>{fmtNum(lang, st.med, 1)} ms</Ltr>}</strong>
          <div className="dash-kpi-foot"><span>{st.hist.length ? t('ov.window', { n: fmtNum(lang, st.hist.length, 0) }) : histEmpty}</span></div>
        </article>

        <article className="dash-kpi dash-span-3 tone-blue">
          <div className="dash-kpi-top"><span className="dash-kpi-icon"><Icon name="activity" /></span><span className="dash-kpi-label">{t('dash.kpi.throughput')}</span></div>
          <strong className="dash-kpi-value"><Ltr>{rateText}</Ltr></strong>
          <div className="dash-kpi-foot"><span>{tr?.sampled_unix ? t('kpi.traffic.live', { ago: ago(lang, t, tr.sampled_unix) }) : t('dash.traffic.unavailable')}</span>{tr && <Pill kind={tr.fresh ? 'ok' : 'warn'}>{tr.fresh ? t('fresh') : t('stale')}</Pill>}</div>
        </article>

        <article className="dash-kpi dash-span-3 tone-amber">
          <div className="dash-kpi-top"><span className="dash-kpi-icon"><Icon name="diagnostics" /></span><span className="dash-kpi-label">{t('dash.kpi.alerts')}</span></div>
          <strong className="dash-kpi-value">{fmtNum(lang, ws.length, 0)}</strong>
          <div className="dash-kpi-foot"><span>{t('dash.kpi.critical')}: {fmtNum(lang, critical, 0)}</span><span>{t('dash.kpi.warning')}: {fmtNum(lang, warning, 0)}</span></div>
        </article>

        <div className="dash-panel dash-span-7">
          <Topology d={d} />
        </div>

        <div className="dash-panel dash-span-5">
          <Card title={t('dash.sessions.title')} action={<Pill kind={ts.k}>{ts.l}</Pill>}>
            {tr ? (
              <>
                <div className="dash-session-metrics">
                  <div className="dash-session-primary">
                    <span className="dash-kpi-label">{t('dash.sessions.total')}</span>
                    <strong>{tr.sessions_total == null ? t('unknown') : fmtNum(lang, tr.sessions_total, 0)}</strong>
                  </div>
                  <div className="dash-session-secondary">
                    <span className="dash-kpi-label">{t('dash.streams.total')}</span>
                    <strong>{tr.streams_total == null ? t('unknown') : fmtNum(lang, tr.streams_total, 0)}</strong>
                  </div>
                </div>
                <p className="dash-note">{t('dash.sessions.note')}</p>
              </>
            ) : <Empty kind={d.meta.traffic === 'error' ? 'error' : 'unavailable'}>{d.meta.traffic === 'error' ? t('ui.err.traffic') : t('dash.sessions.unavailable')}</Empty>}
            <div className="dash-history">
              <div className="dash-section-label">{t('dash.history')}</div>
              {d.meta.history === 'error' ? <Empty kind="error">{t('ui.err.history')}</Empty> : st.hist.length
                ? <StatusStrip samples={st.hist} label={t('dash.history')} />
                : <Empty>{histEmpty}</Empty>}
            </div>
          </Card>
        </div>

        <div className="dash-panel dash-span-6">
          <Card title={t('dash.throughput.title')} action={
            <Segmented value={direction} onChange={setDirection} label={t('dash.throughput.title')} options={[['in', t('dash.throughput.in')], ['out', t('dash.throughput.out')]]} />
          }>
            {d.meta.traffic === 'error' ? <Empty kind="error">{t('ui.err.traffic')}</Empty>
              : chartReady ? (
                <div className={`dash-chart ${direction}`}>
                  <LineChart
                    label={direction === 'in' ? t('dash.throughput.in') : t('dash.throughput.out')}
                    pts={points}
                    fmtY={(v) => `${fmtBytes(lang, v)}/s`}
                  />
                </div>
              ) : <Empty kind={d.meta.traffic === 'unavailable' ? 'unavailable' : 'empty'}>{tr ? t('empty.history') : t('dash.traffic.unavailable')}</Empty>}
            <p className="dash-note">{t('dash.throughput.note')}</p>
          </Card>
        </div>

        <div className="dash-panel dash-span-6">
          <Card title={t('dash.link.title')}>
            <Table
              caption={t('dash.link.title')}
              cols={[t('dash.table.host'), t('dash.table.model'), t('dash.table.link1'), t('dash.table.link2'), t('dash.table.vpn')]}
              rows={[
                [
                  <span><strong><Ltr>{localName}</Ltr></strong><small className="dash-cell-sub">{t('dash.node.local')}</small></span>,
                  <Ltr>{s.engine ?? t('unknown')}</Ltr>,
                  s.tunnel ? <Pill kind={ts.k}>{ts.l}</Pill> : unknown,
                  unknown,
                  tunnelPill,
                ],
                [
                  <span><strong><Ltr>{peerName}</Ltr></strong><small className="dash-cell-sub">{t('dash.node.peer')}</small></span>,
                  unknown,
                  unknown,
                  unknown,
                  unknown,
                ],
              ]}
            />
            <p className="dash-note">{firewallNote}</p>
          </Card>
        </div>

        <div className="dash-panel dash-span-6">
          <GeoMap d={d} />
        </div>

        <div className="dash-panel dash-span-6 dash-stack">
          <Card title={t('dash.alerts.title')}><WarnList ws={ws} /></Card>
          <Card title={t('dash.activity.title')}><RecentOps d={d} n={5} /></Card>
        </div>
      </section>

      <p className="dash-footer">{fetchedAt ? `${t('checked', { ago: ago(lang, t, fetchedAt) })} · ${fmtTime(lang, fetchedAt)}` : ''}</p>
    </div>
  );
}
