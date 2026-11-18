import { useMemo, useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { conf, directionsOffered, type Kind } from '../derive';
import { Banner, Card, Field, KV, Ltr, Pill } from '../components/ui';
import { newKey } from '../api';
import { AutoPair } from './AutoPair';

type Dir = 'direct' | 'reverse';
interface Form { loc: 'iran' | 'abroad' | null; peerManual: boolean; direction: Dir | null; role: 'forward' | 'serve'; engine: string; node: string; peerId: string; peerHost: string; serverName: string; tunnel: string; port: string; tport: string; dir: string; inst: string }
const BLANK: Form = { loc: null, peerManual: false, direction: null, role: 'forward', engine: '', node: '', peerId: '', peerHost: '', serverName: '', tunnel: '', port: '', tport: '8443', dir: '/etc/baft-tunnel', inst: '' };

const x509 = (engine: string) => engine === 'tcp-tls' || engine === 'baft';
const portOk = (v: string) => /^\d+$/.test(v.trim()) && Number(v) >= 1 && Number(v) <= 65535;

/** The body the Agent's setup planner already accepts (same contract as the classic wizard). */
function planBody(f: Form) {
  const b: Record<string, unknown> = {
    engine: f.engine, direction: f.direction, role: f.role, protocol_version: f.engine === 'baft' ? 1 : 2,
    node_id: f.node.trim(), peer_id: f.peerId.trim(), tunnel_id: f.tunnel.trim(),
    service_port: Number(f.port), tunnel_port: Number(f.tport), dir: f.dir.trim(),
  };
  if (f.inst.trim()) b.instance = f.inst.trim();
  if (f.role === 'forward') {
    b.peer_host = f.peerHost.trim();
    if (x509(f.engine)) b.server_name = f.serverName.trim();
  }
  return b;
}

const RESULT_KIND: Record<string, Kind> = { pass: 'ok', warning: 'warn', blocked: 'bad', unknown: 'unknown' };
const ORDER: Record<string, number> = { blocked: 0, warning: 1, unknown: 2, pass: 3 };

/** Guided three-step wizard: Install node, Connect, Create tunnel. Preflight and plan are read-only calls. */
export function Setup() {
  const { t } = useApp();
  return (
    <div className="stack">
      <AutoPair />
      <details className="manual"><summary>{t('auto.manual')}</summary><Manual /></details>
    </div>
  );
}

function Manual() {
  const { t, call, hold } = useApp();
  const { data: d } = useData();
  const c = d ? conf(d) : {};
  const [step, setStep] = useState(0);
  const [f, setF] = useState<Form>({ ...BLANK, node: c.local_identity ?? '' });
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [pf, setPf] = useState<any>(null);
  const [plan, setPlan] = useState<any>(null);
  const [fail, setFail] = useState('');
  const [confirmed, setConfirmed] = useState(false);
  const [receipt, setReceipt] = useState<any>(null);
  const [applyKey] = useState(() => newKey('ui-'));
  const set = <K extends keyof Form>(k: K, v: Form[K]) => { setF((x) => ({ ...x, [k]: v })); setPf(null); setPlan(null); setFail(''); };

  const offered = d ? directionsOffered(d) : { direct: false, reverse: false };
  const engines: any[] = useMemo(() => ((d?.engines?.engines ?? []) as any[]).filter((e) => f.direction && (e.directions ?? []).includes(f.direction) && e.guided_setup?.available !== false), [d, f.direction]);
  if (!d?.status) return null;
  const s = d.status;

  const validate = (n: number): boolean => {
    const e: Record<string, string> = {};
    const req = (k: keyof Form) => { if (!String(f[k] ?? '').trim()) e[k] = t('rx.wiz.err.required'); };
    if (n === 1) {
      if (!f.loc) e.loc = t('rx.wiz.err.required');
      if (!f.direction) e.direction = t('rx.wiz.err.required');
      req('node'); req('peerId');
      if (f.role === 'forward') { req('peerHost'); if (x509(f.engine || 'tcp-tls')) req('serverName'); }
    }
    if (n === 2) {
      req('tunnel'); req('dir');
      if (!f.engine) e.engine = t('rx.wiz.err.required');
      if (!portOk(f.port)) e.port = t('rx.wiz.err.port');
      if (!portOk(f.tport)) e.tport = t('rx.wiz.err.port');
    }
    setErrors(e);
    return Object.keys(e).length === 0;
  };
  const go = (n: number) => { if (n > step && !validate(step)) return; setErrors({}); setStep(n); };

  const post = async (path: string): Promise<any | null> => {
    setBusy(true); hold(true); setFail('');
    try {
      const r = await call('POST', path, planBody(f));
      if (r.status !== 200) { setFail(r.body?.detail ?? r.body?.error ?? t('fail')); return null; }
      return r.body;
    } catch (e) {
      setFail(e instanceof Error ? e.message : t('fail'));
      return null;
    } finally { setBusy(false); hold(false); }
  };

  const steps: [string, string][] = [['rx.wiz.n1', 'rx.wiz.n1.d'], ['rx.wiz.n2', 'rx.wiz.n2.d'], ['rx.wiz.n3', 'rx.wiz.n3.d']];
  const txt = (k: keyof Form, label: string, o?: { ltr?: boolean; hint?: string; type?: string }) => (
    <Field label={label} error={errors[k]} hint={o?.hint}>
      {(p) => <input {...p} type={o?.type ?? 'text'} value={String(f[k] ?? '')} dir={o?.ltr === false ? undefined : 'ltr'} inputMode={o?.type === 'number' ? 'numeric' : undefined} autoComplete="off" onChange={(e) => set(k, e.target.value as never)} />}
    </Field>
  );

  // The only server this Agent can name is its own configured peer; there is no fleet inventory API yet.
  const knownPeers = [c.peer_identity].filter((x): x is string => !!x);
  const peerField = () => {
    if (knownPeers.length === 0) return <>{txt('peerId', t('f.peerid'), { hint: t('rx.peer.none') })}</>;
    const manual = f.peerManual || (!!f.peerId && !knownPeers.includes(f.peerId));
    return (
      <>
        <Field label={t('rx.peer.pick')} error={manual ? undefined : errors.peerId}>
          {(p) => (
            <select {...p} dir="ltr" value={manual ? '__manual' : f.peerId} onChange={(e) => { const v = e.target.value; setF((x) => ({ ...x, peerManual: v === '__manual', peerId: v === '__manual' ? '' : v })); setPf(null); setPlan(null); }}>
              <option value="">—</option>
              {knownPeers.map((k) => <option key={k} value={k}>{k} · {t('rx.peer.known')}</option>)}
              <option value="__manual">{t('rx.peer.manual')}</option>
            </select>
          )}
        </Field>
        {manual && txt('peerId', t('f.peerid'))}
      </>
    );
  };

  const sorted = pf ? [...(pf.checks as any[])].sort((a, b) => (ORDER[a.result] ?? 2) - (ORDER[b.result] ?? 2)) : [];
  const unknown: string[] = pf ? sorted.filter((x) => x.result === 'unknown').map((x) => x.id) : [];

  return (
    <div className="stack">
      <p className="muted">{t('setup.intro')}</p>
      <ol className="stepper" aria-label={t('setup.title')}>
        {steps.map(([k], i) => (
          <li key={k} className={i < step ? 'done' : ''} aria-current={i === step ? 'step' : undefined}>
            <span className="n" aria-hidden="true">{i < step ? '✓' : i + 1}</span><span className="l">{t(k)}</span>
          </li>
        ))}
      </ol>
      <Card title={t(steps[step]![0])}>
        <p className="sub">{t(steps[step]![1])}</p>

        {step === 0 && (
          <KV rows={[
            [t('rx.wiz.installed'), <Ltr key="v">{'v' + s.version}</Ltr>],
            [t('f.engine'), s.engine ? <Ltr key="e">{s.engine}</Ltr> : '—'],
            [t('rx.wiz.mode'), c.mode ? t((c.direction === 'reverse' ? 'role.rev.' : 'role.') + c.mode) : t('rx.wiz.none')],
            [t('f.node'), c.local_identity ? <Ltr key="n">{c.local_identity}</Ltr> : t('rx.wiz.none')],
          ]} />
        )}

        {step === 1 && (
          <div className="form">
            <fieldset className="choices">
              <legend>{t('rx.loc.q')}</legend>
              {(['iran', 'abroad'] as const).map((k) => (
                <label key={k} className={`choice ${f.loc === k ? 'on' : ''}`}>
                  <input type="radio" name="loc" value={k} checked={f.loc === k} onChange={() => { setF((x) => ({ ...x, loc: k, role: k === 'iran' ? 'forward' : 'serve' })); setPf(null); setPlan(null); }} />
                  <span className="choice-t">{t('rx.loc.' + k)}</span><span className="sub">{t('rx.loc.' + k + '.d')}</span>
                </label>
              ))}
              {errors.loc && <p className="fielderr" role="alert">{errors.loc}</p>}
            </fieldset>
            {f.loc && <>
            <fieldset className="choices">
              <legend>{t('dir.q')}</legend>
              {(['direct', 'reverse'] as Dir[]).map((k) => {
                const avail = offered[k];
                // Reverse is only selectable when the Agent itself lists it; otherwise it is shown as not available.
                return (
                  <label key={k} className={`choice ${f.direction === k ? 'on' : ''} ${avail ? '' : 'off'}`}>
                    <input type="radio" name="direction" value={k} checked={f.direction === k} disabled={!avail}
                      onChange={() => { set('direction', k); setF((x) => ({ ...x, direction: k, engine: '' })); }} />
                    <span className="choice-t">{t('dir.' + k + '.t')} {k === 'reverse' && avail && <Pill kind="warn">{t('dir.preview')}</Pill>}{!avail && <Pill kind="unknown">{t('dir.na.pill')}</Pill>}</span>
                    <span className="sub">{avail ? t('dir.' + k + '.d') : t('dir.na.d')}</span>
                  </label>
                );
              })}
              {errors.direction && <p className="fielderr" role="alert">{errors.direction}</p>}
            </fieldset>
            {!offered.reverse && <p className="sub">{t('rx.reverse.hidden')}</p>}
            <div className="role-auto">
              <p className="sub">{t('rx.role.auto', { r: f.role === 'forward' ? t(f.direction === 'reverse' ? 'srv.rev.forward' : 'srv.forward') : t(f.direction === 'reverse' ? 'srv.rev.serve' : 'srv.serve') })}</p>
              <details><summary>{t('rx.role.change')}</summary>
            <fieldset className="choices">
              <legend>{t('srv.q')}</legend>
              {(['forward', 'serve'] as const).map((k) => {
                const rev = f.direction === 'reverse';
                const title = k === 'forward' ? t(rev ? 'srv.rev.forward' : 'srv.forward') : t(rev ? 'srv.rev.serve' : 'srv.serve');
                const desc = k === 'forward' ? t(rev ? 'srv.rev.forward.d' : 'srv.opens') : rev ? t('srv.rev.serve.d') : '';
                return (
                  <label key={k} className={`choice ${f.role === k ? 'on' : ''}`}>
                    <input type="radio" name="role" value={k} checked={f.role === k} onChange={() => set('role', k)} />
                    <span className="choice-t">{title}</span><span className="sub">{desc}</span>
                  </label>
                );
              })}
            </fieldset>
              </details>
            </div>
            <div className="form-grid">
              {txt('node', t('f.node'))}
              {peerField()}
              {f.role === 'forward' && txt('peerHost', t('f.peerhost'))}
              {f.role === 'forward' && x509(f.engine || 'tcp-tls') && txt('serverName', t('f.servername'))}
            </div>
            </>}
          </div>
        )}

        {step === 2 && (
          <div className="form">
            <fieldset className="choices">
              <legend>{t('meth.q')}</legend>
              {engines.length === 0 && <p className="sub">{t('empty.history') && t('cap.unsupported')}</p>}
              {engines.map((e) => (
                <label key={e.id} className={`choice ${f.engine === e.id ? 'on' : ''}`}>
                  <input type="radio" name="engine" value={e.id} checked={f.engine === e.id} onChange={() => set('engine', e.id)} />
                  <span className="choice-t"><Ltr>{e.id}</Ltr> <Pill kind={e.status === 'stable' && f.direction !== 'reverse' ? 'ok' : 'warn'}>{e.status === 'stable' && f.direction !== 'reverse' ? t('meth.stable') : t('dir.preview')}</Pill></span>
                </label>
              ))}
              {errors.engine && <p className="fielderr" role="alert">{errors.engine}</p>}
            </fieldset>
            <div className="form-grid">
              {txt('tunnel', t('f.tunnel'))}
              {txt('port', t(f.direction === 'reverse' && f.role === 'serve' ? 'f.port.ingress' : 'f.port'), { type: 'number' })}
            </div>
            <details>
              <summary>{t('adv')}</summary>
              <div className="form-grid">{txt('tport', t('f.tport'), { type: 'number' })}{txt('dir', t('f.dir'))}{txt('inst', t('f.inst'))}</div>
            </details>
            <div className="row">
              <button type="button" className="primary" disabled={busy} onClick={async () => { if (!validate(2)) return; setPlan(null); const r = await post('/v1/setup/preflight'); if (r) setPf(r); }}>{t('rx.wiz.check')}</button>
              <button type="button" disabled={busy || !pf} onClick={async () => { const r = await post('/v1/setup/plan'); if (r) setPlan(r); }}>{t('rx.wiz.plan')}</button>
            </div>
            <div aria-live="polite">
              {busy && <p>{t('pv.load')}</p>}
              {fail && <p className="fielderr" role="alert">{fail}</p>}
            </div>
            {pf && (
              <ul className="list pf">
                {sorted.map((x) => {
                  let ev = x.code ? t(x.code, x.params ?? {}) : x.evidence;
                  if (!x.code || ev === x.code) ev = x.evidence;
                  return <li key={x.id}><Pill kind={RESULT_KIND[x.result] ?? 'unknown'}>{x.result}</Pill> <Ltr>{x.id}</Ltr><div className="sub">{ev}</div></li>;
                })}
              </ul>
            )}
            {plan && (
              <div className="plan">
                <Banner kind="info"><p>{t('pv.none')}</p></Banner>
                <h3>{t('pv.files')}</h3>
                <pre dir="ltr">{(plan.files as any[]).map((x) => `${x.path} [${x.mode}] ${x.note}`).join('\n')}</pre>
                <h3>{t('pv.pending')}</h3>
                <pre dir="ltr">{(plan.pending as string[]).join('\n')}</pre>
                <h3>{t('pv.cfg')}</h3>
                <pre dir="ltr">{JSON.stringify(plan.config, null, 2)}</pre>
                <h3>{t('pv.unknowns')}</h3>
                {unknown.length ? <ul>{unknown.map((u) => <li key={u}><Ltr>{u}</Ltr></li>)}</ul> : <p className="muted">{t('pv.nounknown')}</p>}
                <h3>{t('pv.rollback')}</h3><p>{t('pv.rollbackbody')}</p>
                <h3>{t('pv.secrets')}</h3><p>{t('pv.secretsbody')}</p>
                <label className="inline"><input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} /> {t('ap.confirm')}</label>
                <button type="button" className="primary" disabled={!confirmed || busy || pf?.overall === 'blocked' || receipt?.result === 'applied'} onClick={async () => {
                  setBusy(true); hold(true); setFail('');
                  try {
                    const r = await call('POST', '/v1/setup/apply', { stage: 'init', confirm: true, idempotency_key: applyKey, plan: planBody(f) });
                    if (r.body?.result) setReceipt(r.body); else setFail(r.body?.detail ?? r.body?.error ?? t('fail'));
                  } catch (e) { setFail(e instanceof Error ? e.message : t('fail')); } finally { setBusy(false); hold(false); }
                }}>{t('ap.go')}</button>
                {receipt && (
                  <div className="stack" aria-live="polite">
                    <h3>{t('rc.title')}</h3>
                    <p><Pill kind={receipt.result === 'applied' ? 'ok' : 'warn'}>{t('rc.' + receipt.result)}</Pill> <Ltr>{receipt.id ?? ''}</Ltr></p>
                    {receipt.reason && <p>{receipt.reason}</p>}
                    <h3>{t('rc.verify')}</h3>
                    <ul>{((receipt.verification ?? []) as any[]).map((v) => <li key={v.id}><Pill kind={v.result === 'pass' ? 'ok' : 'warn'}>{v.result}</Pill> <Ltr>{v.id}</Ltr> <span className="muted">· {v.evidence}</span></li>)}</ul>
                    <h3>{t('rc.changes')}</h3>
                    <pre dir="ltr">{((receipt.changes ?? []) as any[]).map((c) => `${c.action} ${c.path} [${c.mode}]`).join('\n') || '—'}</pre>
                    {receipt.rollback?.performed && <Banner kind="bad"><p>{t('rc.rolled')} {(receipt.rollback.removed ?? []).join(', ')}</p></Banner>}
                    {receipt.next_commands && <><h3>{t('rc.next')}</h3><pre dir="ltr">{(receipt.next_commands as string[]).join('\n')}</pre></>}
                    <p className="sub">{t('rx.wiz.apply.note')} <a href="/#/setup">{t('rx.wiz.apply')}</a></p>
                  </div>
                )}
                {fail && <p className="fielderr" role="alert">{fail}</p>}
              </div>
            )}
          </div>
        )}

        <div className="row wiz-nav">
          <button type="button" disabled={step === 0} onClick={() => go(step - 1)}>{t('rx.wiz.back')}</button>
          {step < 2 && <button type="button" className="primary" onClick={() => go(step + 1)}>{t('rx.wiz.next')}</button>}
        </div>
      </Card>
    </div>
  );
}
