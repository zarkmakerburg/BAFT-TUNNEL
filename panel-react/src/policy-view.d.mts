/** Types for `policy-view.mjs` (ADR-0047's panel view model). */

export type PolicyState = 'off' | 'active' | 'fallback' | 'unknown';
export type SourceMeta = 'ok' | 'unavailable' | 'error';

export interface Unavailability {
  kind: 'unavailable' | 'error';
  reason: string;
}

export interface PolicyMetric {
  name: string;
  unit: string | null;
  provenance: string | null;
  intervalSeconds: number | null;
  fresh: boolean | null;
  available: boolean;
  value: number | null;
  reason: string | null;
}

export interface PolicyTuple {
  engine: string | null;
  carrier: string | null;
  profile: string | null;
  claim: 'unsupported' | 'unknown' | 'measured';
  evidence: string | null;
}

export interface PolicyView {
  available: boolean;
  unavailability: Unavailability | null;
  interfaceVersion: number | null;
  state: PolicyState;
  reason: string | null;
  featureCompiled: boolean | null;
  profile: { configured: string | null; configuredEffective: string | null; running: string | null };
  switchState: { state: 'on' | 'off' | 'absent' | 'unknown'; value: string | null; sources: string[]; scan: string | null };
  adaptive: { configured: boolean | null; effective: boolean | null };
  fallback: { cause: string; since: number | null } | null;
  tuples: PolicyTuple[];
  capabilities: { name: string; value: 'supported' | 'unsupported' | 'unknown' }[];
  metrics: PolicyMetric[];
  evidence: { pid: number | null; ageSeconds: number | null } | null;
}

export const STATES: PolicyState[];
export function policyView(status: unknown, meta?: SourceMeta): PolicyView;
