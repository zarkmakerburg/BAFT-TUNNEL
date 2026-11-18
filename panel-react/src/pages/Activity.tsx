import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { fmtDate, opLabel } from '../derive';
import { Card, Empty, Ltr, Pill, Table } from '../components/ui';

/** The audit journal the Agent keeps. Filter by result and search over what the entries actually contain. */
export function Activity() {
  const { t, lang } = useApp();
  const { data: d } = useData();
  const [res, setRes] = useState<'all' | 'accepted' | 'rejected'>('all');
  const [q, setQ] = useState('');
  if (!d?.status) return null;
  const a = d.audit;
  if (!a) return <Card title={t('activity.title')}><Empty kind={d.meta.audit === 'error' ? 'error' : 'unavailable'}>{d.meta.audit === 'error' ? t('ui.err.audit') : t('activity.disabled')}</Empty></Card>;
  const all: any[] = [...(a.entries ?? [])].reverse();
  const needle = q.trim().toLowerCase();
  const rows = all.filter((e) => (res === 'all' || e.result === res) && (!needle || [opLabel(e, t), e.detail, e.target?.node, e.target?.instance, e.receipt_id, e.resource].filter(Boolean).join(' ').toLowerCase().includes(needle)));
  return (
    <Card title={t('activity.title')}>
      <p className="sub">{t('activity.note')} {t('activity.actor')}</p>
      <div className="toolbar">
        <div className="field inline"><label htmlFor="act-res">{t('filter.result')}</label>
          <select id="act-res" value={res} onChange={(e) => setRes(e.target.value as typeof res)}>
            <option value="all">{t('filter.all')}</option><option value="accepted">{t('accepted')}</option><option value="rejected">{t('rejected')}</option>
          </select></div>
        <div className="field inline"><label htmlFor="act-q">{t('rx.search')}</label>
          <input id="act-q" type="search" value={q} onChange={(e) => setQ(e.target.value)} /></div>
      </div>
      <div aria-live="polite" className="sr">{rows.length}</div>
      {rows.length ? (
        <Table caption={t('activity.title')} cols={['col.time', 'col.op', 'col.actor', 'col.target', 'col.result', 'col.receipt', 'col.detail'].map((k) => t(k))}
          rows={rows.map((e) => [
            fmtDate(lang, e.ts),
            opLabel(e, t),
            e.actor ? <Ltr key="a">{`${t('actor.token')} ${String(e.actor.fingerprint ?? '').slice(0, 8)}`}</Ltr> : t('unknown'),
            e.target ? <Ltr key="t">{[e.target.node, e.target.instance].filter(Boolean).join(' / ')}</Ltr> : '—',
            <Pill key="r" kind={e.result === 'accepted' ? 'ok' : 'bad'}>{t(e.result) === e.result ? e.result : t(e.result)}</Pill>,
            <code key="c" dir="ltr">{String(e.receipt_id ?? e.sha256 ?? '').slice(0, 12)}</code>,
            e.detail ? <Ltr key="d">{e.detail}</Ltr> : '—',
          ])} />
      ) : <Empty>{t('activity.empty')}</Empty>}
    </Card>
  );
}
