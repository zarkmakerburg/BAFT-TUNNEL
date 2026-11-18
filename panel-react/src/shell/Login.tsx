import { useState } from 'react';
import { useApp } from '../app';
import { LANGS } from '../i18n';
import { Icon, Mark } from '../components/icons';

export function LangButton() {
  const { lang, setLang, t } = useApp();
  const other = lang === 'fa' ? 'en' : 'fa';
  return (
    <button type="button" className="ghost lang-btn" lang={other} aria-label={t('ui.lang.switch', { l: LANGS[other].label })} onClick={() => setLang(other)}>
      <Icon name="globe" /><span>{LANGS[other].label}</span>
    </button>
  );
}

export function Login() {
  const { t, signIn } = useApp();
  const [value, setValue] = useState('');
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);
  const go = async () => {
    const tok = value.trim();
    if (!tok) { setErr(t('ui.login.empty')); return; }
    setBusy(true);
    setErr('');
    const r = await signIn(tok);
    setBusy(false);
    if (r !== 'ok') setErr(r === 'bad' ? t('badtoken') : r === 'offline' ? t('ui.login.offline') : t('fail'));
  };
  return (
    <main className="login">
      <form className="card login-card" onSubmit={(e) => { e.preventDefault(); void go(); }}>
        <div className="brand"><Mark /><span className="brand-name">{t('app')}</span></div>
        <p className="sub">{t('ui.login.lead')}</p>
        <label className="field" htmlFor="token"><span>{t('token')}</span></label>
        <input id="token" type="password" autoComplete="off" dir="ltr" autoFocus value={value} onChange={(e) => setValue(e.target.value)} aria-describedby="token-hint" />
        <p className="sub" id="token-hint">{t('ui.login.hint')}</p>
        <button className="primary" type="submit" disabled={busy}>{t('connect')}</button>
        {err && <p className="fielderr" role="alert">{err}</p>}
        <div className="login-foot"><LangButton /></div>
      </form>
    </main>
  );
}
