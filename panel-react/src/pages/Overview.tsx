import { useApp } from '../app';
import { useData } from '../feed-context';
import { ago, conf, fmtBytes, fmtNum, fmtTime, listenState, stats, tunnelState, warnings } from '../derive';
import { Banner, Card, Empty, Kpi, Ltr, Pill } from '../components/ui';
import { LineChart, StatusStrip } from '../components/charts';
import { GeoMap, Topology } from '../components/viz';
import { NetMark } from '../components/mark';
import { RecentOps, TrafficBody, WarnList, nv } from './parts';

export function Overview() {
  const { t, lang } = useApp();
  const { data: d, fetchedAt } = useData();
  const localPreview = import.meta.env.DEV && new URLSearchParams(window.location.search).get('preview') === '1';
  // In the local visual preview, keep the animation visible without inventing Agent data.
  if (!d?.status) {
    if (!localPreview) return null;
    return (
      <div className="stack">
        <div className="crest crest-unknown">
          <div className="crest-canvas">
            <NetMark state="unknown" label={t('crest.aria', { l: t('unknown') })} />
          </div>
          <div className="crest-body">
            <div className="crest-top"><Pill kind="unknown">{t('unknown')}</Pill></div>
            <p className="crest-id">{lang === 'fa' ? 'پیش‌نمایش انیمیشن پنل' : 'Panel animation preview'}</p>
            <p className="crest-why">{lang === 'fa' ? 'انیمیشن شبکه در حال اجراست. برای نمایش وضعیت و آمار واقعی، Agent باید متصل باشد.' : 'The network animation is running. Connect the Agent to display real status and metrics.'}</p>
          </div>
        </div>
      </div>
    );
  }
  const s = d.status;
  const c = conf(d);
  const st = stats(d);
  const ws = warnings(d, lang, t);
  const tr = d.traffic?.available ? d.traffic : null;
  const ts = s.tunnel ? tunnelState(s.tunnel, t) : listenState(s, t);
  const n = ws.filter((w) => w.sev !== 'info').length;
  const fresh = !c.tunnel_id;
  const hist = st.hist;
  const histEmpty = d.meta.history === 'unavailable' ? t('ui.unavail.history') : t('empty.history');
  const histErr = d.meta.history === 'error';
  return (
    <div className="stack">
      {localPreview && <div className="preview-notice" role="status">{lang === 'fa' ? 'پیش‌نمایش — داده‌ها آزمایشی هستند و وضعیت زندهٔ عامل نیستند.' : 'Preview — sample fixture data, not live Agent telemetry.'}</div>}
      {fresh && <Banner kind="info" title={t('ov.start.title')} action={<a className="btn primary" href="#/setup">{t('ov.start.cta')}</a>}><p>{t('ov.start.body')}</p></Banner>}
      <div className={`crest crest-${ts.k}`}>
        <div className="crest-canvas">
          <NetMark state={ts.k === 'ok' || ts.k === 'warn' || ts.k === 'bad' ? ts.k : 'unknown'} label={t('crest.aria', { l: ts.l })} />
        </div>
        <div className="crest-body">
          <div className="crest-top">
            <Pill kind={ts.k}>{ts.l}</Pill>
            {n > 0 && <a className="more-link" href="#/diagnostics">{t('ui.needs.action')}: {fmtNum(lang, n, 0)}</a>}
          </div>
          <p className="crest-id">{c.tunnel_id ? <Ltr>{c.tunnel_id}</Ltr> : t('ov.notunnel')}</p>
          <p className="crest-why">{s.tunnel ? t('ts.why') : t('state.why.local')}</p>
        </div>
      </div>
      {!fresh && <Topology d={d} />}
      <div className="grid kpis">
        <Kpi label={t('kpi.tunnels')} value={c.tunnel_id ? <Ltr>{c.tunnel_id}</Ltr> : '—'} hint={s.tunnel ? t('ts.why') : ''} state={ts} />
        <Kpi label={t('ov.avail')} value={st.avail == null ? t('unknown') : <Ltr>{fmtNum(lang, st.avail, 1)}%</Ltr>}
          hint={st.avail == null ? t('empty.history') : `${t('ov.window', { n: fmtNum(lang, hist.length, 0) })}${hist.length < 30 ? ' · ' + t('ov.fewsamples') : ''}`}
          state={st.avail == null ? { k: 'unknown', l: t('unknown') } : null} />
        {tr
          ? <Kpi label={t('kpi.traffic')} value={tr.bytes_from_peer == null || tr.bytes_to_peer == null ? t('unknown') : <Ltr>{fmtBytes(lang, tr.bytes_from_peer + tr.bytes_to_peer)}</Ltr>} hint={t('kpi.traffic.live', { ago: ago(lang, t, tr.sampled_unix) })} state={tr.fresh ? { k: 'ok', l: t('fresh') } : { k: 'warn', l: t('stale') }} />
          : <Kpi label={t('kpi.traffic')} value={t('kpi.traffic.value')} hint={t('kpi.traffic.hint')} state={{ k: 'unknown', l: t('unknown') }} />}
        <Kpi label={t('kpi.attention')} value={fmtNum(lang, n, 0)} hint={t('kpi.attention.hint')} state={n ? { k: ws.some((w) => w.sev === 'bad') ? 'bad' : 'warn', l: t('ui.needs.action') } : null} />
      </div>
      <div className="split">
        <div className="stack">
          <Card title={t('ov.health')} action={<a className="more-link" href="#/monitoring">{t('nav.monitoring')}</a>}>
            {histErr ? <Empty kind="error">{t('ui.err.history')}</Empty> : hist.length ? (
              <>
                <StatusStrip samples={hist} label={t('ov.health')} />
                <p className="sub">{t('ov.health.src', { s: nv(lang, d.history?.interval_s), n: nv(lang, d.history?.capacity) })}</p>
              </>
            ) : <Empty>{histEmpty}</Empty>}
          </Card>
          <Card title={t('ov.latency')}>
            {histErr ? <Empty kind="error">{t('ui.err.history')}</Empty> : hist.length && st.med != null
              ? <><LineChart label={`${t('ov.latency')} (${t('ov.latency.unit')})`} pts={hist.map((x) => ({ x: x.t, y: x.up ? x.rtt_ms ?? null : null }))} fmtY={(v) => fmtNum(lang, v, 1)} />
                  <div className="legend"><span>{t('ov.latency.unit')}</span><Ltr>{fmtNum(lang, st.med, 2)} ms</Ltr></div></>
              : <Empty>{hist.length ? t('ov.latency.none') : histEmpty}</Empty>}
          </Card>
          <Card title={t('ov.traffic.title')}><TrafficBody d={d} /></Card>
        </div>
        <div className="stack">
          {!fresh && <GeoMap d={d} />}
          <Card title={t('ov.warnings')}><WarnList ws={ws} /></Card>
          <Card title={t('ov.recent')} action={<a className="more-link" href="#/activity">{t('nav.activity')}</a>}><RecentOps d={d} n={5} /></Card>
        </div>
      </div>
      <p className="sub">{fetchedAt ? `${t('checked', { ago: ago(lang, t, fetchedAt) })} · ${fmtTime(lang, fetchedAt)}` : ''}</p>
    </div>
  );
}
