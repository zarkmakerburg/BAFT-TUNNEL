import { useApp } from '../app';
import { useData } from '../feed-context';
import { ago, conf, fmtNum, life, listenState, roleLabel, stats, tunnelState } from '../derive';
import { Card, KV, Ltr, Pill } from '../components/ui';
import { CapPill, Flag, HostTunnels } from './parts';

/** This node plus the configured peer. The peer is never observed remotely, and the UI says so. */
export function Servers() {
  const { t, lang } = useApp();
  const { data: d, fetchedAt } = useData();
  if (!d?.status) return null;
  const s = d.status, c = conf(d), sec = d.security ?? {}, st = stats(d);
  const engines = ((d.engines?.engines ?? []) as any[]).map((e) => e.id).join(', ');
  const ls = s.tunnel ? tunnelState(s.tunnel, t) : listenState(s, t);
  const trust = sec.tls === false
    ? <p className="muted">{t('trust.noise')}</p>
    : d.meta.security !== 'ok' && d.meta.security !== undefined
      ? <p className="muted">{t('ui.err.security')}</p>
      : <KV rows={[
        [t('trust.pin'), <Flag key="p" v={c.peer_certificate_pin_configured} bad="warn" />],
        [t('trust.ca'), <Flag key="c" v={c.ca_file_present} bad="bad" />],
        [t('trust.certfile'), <Flag key="f" v={c.certificate_file_present} bad="bad" />],
        [t('f.cert'), life(sec.certificate_seconds_remaining, lang, t) ?? '—'],
        [t('f.crl'), life(sec.crl_seconds_remaining, lang, t) ?? '—'],
        [t('f.keysrc'), c.private_key_source ?? '—'],
      ]} />;
  const role = c.mode ? roleLabel(c.mode, c.direction, t) : '—';
  return (
    <div className="stack">
      <p className="muted">{t('servers.note')}</p>
      <div className="grid cols-2">
        <section className="card res">
          <div className="head">
            <div><div className="title"><Ltr>{c.local_identity}</Ltr></div><div className="sub">{t('servers.this')} · {role}</div></div>
            <Pill kind={ls.k}>{ls.l}</Pill>
          </div>
          <p className="sub">{t('state.why.local')}</p>
          <KV rows={[
            [t('f.lastseen'), t('checked', { ago: ago(lang, t, fetchedAt) })],
            [t('f.version'), <Ltr key="v">{'v' + s.version}</Ltr>],
            [t('f.engine'), <Ltr key="e">{s.engine}</Ltr>],
            [t('f.diag'), st.med == null ? t('unknown') : `${t('f.latency')}: ${fmtNum(lang, st.med, 2)} ms · ${t('f.avail')}: ${fmtNum(lang, st.avail ?? 0, 1)}%`],
          ]} />
          <hr className="sep" />
          <h3>{t('f.caps')}</h3>
          <ul className="list">
            <li>{t('cap.engines', { l: engines || '—' })}</li>
            <li>{t('cap.audit')}: <CapPill state={d.meta.audit} /></li>
            <li>{t('cap.desired')}: <CapPill state={d.meta.desired} /></li>
            <li>{t('cap.plan')}: <CapPill state={null} /></li>
            <li>{t('cap.apply')}: <CapPill state={null} /></li>
          </ul>
          <hr className="sep" />
          <h3>{t('f.trust')}</h3>
          {trust}
        </section>
        <section className="card res">
          <div className="head">
            <div><div className="title"><Ltr>{c.peer_identity}</Ltr></div><div className="sub">{t('servers.peer')}</div></div>
            <Pill kind="unknown">{t('state.unknown')}</Pill>
          </div>
          <p className="sub">{t('servers.peer.note')}</p>
          <KV rows={[[t('f.lastseen'), t('notobserved')], [t('f.version'), t('unknown')], [t('f.caps'), t('unknown')]]} />
        </section>
      </div>
      <HostTunnels d={d} />
      <Card title={t('fleet.title')}>
        <p className="sub">{t('fleet.note')}</p>
        <KV rows={[[t('fleet.hub'), <Pill key="h" kind="unknown">{t('fleet.hub.none')}</Pill>], [t('fleet.nodes'), t('fleet.nodes.none')]]} />
        <p className="sub">{t('fleet.add')}</p>
      </Card>
    </div>
  );
}
