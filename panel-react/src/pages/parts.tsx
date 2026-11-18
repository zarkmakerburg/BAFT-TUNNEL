import type { ReactNode } from 'react';
import { useApp } from '../app';
import type { Loaded } from '../api';
import { ago, fmtBytes, fmtDate, fmtNum, fmtTime, opLabel, tierKind, type Warning } from '../derive';
import { Card, Empty, KV, Ltr, Pill } from '../components/ui';
import { LineChart } from '../components/charts';

export const nv = (lang: 'fa' | 'en', v: number | null | undefined): string => (v == null ? '—' : fmtNum(lang, v, 0));

export function WarnList({ ws, onNavigate }: { ws: Warning[]; onNavigate?: () => void }) {
  const { t } = useApp();
  if (!ws.length) return <p className="muted">{t('ov.nowarn')}</p>;
  const SEV = { bad: 'sev.error', warn: 'sev.warning', info: 'sev.info' } as const;
  return (
    <ul className="list">
      {ws.map((w, i) => (
        <li key={i} className={`w-${w.sev}`}>
          <div className="top"><Pill kind={w.sev}>{t(SEV[w.sev])}</Pill>{w.res && <strong><Ltr>{w.res}</Ltr></strong>}</div>
          <div>{w.text}</div>
          <div className="sub">{w.next}</div>
          <a className="more-link" href={w.href} onClick={onNavigate}>{t('ui.details')}</a>
        </li>
      ))}
    </ul>
  );
}
export const NotificationList = WarnList;

export function TierPill({ result }: { result: string | undefined }) {
  const { t } = useApp();
  const key = 'tier.r.' + (result ?? 'unknown');
  const l = t(key);
  return <Pill kind={tierKind(result)}>{l === key ? String(result) : l}</Pill>;
}

export function Flag({ v, bad }: { v: boolean | null | undefined; bad: 'warn' | 'bad' }) {
  const { t } = useApp();
  return v == null ? <Pill kind="unknown">{t('unknown')}</Pill> : <Pill kind={v ? 'ok' : bad}>{v ? t('yes') : t('no')}</Pill>;
}

export function CapPill({ state }: { state: string | undefined | null }) {
  const { t } = useApp();
  return state === 'ok' ? <Pill kind="ok">{t('cap.on')}</Pill> : state === 'unavailable' ? <Pill kind="unknown">{t('cap.off')}</Pill> : <Pill kind="unknown">{t('unknown')}</Pill>;
}

export function TrafficBody({ d }: { d: Loaded }) {
  const { t, lang } = useApp();
  const tr = d.traffic?.available ? d.traffic : null;
  const m = d.meta.traffic;
  if (!tr) return <Empty kind={m === 'error' ? 'error' : 'unavailable'}>{m === 'error' ? t('ui.err.traffic') : d.traffic?.reason ?? t('ov.traffic.body')}</Empty>;
  const r = tr.rate;
  const hist: any[] = tr.history ?? [];
  const b = (n: number) => fmtBytes(lang, n);
  return (
    <div>
      {tr.fresh === false && <p className="stale-note" role="status"><Pill kind="warn">{t('stale')}</Pill> {t('warn.stale')}</p>}
      <KV rows={[
        [t('tr.from'), <Ltr key="a">{b(tr.bytes_from_peer)}</Ltr>],
        [t('tr.to'), <Ltr key="b">{b(tr.bytes_to_peer)}</Ltr>],
        [t('tr.rate'), r ? <Ltr key="c">{`↓ ${b(r.from_peer_bps)}/s · ↑ ${b(r.to_peer_bps)}/s`}</Ltr> : t('tr.norate')],
        [t('tr.streams'), nv(lang, tr.streams_total)],
        [t('tr.sessions'), nv(lang, tr.sessions_total)],
        [t('tr.sampled'), `${ago(lang, t, tr.sampled_unix)} · ${t('tr.interval', { s: nv(lang, tr.sampling_interval_s) })}`],
      ]} />
      {hist.length > 1 && <LineChart label={`${t('tr.rate')} (B/s)`} pts={hist.map((x) => ({ x: x.t, y: (x.from_peer_bps ?? 0) + (x.to_peer_bps ?? 0) }))} fmtY={(v) => `${b(v)}/s`} />}
      <p className="sub">{t('tr.source')}</p>
    </div>
  );
}

export function RecentOps({ d, n }: { d: Loaded; n: number }) {
  const { t, lang } = useApp();
  if (!d.audit) return <p className="muted">{d.meta.audit === 'error' ? t('ui.err.audit') : t('activity.disabled')}</p>;
  const entries = [...(d.audit.entries ?? [])].reverse();
  if (!entries.length) return <p className="muted">{t('ov.recent.empty')}</p>;
  return (
    <ul className="list">
      {entries.slice(0, n).map((e: any, i: number) => (
        <li key={i}>
          <div className="top"><Pill kind={e.result === 'accepted' ? 'ok' : 'bad'}>{t(e.result) === e.result ? e.result : t(e.result)}</Pill><span className="sub">{fmtDate(lang, e.ts)}</span></div>
          <div>{opLabel(e, t)}</div>
        </li>
      ))}
    </ul>
  );
}

/** Sibling instances on this host (ADR-0036): links only where the address is reachable from here. */
export function HostTunnels({ d }: { d: Loaded }) {
  const { t } = useApp();
  const list: any[] | undefined = d.host?.tunnels;
  if (!list || d.meta.host !== 'ok') return <Card title={t('tunnels.host')}><p className="sub">{t('tunnels.host.none')}</p></Card>;
  const local = /^(127\.0\.0\.1|localhost|\[::1\])$/.test(location.hostname);
  return (
    <Card title={t('tunnels.host')}>
      <ul className="list">
        {list.map((x) => (
          <li key={x.instance} className="row-between">
            <span><Ltr>{x.instance}</Ltr> · <Ltr>{`${x.active}/${x.sub}`}</Ltr></span>
            {x.current ? <Pill kind="ok">{t('tunnels.host.this')}</Pill>
              : !x.agent_listen ? <span className="sub">{t('tunnels.host.noagent')}</span>
              : local ? <a className="btn" href={`http://${x.agent_listen}/`} target="_blank" rel="noopener noreferrer">{t('tunnels.host.open')}</a>
              : <span className="sub">{t('tunnels.host.addr', { a: x.agent_listen })}</span>}
          </li>
        ))}
      </ul>
      {d.host?.truncated && <p className="sub">{t('tunnels.host.more', { n: d.host.total })}</p>}
      <p className="sub">{t('tunnels.host.add')}</p>
    </Card>
  );
}

export function OpsBody() {
  const { t } = useApp();
  const cmds: [string, string][] = [['ops.backup', 'sudo scripts/backup.sh'], ['ops.restore', 'sudo scripts/restore.sh --stop --yes BACKUP_DIR'], ['ops.update', 'sudo scripts/update.sh --binary NEW_BINARY --instance NAME'], ['ops.rollback', 'sudo scripts/rollback.sh']];
  return (
    <div>
      <p className="sub">{t('ops.note')}</p>
      <ul className="list">{cmds.map(([k, c]) => <li key={k}><strong>{t(k)}</strong><div className="sub">{t(k + '.d')}</div><code dir="ltr">{c}</code></li>)}</ul>
      <p className="sub">{t('ops.limits')}</p>
    </div>
  );
}

export function PageLead({ children }: { children: ReactNode }) {
  return <p className="muted">{children}</p>;
}

export { fmtTime };
