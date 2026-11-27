// ADR-0047's `policy` member, turned into exactly what the card may display.
//
// Pure and dependency-free: no DOM, no fetch, no clock. `policyView` is fed the `GET /v1/status`
// body and the source state the feed recorded for that endpoint, and it never adds a value the
// payload did not carry. The rules it enforces:
//
// * absent status, a 401/unreachable status endpoint and an absent `policy` member are unavailable
//   readings with a reason, never a healthy default;
// * `state` is one of the closed four; anything else is `unknown` with `unrecognized_state`;
// * `profile.running` and `adaptive.effective` are shown only for `active`/`fallback`; configured
//   intent is never promoted to running;
// * a metric is displayed only when `available === true` and its value is a finite number, so an
//   unobserved metric is its reason (`not_instrumented`, `signal_blind`, ...), never `0`;
// * tuple claims are restricted to `unsupported`/`unknown`/`measured`, and capabilities default to
//   `unknown` rather than assumed supported.
//
// The module is plain ESM so `node --test` can exercise it without a bundler or a DOM; the React
// card imports it through `policy-view.d.mts`.

export const STATES = ['off', 'active', 'fallback', 'unknown'];
const CLAIMS = ['unsupported', 'unknown', 'measured'];
const CAPABILITIES = ['priority_enforcement', 'write_pacing', 'rtt_observation', 'queue_observation', 'datagram'];
const SWITCH_STATES = ['on', 'off', 'absent', 'unknown'];

const isObject = (value) => typeof value === 'object' && value !== null && !Array.isArray(value);
const text = (value) => (typeof value === 'string' && value.length ? value : null);
const bool = (value) => (typeof value === 'boolean' ? value : null);
const finite = (value) => (typeof value === 'number' && Number.isFinite(value) ? value : null);

const base = (extra) => ({
  available: true,
  unavailability: null,
  interfaceVersion: null,
  state: 'unknown',
  reason: null,
  featureCompiled: null,
  profile: { configured: null, configuredEffective: null, running: null },
  switchState: { state: 'unknown', value: null, sources: [], scan: null },
  adaptive: { configured: null, effective: null },
  fallback: null,
  tuples: [],
  capabilities: CAPABILITIES.map((name) => ({ name, value: 'unknown' })),
  metrics: [],
  evidence: null,
  ...extra,
});

/**
 * @param {unknown} status the `/v1/status` body, or null when the feed could not read it
 * @param {'ok'|'unavailable'|'error'|undefined} meta the feed's state for the status endpoint
 */
export function policyView(status, meta = 'ok') {
  if (!isObject(status)) {
    const kind = meta === 'unavailable' ? 'unavailable' : 'error';
    const reason = meta === 'unavailable' ? 'status_unavailable' : meta === 'error' ? 'status_error' : 'status_missing';
    return base({ available: false, unavailability: { kind, reason } });
  }
  const policy = status.policy;
  if (!isObject(policy)) {
    // An agent older than the ADR-0047 read: the member is simply absent. That is not "off".
    return base({ available: false, unavailability: { kind: 'unavailable', reason: 'no_policy_member' } });
  }

  const recognized = typeof policy.state === 'string' && STATES.includes(policy.state);
  const state = recognized ? policy.state : 'unknown';
  let reason = text(policy.reason);
  if (!recognized) reason = 'unrecognized_state';
  else if (state !== 'active' && !reason) reason = 'reason_not_reported';

  // Only published running evidence may be called running. If a payload ever pairs a running
  // profile with a state that does not come from running evidence, the card still refuses it.
  const runningVisible = state === 'active' || state === 'fallback';
  const profileIn = isObject(policy.profile) ? policy.profile : {};
  const switchIn = isObject(policy.switch) ? policy.switch : {};
  const adaptiveIn = isObject(policy.adaptive) ? policy.adaptive : {};
  const fallbackIn = isObject(policy.fallback) ? policy.fallback : null;
  const evidenceIn = isObject(policy.evidence) && isObject(policy.evidence.running) ? policy.evidence.running : null;

  const capabilitiesIn = isObject(policy.capabilities) ? policy.capabilities : {};
  const metricsIn = isObject(policy.metrics) ? policy.metrics : {};

  return base({
    available: true,
    interfaceVersion: finite(policy.interface_version),
    state,
    reason,
    featureCompiled: bool(policy.feature_compiled),
    profile: {
      configured: text(profileIn.configured),
      configuredEffective: text(profileIn.configured_effective),
      running: runningVisible ? text(profileIn.running) : null,
    },
    switchState: {
      state: SWITCH_STATES.includes(switchIn.state) ? switchIn.state : 'unknown',
      value: text(switchIn.value),
      sources: Array.isArray(switchIn.sources) ? switchIn.sources.filter((source) => typeof source === 'string' && source) : [],
      scan: text(switchIn.scan),
    },
    adaptive: {
      configured: bool(adaptiveIn.configured),
      effective: runningVisible ? bool(adaptiveIn.effective) : null,
    },
    fallback:
      state === 'fallback' && fallbackIn
        ? { cause: text(fallbackIn.cause) ?? 'cause_not_reported', since: finite(fallbackIn.since_unix) }
        : null,
    tuples: (Array.isArray(policy.tuples) ? policy.tuples : []).filter(isObject).map((row) => ({
      engine: text(row.engine),
      carrier: text(row.carrier),
      profile: text(row.profile),
      claim: CLAIMS.includes(row.claim) ? row.claim : 'unknown',
      evidence: text(row.evidence),
    })),
    capabilities: CAPABILITIES.map((name) => ({
      name,
      value: ['supported', 'unsupported', 'unknown'].includes(capabilitiesIn[name]) ? capabilitiesIn[name] : 'unknown',
    })),
    metrics: Object.keys(metricsIn)
      .sort()
      .map((name) => {
        const metric = isObject(metricsIn[name]) ? metricsIn[name] : {};
        const value = finite(metric.value);
        const available = metric.available === true && value !== null;
        return {
          name,
          unit: text(metric.unit),
          provenance: text(metric.provenance),
          intervalSeconds: finite(metric.interval_seconds),
          fresh: bool(metric.fresh),
          available,
          // An unavailable metric never contributes a number, not even a literal 0.
          value: available ? value : null,
          reason: available ? null : (text(metric.reason) ?? 'not_observed'),
        };
      }),
    evidence: evidenceIn
      ? { pid: finite(evidenceIn.pid), ageSeconds: finite(evidenceIn.age_s) }
      : null,
  });
}
