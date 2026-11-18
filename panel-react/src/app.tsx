import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { BASE, api, load, savedToken, saveToken, setUnauthorizedHandler, store, type EndpointKey, type Loaded } from './api';
import { LANGS, translate, type Lang, type Vars } from './i18n';
import type { TFn } from './derive';

export type Theme = 'dark' | 'light' | 'auto';
export type NavMode = 'full' | 'rail';

export interface Toast { id: number; text: string; kind: 'ok' | 'bad' | '' }

export interface AppCtx {
  token: string;
  lang: Lang;
  theme: Theme;
  nav: NavMode;
  t: TFn;
  setLang(l: Lang): void;
  setTheme(t: Theme): void;
  setNav(n: NavMode): void;
  signIn(token: string): Promise<'ok' | 'bad' | 'offline' | 'fail'>;
  signOut(): void;
  call(method: 'GET' | 'POST', path: string, body?: unknown): ReturnType<typeof api>;
  toast(text: string, kind?: 'ok' | 'bad'): void;
  toasts: Toast[];
  /** Marks an operation in flight so the 5 s refresh never replaces content under the operator. */
  hold(on: boolean): void;
}

const Ctx = createContext<AppCtx | null>(null);
export const useApp = (): AppCtx => {
  const c = useContext(Ctx);
  if (!c) throw new Error('AppProvider missing');
  return c;
};

const asLang = (v: string | null): Lang => (v === 'en' || v === 'fa' ? v : 'fa');
const asTheme = (v: string | null): Theme => (v === 'light' || v === 'auto' || v === 'dark' ? v : 'dark');

export function AppProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState(savedToken());
  const [lang, setLangState] = useState<Lang>(asLang(store('lang')));
  const [theme, setThemeState] = useState<Theme>(asTheme(store('theme')));
  const [nav, setNavState] = useState<NavMode>(store('nav') === 'rail' ? 'rail' : 'full');
  const [toasts, setToasts] = useState<Toast[]>([]);
  const seq = useRef(0);

  useEffect(() => {
    document.documentElement.lang = lang;
    document.documentElement.dir = LANGS[lang].dir;
    store('lang', lang);
  }, [lang]);
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    store('theme', theme);
  }, [theme]);
  useEffect(() => store('nav', nav) as unknown as void, [nav]);

  const signOut = useCallback(() => {
    saveToken(null);
    setToken('');
  }, []);
  useEffect(() => {
    setUnauthorizedHandler(signOut);
    return () => setUnauthorizedHandler(null);
  }, [signOut]);

  const t = useCallback<TFn>((key: string, vars?: Vars) => translate(lang, key, vars), [lang]);

  const toast = useCallback((text: string, kind: 'ok' | 'bad' | '' = '') => {
    const id = ++seq.current;
    setToasts((l) => [...l, { id, text, kind: kind as Toast['kind'] }]);
    window.setTimeout(() => setToasts((l) => l.filter((x) => x.id !== id)), 3500);
  }, []);

  const signIn = useCallback(async (candidate: string) => {
    try {
      const r = await api('GET', '/v1/status', candidate, undefined, true);
      if (r.status === 200) {
        saveToken(candidate);
        setToken(candidate);
        return 'ok' as const;
      }
      return r.status === 401 ? ('bad' as const) : ('fail' as const);
    } catch {
      return 'offline' as const;
    }
  }, []);

  const holds = useRef(0);
  const value = useMemo<AppCtx>(() => ({
    token, lang, theme, nav, t, toasts, toast, signOut, signIn,
    setLang: setLangState, setTheme: setThemeState, setNav: setNavState,
    call: (m, p, b) => api(m, p, token, b),
    hold: (on) => { holds.current += on ? 1 : -1; (window as any).__baftHold = holds.current; },
  }), [token, lang, theme, nav, t, toasts, toast, signOut, signIn]);

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export interface Feed {
  data: Loaded | null;
  fetchedAt: number;
  loading: boolean;
  refresh(): void;
}

/** Loads the endpoints a route needs, then refreshes every 5 s unless an operation is in flight or the tab is hidden. */
export function useFeed(needs: EndpointKey[]): Feed {
  const { token } = useApp();
  const [data, setData] = useState<Loaded | null>(null);
  const [fetchedAt, setFetchedAt] = useState(0);
  const [loading, setLoading] = useState(true);
  const key = needs.join(',');
  const gen = useRef(0);

  const run = useCallback(async (force: boolean) => {
    if (!token) return;
    if (!force && ((window as any).__baftHold > 0 || document.visibilityState !== 'visible')) return;
    const mine = ++gen.current;
    const list = [...BASE, ...(key ? (key.split(',') as EndpointKey[]) : [])].filter((k, i, a) => a.indexOf(k) === i);
    const d = await load(token, list);
    if (mine !== gen.current) return;
    setData(d);
    if (d.status) setFetchedAt(Math.floor(Date.now() / 1000));
    setLoading(false);
  }, [token, key]);

  useEffect(() => {
    setLoading(true);
    void run(true);
    const id = window.setInterval(() => void run(false), 5000);
    return () => { window.clearInterval(id); gen.current++; };
  }, [run]);

  return { data, fetchedAt, loading, refresh: () => void run(true) };
}
