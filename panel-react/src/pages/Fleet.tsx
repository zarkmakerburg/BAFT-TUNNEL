import { useApp } from '../app';
import { useData } from '../feed-context';
import { Banner, Card, Empty, Kpi, Ltr, Pill } from '../components/ui';
import type { Kind } from '../derive';
import { fmtNum } from '../derive';
import { AutoPair } from './AutoPair';

const FRESH: Record<string, Kind> = { live: 'ok', stale: 'warn', never: 'unknown' };
const UPD: Record<string, Kind> = { current: 'ok', update_available: 'info' };

/** Fleet inventory (GET /v1/fleet/nodes). Stale or never-reported nodes show Unknown, never a last value. */
export function Fleet() {
  const { t, lang } = useApp();
  const { data: d } = useData();
  if (!d?.status) return null;
  const f = d.fleet;
  const nodes: any[] = Array.isArray(f?.nodes) ? f.nodes : [];
  const hub = f?.hub ?? {};
  const live = nodes.filter((n) => n.freshness === 'live').length;
  const stale = nodes.filter((n) => n.freshness === 'stale' || n.freshness === 'never').length;
  const upd = nodes.filter((n) => n.freshness === 'live' && n.update === 'update_available').length;
  const ageText = (s: number | null | undefined): string => {
    if (s == null) return t('fleet.age.none');
    const m = Math.round(s / 60);
    return t('fleet.age', { ago: m < 1 ? `${fmtNum(lang, s, 0)} s` : `${fmtNum(lang, m, 0)} min` });
  };
  return (
    <div className="stack">
      {d.meta.fleet === 'unavailable' && <Banner kind="info">{t('fleet.unavail')}</Banner>}
      {d.meta.fleet === 'error' && <Banner kind="warn">{t('fleet.err')}</Banner>}
      {f && (
        <>
          <div className="grid kpis">
            <Kpi label={t('fleet.k.nodes')} value={nodes.length ? fmtNum(lang, nodes.length, 0) : t('unknown')} />
            <Kpi label={t('fleet.k.live')} value={nodes.length ? fmtNum(lang, live, 0) : '—'} />
            <Kpi label={t('fleet.k.stale')} value={nodes.length ? fmtNum(lang, stale, 0) : '—'} />
            <Kpi label={t('fleet.k.upd')} value={nodes.length ? fmtNum(lang, upd, 0) : '—'} hint={f.latest_tag ? <>{t('fleet.latest')}: <Ltr>{f.latest_tag}</Ltr></> : undefined} />
          </div>
          <Card title={t('fleet.hub.title')} action={<Pill kind={hub.listener ? 'ok' : 'unknown'}>{hub.listener ? t('fleet.hub.on') : t('fleet.hub.off')}</Pill>}>
            {hub.reason && <p className="muted" dir="auto">{String(hub.reason)}</p>}
          </Card>
          {nodes.length === 0 ? (
            <Empty>{t('fleet.none')}</Empty>
          ) : (
            <div className="grid cols-2">
              {nodes.map((n) => {
                const fresh = String(n.freshness ?? 'never');
                const insts: any[] = Array.isArray(n.instances) ? n.instances : [];
                return (
                  <section key={n.node_id} className={`card res fleet-node fresh-${fresh}`}>
                    <div className="head">
                      <div><div className="title"><span className="fdot" aria-hidden="true" /><Ltr>{n.node_id}</Ltr></div><div className="sub">{ageText(n.age_s)}</div></div>
                      <Pill kind={FRESH[fresh] ?? 'unknown'}>{t('fleet.fresh.' + (FRESH[fresh] ? fresh : 'never'))}</Pill>
                    </div>
                    {fresh !== 'live' && <p className="muted">{t('fleet.stale.note')}</p>}
                    <dl className="kv">
                      <dt>{t('fleet.ver')}</dt><dd>{fresh === 'live' && n.version ? <Ltr>{'v' + n.version}</Ltr> : t('unknown')}</dd>
                      <dt>{t('fleet.upd')}</dt><dd><Pill kind={fresh === 'live' ? UPD[n.update] ?? 'unknown' : 'unknown'}>{t('fleet.upd.' + (fresh === 'live' && n.update ? n.update : 'unknown'))}</Pill></dd>
                    </dl>
                    <h3>{t('fleet.inst')}</h3>
                    {fresh === 'live' && insts.length ? (
                      <ul className="list">{insts.map((i) => <li key={i.name}><Ltr>{i.name}</Ltr>{i.tunnel ? <> · <Ltr>{String(i.tunnel)}</Ltr></> : null}{i.health ? <> · {String(i.health)}</> : null}</li>)}</ul>
                    ) : <p className="muted">{t('fleet.inst.none')}</p>}
                  </section>
                );
              })}
            </div>
          )}
        </>
      )}
      <Card title={t('fleet.add.title')}>
        <p className="muted">{t('fleet.add.intro')}</p>
        <AutoPair />
      </Card>
    </div>
  );
}
