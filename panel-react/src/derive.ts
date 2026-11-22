// Pure helpers that turn Agent responses into display state. Same rules as the classic panel:
// "healthy" is never inferred from a running process or an HTTP 200; unit evidence is the only source.
import type { Loaded } from './api';
import type { Lang, Vars } from './i18n';

export type Kind = 'ok' | 'warn' | 'bad' | 'unknown' | 'info';
export type TFn = (key: string, vars?: Vars) => string;
export interface StateLabel { k: Kind; l: string }

export const conf = (d: Loaded): any => d.status?.config?.configuration ?? {};

/** Render only the two roles accepted by the Agent config contract; unknown values stay honest. */
export function roleLabel(mode: unknown, direction: unknown, t: TFn): string {
  if (mode !== 'serve' && mode !== 'forward') return t('unknown');
  return t((direction === 'reverse' ? 'role.rev.' : 'role.') + mode);
}

export function median(v: number[]): number | null {
  const s = v.filter((x) => typeof x === 'number' && !Number.isNaN(x)).sort((a, b) => a - b);
  if (!s.length) return null;
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? (s[m] as number) : ((s[m - 1] as number) + (s[m] as number)) / 2;
}

export interface HistSample { t: number; up: boolean; rtt_ms?: number | null }
export function stats(d: Loaded): { hist: HistSample[]; avail: number | null; med: number | null } {
  const hist: HistSample[] = d.history?.samples ?? [];
  const up = hist.filter((x) => x.up);
  return { hist, avail: hist.length ? (up.length / hist.length) * 100 : null, med: median(up.map((x) => x.rtt_ms as number)) };
}

export const listenState = (s: any, t: TFn): StateLabel => (s.listener_accepting ? { k: 'ok', l: t('state.listening') } : { k: 'bad', l: t('state.notlistening') });

/** "Ready" does not exist: carrying traffic is the strongest claim. */
export function tunnelState(tn: any, t: TFn): StateLabel {
  const k: string = tn?.state ?? 'unknown';
  const kind: Kind = k === 'carrying_traffic' || k === 'session_authorized' ? 'ok' : k === 'running_no_session' ? 'warn' : k === 'down' ? 'bad' : 'unknown';
  return { k: kind, l: t('ts.' + k) };
}

export function nodeState(s: any, t: TFn): StateLabel {
  if (!s) return { k: 'unknown', l: t('state.unknown') };
  if (s.setup_pending) return { k: 'warn', l: t('state.setup') };
  return s.tunnel ? tunnelState(s.tunnel, t) : listenState(s, t);
}

export function tierKind(r: string | undefined): Kind {
  return r === 'pass' || r === 'observed' || r === 'traffic_observed' || r === 'active' ? 'ok' : r === 'fail' ? 'bad' : r === 'no_recent_traffic' || r === 'not_observed' ? 'warn' : 'unknown';
}

export function fmtNum(lang: Lang, n: number, digits = 1): string {
  return Number(n).toLocaleString(lang === 'fa' ? 'fa-IR' : 'en-US', { maximumFractionDigits: digits });
}

