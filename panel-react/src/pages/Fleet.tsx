import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { Banner, Card, Empty, Field, Kpi, Ltr, Pill } from '../components/ui';
import type { Kind } from '../derive';
import { fmtNum } from '../derive';
import { AddServer } from './AddServer';

const FRESH: Record<string, Kind> = { live: 'ok', stale: 'warn', never: 'unknown' };
const UPD: Record<string, Kind> = { current: 'ok', update_available: 'info' };

/** Fleet inventory (GET /v1/fleet/nodes). Stale or never-reported nodes show Unknown, never a last value. */
export function Fleet() {
  const { t, lang } = useApp();
  const { data: d, refresh } = useData();
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
            <HubSetup hub={hub} refresh={refresh} />
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
        <AddServer />
      </Card>
    </div>
  );
}


function HubSetup({ hub, refresh }: { hub: any; refresh(): void }) {
  const { t, call, toast, hold } = useApp();
  const [hubName, setHubName] = useState('');
  const [dialSan, setDialSan] = useState('');
  const [listen, setListen] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const provision = async () => {
    setError('');
    if (!/^[a-z0-9][a-z0-9-]{0,62}$/.test(hubName)) { setError(t('fleet.hub.badname')); return; }
    if (!/^(DNS:[A-Za-z0-9.-]+|IP:[0-9a-fA-F:.]+)$/.test(dialSan)) { setError(t('fleet.hub.badsan')); return; }
    if (!/^(\[[0-9a-fA-F:]+\]|[0-9.]+):[1-9][0-9]{0,4}$/.test(listen)) { setError(t('fleet.hub.badlisten')); return; }
    setBusy(true); hold(true);
    try {
      const r = await call('POST', '/v1/fleet/hub/provision', { hub_name: hubName, dial_san: dialSan, listen });
      if (r.status === 202) {
        toast(t('fleet.hub.provisioned'), 'ok');
        setHubName(''); setDialSan(''); setListen('');
        window.setTimeout(refresh, 300);
      } else setError(r.body?.error ? String(r.body.error) : t('fail'));
    } catch { setError(t('fail')); }
    finally { hold(false); setBusy(false); }
  };

  const activate = async () => {
    setError(''); setBusy(true); hold(true);
    try {
      const r = await call('POST', '/v1/fleet/hub/restart', {});
      if (r.status === 202) toast(t('fleet.hub.restart'), 'ok');
      else setError(r.body?.error ? String(r.body.error) : t('fail'));
    } catch { setError(t('fleet.hub.restart.unknown')); }
    finally { hold(false); setBusy(false); }
    window.setTimeout(refresh, 2500);
  };

  return (
    <div className="stack">
      {!hub.configured && <p className="muted">{t('fleet.hub.notconfigured')}</p>}
      {hub.restart_required && <p className="muted">{t('fleet.hub.restart.required')}</p>}
      {hub.listener && hub.listen && <p><Ltr>{String(hub.listen)}</Ltr></p>}
      {error && <Banner kind="warn">{error}</Banner>}
      {!hub.configured && (
        <>
          <p className="muted">{t('fleet.hub.intro')}</p>
          <Field label={t('fleet.hub.name')} hint={t('fleet.hub.namehint')}>
            {(p) => <input {...p} dir="ltr" autoComplete="off" value={hubName} onChange={(e) => setHubName(e.target.value.toLowerCase())} />}
          </Field>
          <Field label={t('fleet.hub.san')} hint={t('fleet.hub.sanhint')}>
            {(p) => <input {...p} dir="ltr" autoComplete="off" spellCheck={false} placeholder="DNS:hub.example.com" value={dialSan} onChange={(e) => setDialSan(e.target.value)} />}
          </Field>
          <Field label={t('fleet.hub.listen')} hint={t('fleet.hub.listenhint')}>
            {(p) => <input {...p} dir="ltr" autoComplete="off" spellCheck={false} placeholder="0.0.0.0:18443" value={listen} onChange={(e) => setListen(e.target.value)} />}
          </Field>
          <button type="button" className="primary" disabled={busy || !hubName || !dialSan || !listen} onClick={() => void provision()}>{busy ? t('fleet.hub.busy') : t('fleet.hub.provision')}</button>
        </>
      )}
      {hub.configured && hub.restart_required && (
        <button type="button" className="primary" disabled={busy} onClick={() => void activate()}>{busy ? t('fleet.hub.busy') : t('fleet.hub.activate')}</button>
      )}
      {hub.listener && <p className="muted">{t('fleet.hub.active.note')}</p>}
    </div>
  );
}
