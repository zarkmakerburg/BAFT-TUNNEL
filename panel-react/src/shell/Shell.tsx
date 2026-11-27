import { Component, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useApp, useFeed } from '../app';
import { FeedCtx } from '../feed-context';
import { GROUPS, ROUTES, routeFromHash, type Route } from '../routes';
import { conf, fmtNum, ago, nodeState, warnings } from '../derive';
import { Banner, Dialog, Ltr, Pill, Skeleton, Toasts } from '../components/ui';
import { Icon, Mark } from '../components/icons';
import { LangButton } from './Login';
import { Overview } from '../pages/Overview';
import { Tunnels } from '../pages/Tunnels';
import { Servers } from '../pages/Servers';
import { Monitoring } from '../pages/Monitoring';
import { Setup } from '../pages/Setup';
import { Diagnostics } from '../pages/Diagnostics';
import { Activity } from '../pages/Activity';
import { AgentPage } from '../pages/Agent';
import { Fleet } from '../pages/Fleet';
import { Settings } from '../pages/Settings';
import { NotificationList } from '../pages/parts';

const PAGES: Record<Route['id'], () => ReactNode> = {
  overview: () => <Overview />, tunnels: () => <Tunnels />, servers: () => <Servers />, fleet: () => <Fleet />, monitoring: () => <Monitoring />,
  setup: () => <Setup />, diagnostics: () => <Diagnostics />, activity: () => <Activity />, agent: () => <AgentPage />, settings: () => <Settings />,
};

class Boundary extends Component<{ label: string; retry: string; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() {
    if (this.state.failed) {
      return <Banner kind="bad"><p>{this.props.label}</p><button type="button" onClick={() => this.setState({ failed: false })}>{this.props.retry}</button></Banner>;
    }
    return this.props.children;
  }
}

