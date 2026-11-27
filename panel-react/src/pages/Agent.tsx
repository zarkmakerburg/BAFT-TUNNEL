import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { ago, conf, fmtNum, roleLabel } from '../derive';
import { Card, KV, Ltr, Pill } from '../components/ui';
import { CapPill, OpsBody, nv } from './parts';
import { PolicyCard } from './PolicyCard';

export function AgentPage() {
  const { t, lang, toast } = useApp();
  const { data: d, fetchedAt } = useData();
  const [out, setOut] = useState('');
  if (!d?.status) return null;
  const s = d.status, h = d.history ?? {}, c = conf(d);
  const check = async () => {
    const t0 = performance.now();
    try {
      const r = await fetch('/v1/health');
      if (!r.ok) throw new Error('x');
      await r.json();
      setOut(t('agent.healthok', { ms: fmtNum(lang, performance.now() - t0, 0) }));
    } catch { setOut(t('agent.healthfail')); toast(t('agent.healthfail'), 'bad'); }
  };
  return (
    <div className="stack">
      <div className="grid cols-2">
        <Card title={t('agent.health')}>
          <KV rows={[[t('f.version'), <Ltr key="v">{'v' + s.version}</Ltr>], [t('f.lastseen'), t('checked', { ago: ago(lang, t, fetchedAt) })]]} />
          <p className="sub">{t('agent.sampler', { s: nv(lang, h.interval_s), n: nv(lang, h.capacity) })}</p>
          <button type="button" onClick={() => void check()}>{t('agent.check')}</button>
          <p aria-live="polite">{out}</p>
        </Card>
        <Card title={t('rx.runtime')}>
          <KV rows={[
            [t('f.engine'), s.engine ? <Ltr key="e">{s.engine}</Ltr> : '—'],
            [t('rx.service'), s.listener_accepting == null ? <Pill key="l" kind="unknown">{t('unknown')}</Pill> : <Pill key="l" kind={s.listener_accepting ? 'ok' : 'bad'}>{s.listener_accepting ? t('state.listening') : t('state.notlistening')}</Pill>],
            [t('f.node'), c.local_identity ? <Ltr key="n">{c.local_identity}</Ltr> : '—'],
          ]} />
        </Card>
      </div>
      <Card title={t('rx.cfg.summary')}>
        <KV rows={[
          [t('kpi.tunnels'), c.tunnel_id ? <Ltr key="t">{c.tunnel_id}</Ltr> : t('rx.wiz.none')],
          [t('f.direction'), c.direction ? t('dir.' + c.direction) : '—'],
          [t('f.role'), c.mode ? roleLabel(c.mode, c.direction, t) : '—'],
          [t('f.peer'), c.peer_identity ? <Ltr key="p">{c.peer_identity}</Ltr> : '—'],
          [t('f.protocol'), c.protocol_version == null ? '—' : <Ltr key="pv">{'v' + c.protocol_version}</Ltr>],
        ]} />
      </Card>
      <PolicyCard />
      <Card title={t('agent.caps')}>
        <ul className="list">
          <li>{t('cap.audit')}: <CapPill state={d.meta.audit} /></li>
          <li>{t('cap.desired')}: <CapPill state={d.meta.desired} /></li>
          <li>{t('cap.plan')}: <CapPill state={null} /></li>
          <li>{t('cap.apply')}: <CapPill state={null} /></li>
        </ul>
        <p className="sub">{t('agent.loopback')}</p><p className="sub">{t('agent.secret')}</p>
      </Card>
      <Card title={t('ops.title')}><OpsBody /></Card>
    </div>
  );
}
