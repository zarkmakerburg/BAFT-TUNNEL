// Thin client for the Agent's existing HTTP API. Same endpoints, same bearer token, same semantics as the
// classic panel: nothing here invents an endpoint or a value. A missing value stays missing.

export interface ApiResult {
  status: number;
  body: any;
}

const TOKEN_KEY = 'token';

export function store(key: string, value?: string | null): string | null {
  try {
    if (value === undefined) return sessionStorage.getItem(key);
    if (value === null) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, value);
  } catch {
    /* storage may be blocked */
  }
  return null;
}

export const savedToken = (): string => store(TOKEN_KEY) ?? '';
export const saveToken = (t: string | null): void => void store(TOKEN_KEY, t);

let onUnauthorized: (() => void) | null = null;
export const setUnauthorizedHandler = (fn: (() => void) | null): void => {
  onUnauthorized = fn;
};

export async function api(method: 'GET' | 'POST', path: string, token: string, body?: unknown, quiet = false): Promise<ApiResult> {
  const headers: Record<string, string> = { Authorization: `Bearer ${token}` };
  const init: RequestInit = { method, headers };
  if (body !== undefined) {
    init.body = JSON.stringify(body);
    headers['Content-Type'] = 'application/json';
  }
  const r = await fetch(path, init);
  const json: unknown = await r.json().catch(() => ({}));
  if (r.status === 401 && token && !quiet) onUnauthorized?.();
  return { status: r.status, body: json };
}

export const ENDPOINTS = {
  status: '/v1/status',
  history: '/v1/history',
  security: '/v1/security',
  engines: '/v1/engines',
  desired: '/v1/desired',
  audit: '/v1/audit',
  traffic: '/v1/traffic',
  diagnostics: '/v1/diagnostics',
  update: '/v1/update',
  host: '/v1/host/tunnels',
  geo: '/v1/geo',
} as const;
export type EndpointKey = keyof typeof ENDPOINTS;
export type SourceState = 'ok' | 'unavailable' | 'error';

export interface Loaded {
  status: any | null;
  history: any | null;
  security: any | null;
  engines: any | null;
  desired: any | null;
  audit: any | null;
  traffic: any | null;
  diagnostics: any | null;
  update: any | null;
  host: any | null;
  geo: any | null;
  meta: Partial<Record<EndpointKey, SourceState>>;
}

/** Endpoints this app loads on every route, so the badge and the notification count agree everywhere. */
export const BASE: EndpointKey[] = ['security', 'history', 'traffic', 'audit', 'update'];

/**
 * Each endpoint ends as data (200), null + 'unavailable' (404: this agent mode does not offer it) or
 * null + 'error' (anything else). Status goes first: an agent in setup mode has nothing else.
 */
export async function load(token: string, needs: EndpointKey[]): Promise<Loaded> {
  const out: Loaded = { status: null, history: null, security: null, engines: null, desired: null, audit: null, traffic: null, diagnostics: null, update: null, host: null, geo: null, meta: {} };
  const get = async (k: EndpointKey): Promise<void> => {
    try {
      const r = await api('GET', ENDPOINTS[k], token);
      out[k] = r.status === 200 ? r.body : null;
      out.meta[k] = r.status === 200 ? 'ok' : r.status === 404 ? 'unavailable' : 'error';
    } catch {
      out[k] = null;
      out.meta[k] = 'error';
    }
  };
  await get('status');
  if (!out.status) return out;
  await Promise.all(needs.filter((k) => k !== 'status').map(get));
  return out;
}

export function newKey(prefix: string): string {
  const a = new Uint8Array(12);
  crypto.getRandomValues(a);
  return prefix + Array.from(a, (x) => x.toString(16).padStart(2, '0')).join('');
}
