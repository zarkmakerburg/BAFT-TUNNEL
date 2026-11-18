import { useState } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { LANGS, type Lang } from '../i18n';
import { Card, Confirm, KV, Ltr, Segmented } from '../components/ui';
import { Icon } from '../components/icons';

/** Panel preferences only; nothing here changes the node. Signing out is the one confirmed action. */
export function Settings() {
  const { t, lang, setLang, theme, setTheme, nav, setNav, signOut, call, toast } = useApp();
  const { data: d } = useData();
  const [ask, setAsk] = useState(false);
  const s = d?.status ?? {};
  const support = async () => {
    try {
      const r = await call('GET', '/v1/support');
      if (r.status !== 200) { toast(t('support.failed'), 'bad'); return; }
      const url = URL.createObjectURL(new Blob([JSON.stringify(r.body, null, 2)], { type: 'application/json' }));
      const a = document.createElement('a');
      a.href = url; a.download = 'baft-tunnel-support.json';
      document.body.appendChild(a); a.click(); a.remove();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch { toast(t('support.failed'), 'bad'); }
  };
  return (
    <div className="grid cols-2">
      <Card title={t('settings.title')}>
        <div className="field"><span className="sub">{t('settings.language')}</span>
          <Segmented<Lang> value={lang} onChange={setLang} label={t('settings.language')} options={(Object.keys(LANGS) as Lang[]).map((k) => [k, LANGS[k].label])} /></div>
        <div className="field"><span className="sub">{t('settings.theme')}</span>
          <Segmented value={theme} onChange={setTheme} label={t('settings.theme')} options={[['dark', t('theme.dark')], ['light', t('theme.light')], ['auto', t('theme.auto')]]} /></div>
        <div className="field"><span className="sub">{t('settings.nav')}</span>
          <Segmented value={nav} onChange={setNav} label={t('settings.nav')} options={[['full', t('settings.nav.full')], ['rail', t('settings.nav.rail')]]} /></div>
        <p className="sub">{t('settings.later')}</p>
      </Card>
      <Card title={t('settings.about')}>
        <KV rows={[[t('settings.version'), s.version ? <Ltr key="v">{'v' + s.version}</Ltr> : null]]} />
        <h3>{t('settings.session')}</h3>
        <p className="sub">{t('settings.sessionhint')}</p>
        <button type="button" onClick={() => setAsk(true)}><Icon name="logout" />{t('logout')}</button>
      </Card>
      <Card title={t('support.title')}>
        <p className="sub">{t('support.note')}</p>
        <button type="button" onClick={() => void support()}>{t('support.download')}</button>
      </Card>
      <Card title={t('rx.classic')}>
        <p className="sub">{t('rx.classic.note')}</p>
        <a className="btn" href="/">{t('rx.classic')}</a>
      </Card>
      <Confirm open={ask} title={t('rx.settings.sensitive')} body={t('rx.settings.logout.body')} okLabel={t('logout')} danger onResult={(ok) => { setAsk(false); if (ok) signOut(); }} />
    </div>
  );
}