function useHash(): string {
  const [h, setH] = useState(location.hash);
  useEffect(() => {
    const on = () => { setH(location.hash); window.scrollTo(0, 0); document.getElementById('main')?.focus({ preventScroll: true }); };
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  return h;
}

export function Shell() {
  const { t, lang, theme, setTheme, nav, setNav, signOut } = useApp();
  const route = routeFromHash(useHash());
  const feed = useFeed(route.needs);
  const [overlay, setOverlay] = useState<null | 'notif' | 'palette' | 'drawer'>(null);
  const [q, setQ] = useState('');
  const d = feed.data;
  const s = d?.status ?? null;
  const setup = !!s?.setup_pending;
  const ws = useMemo(() => (d && s && !setup ? warnings(d, lang, t) : []), [d, s, setup, lang, t]);
  const n = ws.filter((w) => w.sev !== 'info').length;
  const [, tick] = useState(0);
  useEffect(() => { const id = window.setInterval(() => tick((x) => x + 1), 1000); return () => window.clearInterval(id); }, []);
  const age = feed.fetchedAt ? Date.now() / 1000 - feed.fetchedAt : 0;
  const stale = age > 15;
  const c = d ? conf(d) : {};
  const st = nodeState(s, t);

  useEffect(() => {
    document.title = `${t('nav.' + route.id)} - BAFT-TUNNEL`;
  }, [route.id, t]);
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      if (e.code === 'KeyK') { e.preventDefault(); setQ(''); setOverlay('palette'); }
      else if (e.code === 'KeyB') { e.preventDefault(); setNav(nav === 'rail' ? 'full' : 'rail'); }
    };
    window.addEventListener('keydown', on);
    return () => window.removeEventListener('keydown', on);
  }, [nav, setNav]);

  const navTree = (close?: () => void) => GROUPS.map((g) => {
    const items = ROUTES.filter((r) => r.group === g);
    return (
      <div className="nav-group" key={g}>
        <div className="nav-h">{t('nav.g.' + g)}</div>
        <ul className="nav">
          {items.map((r) => (
            <li key={r.id}>
              <a href={`#/${r.id}`} aria-current={r.id === route.id ? 'page' : undefined} title={t('nav.' + r.id)} onClick={close}>
                <Icon name={r.icon} /><span className="nav-l">{t('nav.' + r.id)}</span>
              </a>
            </li>
          ))}
        </ul>
      </div>
    );
  });

  const cmdItems = ROUTES.filter((r) => t('nav.' + r.id).toLowerCase().includes(q.trim().toLowerCase()));
  const body = (): ReactNode => {
    if (feed.loading || !d) return d === null && !feed.loading ? unreachable() : <Skeleton />;
    if (!s) return unreachable();
    if (setup && route.id !== 'setup' && route.id !== 'settings') {
      return <Banner kind="info" title={t('setup.pending.title')}><p>{t('setup.pending.body', { inst: s.instance ?? '' })}</p><a className="btn primary" href="#/setup">{t('nav.setup')}</a></Banner>;
    }
    const bad = Object.entries(d.meta).filter(([k, v]) => v === 'error' && k !== 'status').map(([k]) => t('ui.src.' + k));
    return (
      <>
        {bad.length > 0 && !setup && (
          <Banner kind="partial" action={<button type="button" onClick={feed.refresh}><Icon name="refresh" />{t('ui.retry')}</button>}>
            <p><strong>{t('ui.partial.title')} </strong>{t('ui.partial.body', { l: bad.join(lang === 'fa' ? '، ' : ', ') })}</p>
          </Banner>
        )}
        {PAGES[route.id]()}
      </>
    );
  };
  function unreachable() {
    return (
      <Banner kind="bad" title={t('unreachable.title')} action={<button type="button" onClick={feed.refresh}><Icon name="refresh" />{t('ui.retry')}</button>}>
        <p>{t('unreachable.body')}</p><p className="sub">{t('unreachable.next')}</p>
      </Banner>
    );
  }

  return (
    <FeedCtx.Provider value={feed}>
      <a className="skip" href="#main" onClick={(e) => { e.preventDefault(); document.getElementById('main')?.focus(); }}>{t('skip')}</a>
      <div className={`shell ${route.id === 'overview' ? 'dashboard-shell' : ''}`} data-nav={nav}>
        <aside className="sidebar">
          <div className="brand"><Mark /><span className="brand-name">{t('app')}</span></div>
          <nav className="side-nav" aria-label={t('ui.nav.main')}>{navTree()}</nav>
          <div className="side-foot">
            <div className="side-node">
              <span className="side-id">{c.local_identity ? <Ltr>{c.local_identity}</Ltr> : s?.instance ? <Ltr>{s.instance}</Ltr> : t('unknown')}</span>
              <span className="side-ver">{s ? <Ltr>{'v' + s.version}</Ltr> : '—'}</span>
            </div>
            <button type="button" className="ghost icon-btn collapse" aria-pressed={nav === 'rail'} aria-label={t('ui.nav.collapse')} title={t('ui.nav.collapse')} onClick={() => setNav(nav === 'rail' ? 'full' : 'rail')}><Icon name="panel" className="flip" /></button>
          </div>
        </aside>
        <div className="main">
          <header className="topbar">
            <button type="button" className="ghost icon-btn menu-btn" aria-label={t('ui.nav.open')} aria-haspopup="dialog" onClick={() => setOverlay('drawer')}><Icon name="menu" /></button>
            <div className="heading"><span className="crumb">{t('nav.g.' + route.group)}</span><h1>{t('nav.' + route.id)}</h1></div>
            <span className={`fresh ${stale ? 'stale' : ''}`}>{feed.fetchedAt ? t('updated', { ago: ago(lang, t, feed.fetchedAt) }) : ''}</span>
            {stale && <Pill kind="warn">{t('stale')}</Pill>}
            <Pill kind={st.k}>{st.l}</Pill>
            <button type="button" className="ghost search-btn" aria-label={t('ui.cmd.open')} aria-haspopup="dialog" onClick={() => { setQ(''); setOverlay('palette'); }}>
              <Icon name="search" /><span className="search-l">{t('ui.cmd.open')}</span><kbd>Ctrl K</kbd>
            </button>
            <button type="button" className="ghost icon-btn bell" aria-haspopup="dialog" aria-label={n ? t('ui.notif.n', { n: fmtNum(lang, n, 0) }) : t('ui.notif')} onClick={() => setOverlay('notif')}>
              <Icon name="bell" />{n > 0 && <span className="count">{fmtNum(lang, n, 0)}</span>}
            </button>
            <button type="button" className="ghost icon-btn theme-btn" aria-label={t('settings.theme')} title={t('settings.theme')} onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}><Icon name={theme === 'light' ? 'moon' : 'sun'} /></button>
            <LangButton />
          </header>
          <main>
            <div className="content" id="main" tabIndex={-1} aria-busy={feed.loading}>
              <div className="view enter" key={route.id}>
                <Boundary label={t('ui.err.render')} retry={t('ui.retry')}>{body()}</Boundary>
              </div>
            </div>
          </main>
        </div>
        <nav className="tabbar" aria-label={t('ui.nav.tabs')}>
          {ROUTES.filter((r) => r.tab).map((r) => (
            <a key={r.id} className="tab" href={`#/${r.id}`} aria-current={r.id === route.id ? 'page' : undefined}><Icon name={r.icon} /><span className="nav-l">{t('nav.' + r.id)}</span></a>
          ))}
          <button type="button" className={`tab tab-more ${!route.tab ? 'on' : ''}`} aria-haspopup="dialog" onClick={() => setOverlay('drawer')}><Icon name="more" /><span className="nav-l">{t('nav.more')}</span></button>
        </nav>
      </div>

      <Dialog open={overlay === 'drawer'} onClose={() => setOverlay(null)} label={t('ui.nav.main')} kind="drawer">
        <div className="drawer-head"><div className="brand"><Mark /><span className="brand-name">{t('app')}</span></div>
          <button type="button" className="ghost icon-btn" aria-label={t('ui.close')} onClick={() => setOverlay(null)}><Icon name="close" /></button></div>
        <nav aria-label={t('ui.nav.main')}>{navTree(() => setOverlay(null))}</nav>
        <button type="button" className="ghost" onClick={signOut}><Icon name="logout" />{t('logout')}</button>
      </Dialog>
      <Dialog open={overlay === 'notif'} onClose={() => setOverlay(null)} label={t('ui.notif')} kind="popover">
        <div className="dlg">
          <div className="drawer-head"><h2>{t('ui.notif')}</h2><button type="button" className="ghost icon-btn" aria-label={t('ui.close')} onClick={() => setOverlay(null)}><Icon name="close" /></button></div>
          <NotificationList ws={ws} onNavigate={() => setOverlay(null)} />
          <p className="sub">{t('ui.notif.src')}</p>
        </div>
      </Dialog>
      <Dialog open={overlay === 'palette'} onClose={() => setOverlay(null)} label={t('ui.cmd.open')} kind="palette">
        <div className="dlg">
          <input type="search" autoFocus dir="auto" placeholder={t('ui.cmd.ph')} aria-label={t('ui.cmd.open')} value={q} onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && cmdItems[0]) { location.hash = `#/${cmdItems[0].id}`; setOverlay(null); } }} />
          {cmdItems.length ? (
            <ul className="cmd">{cmdItems.map((r) => <li key={r.id}><a href={`#/${r.id}`} onClick={() => setOverlay(null)}><Icon name={r.icon} />{t('nav.' + r.id)}</a></li>)}</ul>
          ) : <p className="sub">{t('ui.cmd.none')}</p>}
          <p className="sub">{t('ui.cmd.local')}</p>
        </div>
      </Dialog>
      <Toasts />
    </FeedCtx.Provider>
  );
}
