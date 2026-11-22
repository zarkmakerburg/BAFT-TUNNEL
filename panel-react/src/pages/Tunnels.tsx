import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { newKey } from '../api';
import { ago, conf, fmtNum, listenState, roleLabel, stats, tunnelState } from '../derive';
import { Card, Confirm, KV, Ltr, Pill } from '../components/ui';
import { HostTunnels, TierPill } from './parts';

type Action = 'start' | 'stop' | 'restart';

function Receipt({ rc }: { rc: any }) {
  const { t } = useApp();
  const ok = rc.result === 'applied';
  const rk = 'rc.' + rc.result;
  return (
    <div className="card inner" role="status">
      <p><Pill kind={ok ? 'ok' : 'warn'}>{t(rk) === rk ? String(rc.result) : t(rk)}</Pill> {t('life.act.' + rc.stage)} · <Ltr>{rc.id ?? ''}</Ltr></p>
      {rc.reason && <p><Ltr>{rc.reason}</Ltr></p>}
      {rc.boot && <p className="sub">{rc.boot.enabled_at_boot ? t('life.boot.on') : t('life.boot.off')}</p>}
      {Array.isArray(rc.verification) && <ul>{rc.verification.map((v: any) => <li key={v.id}><Pill kind={v.result === 'pass' ? 'ok' : 'warn'}>{v.result}</Pill> <Ltr>{v.id}</Ltr><span className="muted"> · {v.evidence}</span></li>)}</ul>}
      {rc.rollback?.performed && <p className="fielderr">{t('life.rolled')}</p>}
      {Array.isArray(rc.not_done) && <details><summary>{t('rc.notdone')}</summary><ul>{rc.not_done.map((x: string) => <li key={x}>{x}</li>)}</ul></details>}
      <p className="sub">{t('rc.nosecret')}</p>
    </div>
  );
}

/** Start/Stop/Restart through the Agent's lifecycle endpoint. Nothing is simulated: the receipt is what the Agent returned. */
function Lifecycle({ tn }: { tn: any }) {
  const { t, call, toast, hold } = useApp();
  const { refresh } = useData();
  const [pending, setPending] = useState<Action | null>(null);
  const [busy, setBusy] = useState(false);
  const [out, setOut] = useState<{ receipt?: any; error?: string } | null>(null);
  const unit = tn?.tiers?.unit;
  if (!unit || (unit.result === 'unknown' && unit.reason)) return <p className="sub">{t('life.noinstance')}</p>;
  const up = unit.result === 'active';

  const run = async (action: Action) => {
    setBusy(true);
    hold(true);
    try {
      const r = await call('POST', '/v1/tunnel/lifecycle', { action, confirm: true, idempotency_key: newKey('ui-life-') });
      setOut(r.body?.result ? { receipt: r.body } : { error: r.body?.detail ?? r.body?.error ?? t('fail') });
      toast(t('life.done'), 'ok');
    } catch (e) {
      setOut({ error: e instanceof Error ? e.message : t('fail') });
    } finally {
      hold(false);
      setBusy(false);
      refresh();
    }
  };
  const btn = (a: Action, enabled: boolean, primary?: boolean) => (
    <button type="button" className={primary ? 'primary' : undefined} disabled={!enabled || busy} onClick={() => setPending(a)}>{t('life.act.' + a)}</button>
  );
  return (
    <div>
      <KV rows={[
        [t('life.state'), <Pill key="s" kind={up ? 'ok' : unit.result === 'inactive' ? 'warn' : 'unknown'}>{up ? t('life.running') : unit.result === 'inactive' ? t('life.stopped') : t('state.unknown')}</Pill>],
        [t('life.boot'), unit.enabled_at_boot == null ? t('unknown') : unit.enabled_at_boot ? t('life.boot.on') : t('life.boot.off')],
      ]} />
      <div className="row">{btn('start', !up, true)}{btn('restart', up)}{btn('stop', up || !!unit.enabled_at_boot)}</div>
      <p className="sub">{t('life.note')}</p>
      <div aria-live="polite">{busy ? t('life.working') : out?.receipt ? <Receipt rc={out.receipt} /> : out?.error ? <p className="fielderr" role="alert">{out.error}</p> : null}</div>
      <Confirm open={pending !== null} title={pending ? t('life.act.' + pending) : ''} body={pending ? t('life.confirm.' + pending) : ''} okLabel={pending ? t('life.act.' + pending) : ''} danger={pending === 'stop' || pending === 'restart'}
        onResult={(ok) => { const a = pending; setPending(null); if (ok && a) void run(a); }} />
    </div>
  );
}

