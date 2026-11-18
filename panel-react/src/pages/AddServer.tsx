import { useCallback, useEffect, useState } from 'react';
import { useApp } from '../app';
import { Banner, CopyButton, Field, Ltr, Pill } from '../components/ui';
import type { Kind } from '../derive';

interface Offer { id: string; node_id: string; state: string; fingerprint?: string | null; expires?: number }
interface Secrets { node_id: string; token: string; nonce: string; expires?: number }
const KIND: Record<string, Kind> = { 'waiting-for-node': 'info', 'needs-confirmation': 'warn', enrolled: 'ok', closed: 'unknown' };

/** Add server, Path A (ADR-0051): one-time offer, the operator types the fingerprint the new node prints. Secrets are shown once. */
export function AddServer() {
  const { t, call, toast } = useApp();
  const [nodeId, setNodeId] = useState('');
  const [offers, setOffers] = useState<Offer[] | null>(null);
  const [secrets, setSecrets] = useState<Secrets | null>(null);
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const r = await call('GET', '/v1/fleet/enroll');
      setOffers(r.status === 200 && Array.isArray(r.body?.offers) ? r.body.offers : r.status === 404 ? null : []);
    } catch { /* keep the last list */ }
  }, [call]);
  useEffect(() => {
    void refresh();
    const h = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(h);
  }, [refresh]);

  const why = (r: { body?: any; status: number }): string => (r.body?.error ? String(r.body.error) : t('fail'));
  const create = async () => {
    setErr('');
    if (!/^[a-z0-9][a-z0-9-]{0,62}$/.test(nodeId)) { setErr(t('fleet.add.badid')); return; }
    setBusy(true);
    try {
      const r = await call('POST', '/v1/fleet/enroll/offer', { node_id: nodeId });
      if (r.status === 200) { setSecrets(r.body); setNodeId(''); void refresh(); } else setErr(why(r));
    } catch { setErr(t('fail')); }
    setBusy(false);
  };
  const confirm = async (o: Offer) => {
    setBusy(true);
    try {
      const r = await call('POST', '/v1/fleet/enroll/confirm', { id: o.id, fingerprint: (typed[o.id] ?? '').trim() });
      if (r.status === 200) toast(t('fleet.add.done'), 'ok');
      else toast(r.status === 503 ? t('fleet.add.nosigner') : why(r), 'bad');
    } catch { toast(t('fail'), 'bad'); }
    setBusy(false);
    void refresh();
  };
  const deny = async (o: Offer) => {
    try { await call('POST', '/v1/fleet/enroll/deny', { id: o.id }); } catch { toast(t('fail'), 'bad'); }
    void refresh();
  };

  if (offers === null && !secrets) return <p className="muted">{t('fleet.add.unavail')}</p>;
  return (
    <div className="stack">
      {err && <Banner kind="warn">{err}</Banner>}
      <Field label={t('fleet.add.nodeid')} hint={t('fleet.add.nodeid.hint')}>
        {(p) => <input {...p} dir="ltr" value={nodeId} autoComplete="off" onChange={(e) => setNodeId(e.target.value.toLowerCase())} />}
      </Field>
      <button type="button" className="primary" disabled={busy || !nodeId} onClick={() => void create()}>{t('fleet.add.create')}</button>
      {secrets && (
        <Banner kind="info" title={t('fleet.add.secrets', { id: secrets.node_id })}
          action={<button type="button" className="ghost" onClick={() => setSecrets(null)}>{t('fleet.add.hide')}</button>}>
          <p>{t('fleet.add.once')}</p>
          <div className="row"><Ltr><code>{secrets.token}</code></Ltr><CopyButton text={secrets.token} label={t('fleet.add.copy.token')} /></div>
          <div className="row"><Ltr><code>{secrets.nonce}</code></Ltr><CopyButton text={secrets.nonce} label={t('fleet.add.copy.nonce')} /></div>
        </Banner>
      )}
      {offers && offers.length > 0 && (
        <ul className="list">
          {offers.map((o) => (
            <li key={o.id}>
              <div className="row"><Ltr>{o.node_id}</Ltr> <Pill kind={KIND[o.state] ?? 'unknown'}>{t('fleet.enr.' + (KIND[o.state] ? o.state : 'closed'))}</Pill></div>
              {o.state === 'waiting-for-node' && <p className="muted">{t('fleet.enr.waiting')}</p>}
              {o.state === 'needs-confirmation' && (
                <>
                  <p className="muted">{t('fleet.enr.type')}</p>
                  <Field label={t('fleet.enr.fp')}>
                    {(p) => <input {...p} dir="ltr" value={typed[o.id] ?? ''} autoComplete="off" onChange={(e) => setTyped({ ...typed, [o.id]: e.target.value })} />}
                  </Field>
                  <button type="button" className="primary" disabled={busy || !(typed[o.id] ?? '').trim()} onClick={() => void confirm(o)}>{t('fleet.enr.confirm')}</button>
                </>
              )}
              {(o.state === 'waiting-for-node' || o.state === 'needs-confirmation') && (
                <button type="button" className="ghost" onClick={() => void deny(o)}>{t('fleet.enr.deny')}</button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
