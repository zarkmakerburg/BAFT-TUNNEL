import type { EndpointKey } from './api';

export type RouteId = 'overview' | 'tunnels' | 'servers' | 'monitoring' | 'setup' | 'diagnostics' | 'activity' | 'agent' | 'settings';
export type Group = 'operate' | 'setup' | 'troubleshoot' | 'system';

export interface Route { id: RouteId; group: Group; icon: string; tab?: boolean; needs: EndpointKey[] }

// Same nine routes, groups and order as the classic panel (ADR-0032 information architecture).
export const ROUTES: Route[] = [
  { id: 'overview', group: 'operate', icon: 'overview', tab: true, needs: ['desired'] },
  { id: 'tunnels', group: 'operate', icon: 'tunnels', tab: true, needs: ['host'] },
  { id: 'servers', group: 'operate', icon: 'servers', needs: ['engines', 'host'] },
  { id: 'monitoring', group: 'operate', icon: 'monitoring', needs: [] },
  { id: 'setup', group: 'setup', icon: 'setup', tab: true, needs: ['engines'] },
  { id: 'diagnostics', group: 'troubleshoot', icon: 'diagnostics', tab: true, needs: ['diagnostics'] },
  { id: 'activity', group: 'troubleshoot', icon: 'activity', needs: [] },
  { id: 'agent', group: 'system', icon: 'agent', needs: ['engines'] },
  { id: 'settings', group: 'system', icon: 'settings', needs: [] },
];
export const GROUPS: Group[] = ['operate', 'setup', 'troubleshoot', 'system'];

export function routeFromHash(hash: string): Route {
  const id = hash.replace(/^#\//, '');
  return ROUTES.find((r) => r.id === id) ?? (ROUTES[0] as Route);
}