export function fmtBytes(lang: Lang, n: number | null | undefined): string | null {
  if (n == null || Number.isNaN(n)) return null;
  const u = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let i = 0;
  let v = Number(n);
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${fmtNum(lang, v, i ? 1 : 0)} ${u[i]}`;
}

export function life(sec: number | null | undefined, lang: Lang, t: TFn): string | null {
  if (sec == null) return null;
  return sec < 0 ? t('expired') : sec < 172800 ? t('hours', { n: fmtNum(lang, sec / 3600, 0) }) : t('days', { n: fmtNum(lang, sec / 86400, 0) });
}

export function fmtTime(lang: Lang, sec: number): string {
  return new Date(sec * 1000).toLocaleTimeString(lang === 'fa' ? 'fa-IR' : 'en-GB', { hour: '2-digit', minute: '2-digit' });
}
export function fmtDate(lang: Lang, sec: number): string {
  return new Date(sec * 1000).toLocaleString(lang === 'fa' ? 'fa-IR' : 'en-GB', { dateStyle: 'medium', timeStyle: 'medium' });
}
export function ago(lang: Lang, t: TFn, sec: number | null | undefined): string {
  if (!sec) return t('never');
  const d = Math.max(0, Math.round(Date.now() / 1000 - sec));
  if (d < 3) return t('ago.now');
  const rtf = new Intl.RelativeTimeFormat(lang === 'fa' ? 'fa' : 'en', { numeric: 'auto' });
  return d < 60 ? rtf.format(-d, 'second') : d < 3600 ? rtf.format(-Math.round(d / 60), 'minute') : d < 172800 ? rtf.format(-Math.round(d / 3600), 'hour') : rtf.format(-Math.round(d / 86400), 'day');
}

export interface Warning { sev: 'bad' | 'warn' | 'info'; res?: string; text: string; next: string; href: string }

/** Warnings come only from real data; each has severity, affected resource, reason and a next step. */
export function warnings(d: Loaded, lang: Lang, t: TFn): Warning[] {
  if (!d.status || d.status.setup_pending) return [];
  const s = d.status;
  const c = conf(d);
  const sec = d.security ?? {};
  const out: Warning[] = [];
  if (!s.listener_accepting) out.push({ sev: 'bad', res: c.tunnel_id, text: t('warn.listener', { t: c.tunnel_id ?? '—' }), next: t('warn.listener.next'), href: '#/tunnels' });
  if (sec.tls) {
    const cs = sec.certificate_seconds_remaining;
    const ls = sec.crl_seconds_remaining;
    if (cs != null && cs < 30 * 86400) out.push({ sev: cs < 7 * 86400 ? 'bad' : 'warn', res: t('servers.this'), text: t('warn.cert'), next: t('warn.cert.next'), href: '#/servers' });
    if (ls != null && ls < 7 * 86400) out.push({ sev: ls < 3 * 86400 ? 'bad' : 'warn', res: 'CRL', text: t(ls <= 0 ? 'warn.crl.expired' : 'warn.crl'), next: t('warn.crl.next'), href: '#/servers' });
    if (c.peer_certificate_pin_configured === false) out.push({ sev: 'warn', res: c.tunnel_id, text: t('warn.pin'), next: t('warn.pin.next'), href: '#/servers' });
  }
  const up = d.update;
  if (up && (up.state === 'update_available' || up.state === 'unknown_installed') && up.latest_tag) {
    out.push({ sev: 'warn', res: t('warn.update.res'), text: t(up.state === 'update_available' ? 'warn.update' : 'warn.update.unknown', { v: up.latest_tag, c: up.installed }), next: t('warn.update.next', { cmd: up.command ?? '' }), href: '#/settings' });
  }
  for (const a of (d.history?.alerts ?? []) as any[]) {
    const ev = a.evidence ?? {};
    out.push({ sev: 'warn', res: c.tunnel_id, text: t('alert.' + a.id, { p: fmtNum(lang, ev.availability_percent, 1), n: fmtNum(lang, ev.probes, 0) }), next: t('warn.avail.next'), href: '#/monitoring' });
  }
  if (d.traffic?.available && d.traffic.fresh === false) out.push({ sev: 'warn', res: c.tunnel_id, text: t('warn.stale'), next: t('warn.stale.next'), href: '#/diagnostics' });
  if (d.meta.audit === 'unavailable') out.push({ sev: 'info', res: 'Agent', text: t('warn.audit'), next: t('warn.audit.next'), href: '#/agent' });
  return out;
}

export function opLabel(e: any, t: TFn): string {
  const k = 'op.' + (e.resource ?? 'desired_state');
  const v = t(k);
  return v === k ? String(e.resource ?? e.action ?? '—') : v;
}

/** Reverse is offered only when at least one engine the Agent reports lists it. */
export function directionsOffered(d: Loaded): { direct: boolean; reverse: boolean } {
  const list: any[] = d.engines?.engines ?? [];
  return { direct: list.some((e) => (e.directions ?? []).includes('direct')), reverse: list.some((e) => (e.directions ?? []).includes('reverse')) };
}
