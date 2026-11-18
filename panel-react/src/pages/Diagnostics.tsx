import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { newKey } from '../api';
import { ago, fmtNum, type TFn } from '../derive';
import { Card, CopyButton, Empty, KV, Ltr, Pill, Table } from '../components/ui';
import { OpsBody, TierPill } from './parts';

function tr(t: TFn, key: string | undefined, params: any, fallback: string): string {
  return key && t(key, params ?? {}) !== key ? t(key, params ?? {}) : fallback;
}

function Report({ rep }: { rep: any }) {
  const { t, lang } = useApp();
  const tiers: any[] = rep.tiers ?? [];
  const na = rep.next_action ?? null;
  let nextText = '';
  if (na) {
    const key = na.code ?? `diag.next.${na.action}`;
    const params = { ...(na.params ?? {}) };
    if (na.code === 'diag.next.rerun_tier' && params.tier) params.tier = t('tier.' + params.tier);
    const l = t(key, params);
    nextText = l !== key ? l : t('diag.next.' + na.action) === 'diag.next.' + na.action ? '' : `${t('diag.next.' + na.action)} · ${na.text ?? ''}`;
  }
  const wrap = (s: string) => (lang === 'fa' ? s : <Ltr>{s}</Ltr>);
  return (
    <div>
      <p className="sub">{t('diag.at', { ago: ago(lang, t, rep.at_unix) })} · {rep.verified ? t('diag.verified') : t('diag.notverified')}{rep.highest_passed_tier ? ' · ' + t('diag.highest', { tier: t('tier.' + rep.highest_passed_tier) }) : ''}</p>
      {na && nextText && <p className="next-action" role="status"><strong>{t('diag.next')} </strong>{nextText}</p>}
      <Table caption={t('diag.title')} cols={['diag.col.tier', 'diag.col.result', 'diag.col.source', 'diag.col.evidence', 'diag.col.next'].map((k) => t(k))}
        rows={tiers.map((x) => [
          t('tier.' + x.id) === 'tier.' + x.id ? x.id : t('tier.' + x.id),
          <TierPill key="r" result={x.result} />,
          t('diag.source.' + x.source) === 'diag.source.' + x.source ? <Ltr key="s">{x.source}</Ltr> : t('diag.source.' + x.source),
          wrap(tr(t, x.code, x.params, x.evidence ?? '—')),
          x.next ? wrap(tr(t, x.next_code, x.next_params, x.next)) : '—',
        ])} />
      <p className="sub">{t('diag.integrity')}</p>
      <CopyButton label={t('rx.copy.details')} text={JSON.stringify(rep, null, 2)} />
    </div>
  );
}

export function Diagnostics() {
  const { t, lang, call, hold } = useApp();
  const { data: d, refresh } = useData();
  const [report, setReport] = useState<any>(null);
  const [running, setRunning] = useState(false);
  const [diagErr, setDiagErr] = useState(false);
  const [vBusy, setVBusy] = useState(false);
  const [vOut, setVOut] = useState<any>(null);
  const [vKey, setVKey] = useState<string | null>(null);
  if (!d?.status) return null;
  const rep = report ?? d.diagnostics;
  const tn = d.status.tunnel;
  const unk = <Pill kind="unknown">{t('state.unknown')}</Pill>;

  const runDiag = async () => {
    setRunning(true); setDiagErr(false); hold(true);
    try {
      const r = await call('POST', '/v1/diagnostics');
      if (r.status === 200) setReport(r.body); else setDiagErr(true);
    } catch { setDiagErr(true); } finally { setRunning(false); hold(false); refresh(); }
  };
  const runVerify = async () => {
    setVBusy(true); hold(true);
    const key = vKey ?? newKey('ui-verify-');
    setVKey(key);
    try {
      const r = await call('POST', '/v1/verify', { idempotency_key: key });
      if (r.status === 200) setVKey(null);
      setVOut({ status: r.status, body: r.body });
    } catch { setVOut({ status: 0, body: {} }); } finally { setVBusy(false); hold(false); refresh(); }
  };
  const verifyView = () => {
    if (!vOut) return null;
    const v = vOut.body ?? {};
    const when = v.timestamp ? ago(lang, t, v.timestamp) : t('unknown');
    if (vOut.status === 200 && v.result === 'pass' && v.sha256_match === true)
      return <><Pill kind="ok">{t('verify.result.pass')}</Pill> {t('verify.pass.prefix')} <Ltr>{v.bytes == null ? '—' : fmtNum(lang, v.bytes, 0)}</Ltr>{t('verify.pass.bytes')}<Ltr>{v.duration_ms == null ? '—' : fmtNum(lang, v.duration_ms, 0)}</Ltr>{t('verify.pass.duration')}{when}</>;
    if (vOut.status === 200 && v.result === 'fail') return <><Pill kind="bad">{t('verify.result.fail')}</Pill> {t('verify.fail', { reason: t('verify.reason.' + (v.reason ?? 'invalid_probe_response')), when })}</>;
    if (vOut.status === 200) return <><Pill kind="unknown">{t('verify.result.unknown')}</Pill> {t('verify.unknown', { reason: t('verify.reason.' + (v.reason ?? 'probe_failed')), when })}</>;
    return <span role="alert">{t('verify.failed')}</span>;
  };

  return (
    <div className="stack">
      <p className="muted">{t('diag.lead')}</p>
      <Card title={t('diag.title')}>
        <p className="sub">{t('diag.note')}</p>
        <button type="button" className="primary" disabled={running} onClick={() => void runDiag()}>{t('diag.run')}</button>
        <div aria-live="polite">
          {running ? <p>{t('diag.running')}</p> : diagErr ? <p className="fielderr" role="alert">{t('diag.failed')}</p>
            : rep ? <Report rep={rep} /> : <Empty kind={d.meta.diagnostics === 'error' ? 'error' : 'empty'}>{d.meta.diagnostics === 'error' ? t('ui.err.diag') : t('diag.none')}</Empty>}
        </div>
      </Card>
      <div className="grid cols-2">
        <Card title={t('verify.title')}>
          <p className="sub">{t('verify.note')}</p>
          <button type="button" disabled={vBusy} onClick={() => void runVerify()}>{t('verify.run')}</button>
          <div className="sub" aria-live="polite">{vBusy ? t('verify.running') : verifyView()}</div>
        </Card>
        <Card title={t('diag.state')}>
          <KV rows={[
            [t('tier.data_path'), tn ? <TierPill key="d" result={tn.tiers?.data_path?.result} /> : unk],
            [t('tier.integrity'), tn ? <TierPill key="i" result={tn.tiers?.integrity?.result} /> : unk],
            [t('ts.lastfail'), tn?.last_failure ? `${t('tier.' + tn.last_failure.stage)} · ${ago(lang, t, tn.last_failure.at_unix)}` : t('ts.nofail')],
          ]} />
        </Card>
      </div>
      <Card title={t('ops.title')}><OpsBody /></Card>
    </div>
  );
}
