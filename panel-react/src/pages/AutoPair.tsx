import { useEffect, useRef, useState } from 'react';
import { useApp } from '../app';
import { newKey } from '../api';
import { Banner, Card, CopyButton, Field } from '../components/ui';

type Phase = 'form' | 'running' | 'starting' | 'started' | 'startfail';
type Loc = 'iran' | 'abroad';

/** Automatic pairing (ADR-0050): Iran/abroad first, then one one-time code. No file is moved by hand. */
export function AutoPair() {
  const { t, call, hold } = useApp();
  const [loc, setLoc] = useState<Loc | null>(null);
  const [name, setName] = useState('');
  const [port, setPort] = useState('');
  const [host, setHost] = useState('');
  const [code, setCode] = useState('');
  const [phase, setPhase] = useState<Phase>('form');
  const [ps, setPs] = useState<any>(null);
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);
  const t0 = useRef(0);
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; hold(false); }, [hold]);

  const plainFail = (r: string): string => {
    if (/cannot reach the other server/.test(r)) return t('auto.fail.reach');
    if (/expected pairing certificate/.test(r)) return t('auto.fail.pin');
    if (/refused|expired|not approved/.test(r)) return t('auto.fail.refused');
    if (/cannot listen/.test(r)) return t('auto.fail.port');
    return `${t('auto.fail.other')} ${r}`;
  };
  const errOf = (r: any): string => r?.body?.detail ?? r?.body?.error ?? t('fail');

  async function afterDone() {
    hold(false); setPhase('starting');
    try {
      const r = await call('POST', '/v1/tunnel/lifecycle', { action: 'start', confirm: true, idempotency_key: newKey('ui-auto-') });
      if (alive.current) setPhase(r.status === 200 ? 'started' : 'startfail');
    } catch { if (alive.current) setPhase('startfail'); }
  }

  async function poll() {
    hold(true);
    while (alive.current) {
      try {
        const r = await call('GET', '/v1/pair/status');
        if (r.status === 200 && r.body) {
          setPs(r.body);
          const st = r.body.state;
          if (st === 'idle' && Date.now() - t0.current > 20000) { hold(false); setPhase('form'); setErr(t('auto.fail.nohelper')); return; }
          if (st === 'failed') { hold(false); setPhase('form'); setErr(plainFail(String(r.body.reason ?? ''))); return; }
          if (st === 'done') { await afterDone(); return; }
        } else {
          // The agent restarts with the new configuration once pairing has written it.
          const q = await call('GET', '/v1/status');
          if (q.status === 200 && q.body && !q.body.setup_pending) { await afterDone(); return; }
        }
      } catch { /* the agent is restarting; keep polling */ }
      await new Promise((res) => window.setTimeout(res, 2000));
    }
  }

  async function submit(path: string, body: unknown) {
    setBusy(true); setErr(''); t0.current = Date.now();
    try {
      const r = await call('POST', path, body);
      if (r.status === 202) { setPhase('running'); setPs({ state: 'starting' }); void poll(); } else setErr(errOf(r));
    } catch (e) { setErr(e instanceof Error ? e.message : t('fail')); } finally { setBusy(false); }
  }

  const create = () => {
    const p = Number(port);
    if (!/^[A-Za-z0-9 -]{1,32}$/.test(name.trim()) || !(p >= 1024 && p <= 65535)) { setErr(t('auto.err.input')); return; }
    void submit('/v1/pair/offer', { name: name.trim(), service_port: p, confirm: true, ...(host.trim() ? { host: host.trim() } : {}) });
  };
  const connect = () => {
    const c = code.trim().replace(/^sudo baft-tunnel pair accept\s+/, '');
    if (!c.startsWith('BAFT1-')) { setErr(t('auto.err.code')); return; }
    void submit('/v1/pair/accept', { code: c, confirm: true });
  };
  const decide = (approve: boolean) => { void call('POST', '/v1/pair/approve', { approve, nonce: ps?.nonce }).then((r) => { if (r.status >= 300) setErr(t('fail')); }, () => setErr(t('fail'))); };
  const cmd = (c: string) => `sudo baft-tunnel pair accept ${c}`;

  return (
    <Card title={t('auto.title')}>
      <p className="sub">{t('auto.intro')}</p>
      {phase === 'form' && (
        <div className="form">
          <fieldset className="choices">
            <legend>{t('rx.loc.q')}</legend>
            {(['abroad', 'iran'] as Loc[]).map((k) => (
              <label key={k} className={`choice ${loc === k ? 'on' : ''}`}>
                <input type="radio" name="autoloc" value={k} checked={loc === k} onChange={() => { setLoc(k); setErr(''); }} />
                <span className="choice-t">{k === 'abroad' ? t('auto.role.first') : t('auto.role.second')}</span>
                <span className="sub">{k === 'abroad' ? t('auto.role.first.d') : t('auto.role.second.d')}</span>
              </label>
            ))}
          </fieldset>
          {loc === 'abroad' && (
            <>
              <div className="form-grid">
                <Field label={t('auto.f.name')}>{(p) => <input {...p} dir="ltr" value={name} autoComplete="off" onChange={(e) => setName(e.target.value)} />}</Field>
                <Field label={t('auto.f.port')}>{(p) => <input {...p} dir="ltr" inputMode="numeric" value={port} autoComplete="off" onChange={(e) => setPort(e.target.value)} />}</Field>
              </div>
              <details><summary>{t('adv')}</summary>
                <Field label={t('auto.f.host')}>{(p) => <input {...p} dir="ltr" value={host} autoComplete="off" onChange={(e) => setHost(e.target.value)} />}</Field>
              </details>
              <button type="button" className="primary" disabled={busy} onClick={create}>{t('auto.create')}</button>
            </>
          )}
          {loc === 'iran' && (
            <>
              <Field label={t('auto.f.code')}>{(p) => <textarea {...p} rows={4} dir="ltr" placeholder="BAFT1-..." value={code} onChange={(e) => setCode(e.target.value)} />}</Field>
              <button type="button" className="primary" disabled={busy} onClick={connect}>{t('auto.connect')}</button>
            </>
          )}
          {err && <p className="fielderr" role="alert">{err}</p>}
        </div>
      )}
      {phase === 'running' && (() => {
        const p = ps ?? {};
        if (p.state === 'waiting' && p.code) return (
          <div className="stack">
            <Banner kind="info" title={t('auto.wait.t')}><p>{t('auto.wait.b')}</p></Banner>
            <p className="sub">{t('auto.cmd')}</p>
            <pre dir="ltr">{cmd(p.code)}</pre>
            <div className="row"><CopyButton text={cmd(p.code)} label={t('auto.copy.cmd')} /><CopyButton text={p.code} label={t('auto.copy.code')} /></div>
            <p className="sub">{t('auto.fw', { port: String(p.port ?? 8443) })}</p>
          </div>
        );
        if (p.state === 'approval' && p.peer) return (
          <div className="stack">
            <Banner kind="warn" title={t('auto.appr.t')}><p>{t('auto.appr.b', { node: p.peer.node_id, addr: p.peer.address })}</p>
              <p>{t('auto.verify')} <strong dir="ltr">{p.verify ?? ''}</strong></p><p className="sub">{t('auto.verify.hint')}</p></Banner>
            <div className="row"><button type="button" className="primary" onClick={() => decide(true)}>{t('auto.appr.yes')}</button><button type="button" onClick={() => decide(false)}>{t('auto.appr.no')}</button></div>
          </div>
        );
        return (
          <div className="stack">
            <p role="status">{t('auto.working')}</p>
            {p.verify && <><p>{t('auto.verify')} <strong dir="ltr">{p.verify}</strong></p><p className="sub">{t('auto.verify.hint2')}</p></>}
          </div>
        );
      })()}
      {phase === 'starting' && <p role="status">{t('auto.starting')}</p>}
      {(phase === 'started' || phase === 'startfail') && (
        <div className="stack">
          <Banner kind={phase === 'started' ? 'info' : 'warn'} title={t('auto.done')}><p>{t(phase === 'started' ? 'auto.started' : 'auto.startfail')}</p></Banner>
          <a className="btn" href="#/tunnels">{t('auto.goto')}</a>
        </div>
      )}
    </Card>
  );
}