/** The one tunnel this Agent is configured for. The backend has no multi-tunnel inventory, so none is drawn. */
export function Tunnels() {
  const { t, lang } = useApp();
  const { data: d, fetchedAt } = useData();
  if (!d?.status) return null;
  const s = d.status, c = conf(d), st = stats(d), ls = listenState(s, t), tn = s.tunnel ?? null, ts = tunnelState(tn, t);
  const tiers = tn?.tiers ?? {};
  const unk = <Pill kind="unknown">{t('state.unknown')}</Pill>;
  const dir = c.direction as 'direct' | 'reverse' | undefined;
  return (
    <div className="stack">
      <p className="muted">{t('tunnels.note')}</p>
      <div className="split">
        <section className="card res">
          <div className="head">
            <div>
              <div className="title"><Ltr>{c.tunnel_id}</Ltr></div>
              <div className="sub"><DirBadge dir={dir} /> <Ltr>{c.local_identity}</Ltr> ⇄ <Ltr>{c.peer_identity}</Ltr></div>
            </div>
            <Pill kind={tn ? ts.k : ls.k}>{tn ? ts.l : ls.l}</Pill>
          </div>
          <p className="sub">{(tn ? t('ts.why') : t('state.why.local')) + ' ' + t('checked', { ago: ago(lang, t, fetchedAt) })}</p>
          <KV rows={[
            [t('f.engine'), <Ltr key="e">{s.engine}</Ltr>],
            [t('f.role'), c.mode ? roleLabel(c.mode, dir, t) : '—'],
            [t('f.peerconn'), tn ? <TierPill key="p" result={tiers.authorized_session?.result} /> : unk],
            [t('f.peer'), tiers.authorized_session?.peer_id ? <Ltr key="pid">{tiers.authorized_session.peer_id}</Ltr> : null],
            [t('tier.data_path'), tn ? <TierPill key="dp" result={tiers.data_path?.result} /> : unk],
            [t('tier.integrity'), tn ? <TierPill key="ig" result={tiers.integrity?.result} /> : unk],
            [t('verify.last'), tiers.integrity?.last_probe
              ? <span key="lp"><Pill kind={tiers.integrity.last_probe.result === 'pass' ? 'ok' : tiers.integrity.last_probe.result === 'fail' ? 'bad' : 'unknown'}>{t('verify.result.' + tiers.integrity.last_probe.result)}</Pill> · {ago(lang, t, tiers.integrity.last_probe.timestamp)}</span>
              : t('never')],
            [t('f.verified'), tn?.last_verified_unix ? ago(lang, t, tn.last_verified_unix) : t('f.verified.none')],
            [t('ts.lastfail'), tn?.last_failure ? `${t('tier.' + tn.last_failure.stage)} · ${ago(lang, t, tn.last_failure.at_unix)}` : t('ts.nofail')],
          ]} />
          <a className="btn" href="#/diagnostics">{t('tunnels.troubleshoot')}</a>
        </section>
        <div className="stack">
          <Card title={t('tunnels.metrics')}>
            <KV rows={[
              [t('f.latency'), st.med == null ? t('unknown') : <Ltr key="l">{fmtNum(lang, st.med, 2)} ms</Ltr>],
              [t('f.avail'), st.avail == null ? t('unknown') : <span key="a"><Ltr>{fmtNum(lang, st.avail, 1)}%</Ltr> · {t('ov.window', { n: fmtNum(lang, st.hist.length, 0) })}</span>],
            ]} />
          </Card>
          <Card title={t('tunnels.config')}>
            <KV rows={[
              [t('f.streams'), c.max_streams == null ? '—' : fmtNum(lang, c.max_streams, 0)],
              [t('f.pin'), c.peer_certificate_pin_configured == null ? '—' : c.peer_certificate_pin_configured ? t('yes') : t('no')],
              [t('f.protocol'), <Ltr key="pv">{'v' + (c.protocol_version ?? 1)}</Ltr>],
            ]} />
            <p className="sub">{t('tunnels.edit.na')}</p>
          </Card>
          <Card title={t('tunnels.actions')}><Lifecycle tn={tn} /></Card>
        </div>
      </div>
      <HostTunnels d={d} />
      <div><a className="btn primary" href="#/setup">{t('tunnels.create')}</a></div>
    </div>
  );
}

/** Direct and Reverse are different topologies; Reverse always carries the Preview label. */
export function DirBadge({ dir }: { dir: 'direct' | 'reverse' | undefined }) {
  const { t } = useApp();
  if (!dir) return <span>—</span>;
  return <span className={`dir ${dir}`}>{t('dir.' + dir)}{dir === 'reverse' && <em> · {t('rx.mode.preview')}</em>}</span>;
}
