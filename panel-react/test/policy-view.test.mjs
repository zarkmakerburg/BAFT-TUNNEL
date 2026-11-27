// The panel's decision table for ADR-0047's `policy` member. Run with `npm test` (node:test);
// no bundler, DOM or Agent is involved, and the fixtures are real payload shapes.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

import { policyView, STATES } from '../src/policy-view.mjs';

const fixture = (name) => JSON.parse(fs.readFileSync(new URL(`./fixtures/${name}`, import.meta.url), 'utf8'));

test('the closed state set is ADR-0047 decision 3', () => {
  assert.deepEqual(STATES, ['off', 'active', 'fallback', 'unknown']);
});

test('a status read that failed is unavailable with its reason, never healthy', () => {
  const error = policyView(null, 'error');
  assert.equal(error.available, false);
  assert.deepEqual(error.unavailability, { kind: 'error', reason: 'status_error' });
  assert.equal(error.state, 'unknown');

  const gone = policyView(null, 'unavailable');
  assert.deepEqual(gone.unavailability, { kind: 'unavailable', reason: 'status_unavailable' });

  const absent = policyView(null, 'ok');
  assert.deepEqual(absent.unavailability, { kind: 'error', reason: 'status_missing' });
});

test('an agent without the policy member is unavailable, not off', () => {
  const view = policyView(fixture('status-absent-policy.json'), 'ok');
  assert.equal(view.available, false);
  assert.deepEqual(view.unavailability, { kind: 'unavailable', reason: 'no_policy_member' });
  assert.equal(view.state, 'unknown');
  assert.equal(view.reason, null);
  assert.deepEqual(view.tuples, []);
  assert.deepEqual(view.metrics, []);
});

test('gate-off renders off/feature_not_compiled and never shows running or effective values', () => {
  const view = policyView(fixture('policy-gate-off.json'), 'ok');
  assert.equal(view.available, true);
  assert.equal(view.state, 'off');
  assert.equal(view.reason, 'feature_not_compiled');
  assert.equal(view.featureCompiled, false);
  // The fixture deliberately carries stale running/effective fields; the card must refuse them.
  assert.equal(view.profile.configured, 'gaming/1');
  assert.equal(view.profile.running, null);
  assert.equal(view.adaptive.configured, true);
  assert.equal(view.adaptive.effective, null);
  assert.equal(view.fallback, null);
  assert.equal(view.switchState.state, 'on');
  assert.equal(view.switchState.sources.length, 1);
});

test('an unavailable metric shows its reason, never the 0 in the payload', () => {
  const view = policyView(fixture('policy-gate-off.json'), 'ok');
  const metric = (name) => view.metrics.find((m) => m.name === name);
  assert.equal(metric('guard_violations').available, false);
  assert.equal(metric('guard_violations').value, null);
  assert.equal(metric('guard_violations').reason, 'not_instrumented');
  assert.equal(metric('queueing_delay_estimate_ms').value, null);
  assert.equal(metric('queueing_delay_estimate_ms').reason, 'signal_blind');
  assert.equal(metric('queueing_delay_estimate_ms').provenance, 'estimated');
});

test('an active reading shows configured and running distinctly, with switch sources and tuples', () => {
  const view = policyView(fixture('policy-active.json'), 'ok');
  assert.equal(view.state, 'active');
  assert.equal(view.reason, null);
  assert.equal(view.profile.configured, 'gaming/1');
  assert.equal(view.profile.configuredEffective, 'gaming/1');
  assert.equal(view.profile.running, 'gaming/1');
  assert.equal(view.adaptive.configured, true);
  assert.equal(view.adaptive.effective, false);
  assert.equal(view.switchState.value, 'sched');
  assert.match(view.switchState.sources[0], /50-policy-pack\.conf$/);
  assert.equal(view.tuples.length, 2);
  assert.ok(view.tuples.every((tuple) => tuple.claim === 'unknown' && tuple.evidence === null));
  assert.equal(view.evidence.pid, 4242);
  assert.equal(view.evidence.ageSeconds, 0);
});

test('a measured zero stays zero only when the payload says it is available', () => {
  const view = policyView(fixture('policy-active.json'), 'ok');
  const queued = view.metrics.find((m) => m.name === 'queued_bytes');
  assert.equal(queued.available, true);
  assert.equal(queued.value, 0);
  assert.equal(queued.reason, null);
  assert.equal(queued.fresh, true);
});

test('an unknown reading carries its reason and hides running evidence', () => {
  const view = policyView(
    {
      policy: {
        interface_version: 1,
        feature_compiled: true,
        state: 'unknown',
        reason: 'no_running_evidence',
        profile: { configured: 'gaming/1', configured_effective: 'gaming/1', running: 'gaming/1' },
        adaptive: { configured: true, effective: true },
        switch: { state: 'on', value: 'sched', sources: [] },
        tuples: [],
        metrics: {},
      },
    },
    'ok',
  );
  assert.equal(view.state, 'unknown');
  assert.equal(view.reason, 'no_running_evidence');
  assert.equal(view.profile.configured, 'gaming/1');
  assert.equal(view.profile.running, null);
  assert.equal(view.adaptive.effective, null);
  assert.equal(view.fallback, null);
});

test('a state or reason outside the contract never becomes healthy', () => {
  const ready = policyView({ policy: { state: 'ready', reason: null, feature_compiled: true } }, 'ok');
  assert.equal(ready.state, 'unknown');
  assert.equal(ready.reason, 'unrecognized_state');

  const missing = policyView({ policy: { feature_compiled: true } }, 'ok');
  assert.equal(missing.state, 'unknown');
  assert.equal(missing.reason, 'unrecognized_state');

  const noReason = policyView({ policy: { state: 'unknown' } }, 'ok');
  assert.equal(noReason.reason, 'reason_not_reported');
});

test('a fallback keeps the cause its publisher recorded and shows the running row', () => {
  const view = policyView(
    {
      policy: {
        state: 'fallback',
        reason: 'guard_violation',
        feature_compiled: true,
        profile: { configured: 'gaming/1', running: 'balanced/1' },
        fallback: { cause: 'guard_violation', since_unix: 1764219000 },
        adaptive: { configured: true, effective: false },
      },
    },
    'ok',
  );
  assert.equal(view.state, 'fallback');
  assert.equal(view.reason, 'guard_violation');
  assert.deepEqual(view.fallback, { cause: 'guard_violation', since: 1764219000 });
  assert.equal(view.profile.running, 'balanced/1');
});

test('claims outside the closed set degrade to unknown and capabilities are never assumed', () => {
  const view = policyView(
    {
      policy: {
        state: 'unknown',
        reason: 'no_running_evidence',
        feature_compiled: true,
        tuples: [
          { engine: 'tcp-tls', carrier: 'tcp', profile: 'gaming/1', claim: 'measured', evidence: 'run-1' },
          { engine: 'tcp-tls', carrier: 'tcp', profile: 'bulk/1', claim: 'yes', evidence: null },
        ],
        capabilities: { priority_enforcement: 'supported', datagram: 'maybe' },
      },
    },
    'ok',
  );
  assert.equal(view.tuples[0].claim, 'measured');
  assert.equal(view.tuples[1].claim, 'unknown');
  const capability = (name) => view.capabilities.find((c) => c.name === name).value;
  assert.equal(capability('priority_enforcement'), 'supported');
  assert.equal(capability('datagram'), 'unknown');
  assert.equal(view.capabilities.length, 5);
});

test('malformed values never crash the card or invent a number', () => {
  const view = policyView(
    {
      policy: {
        state: 'active',
        feature_compiled: true,
        profile: { configured: 7, configured_effective: null, running: false },
        switch: { state: 'on', sources: 'x' },
        tuples: 'nope',
        metrics: {
          queued_bytes: { value: '12', available: true, provenance: 'measured', reason: 'not_sampled' },
          overflow_refusals: 'nope',
        },
        capabilities: 'x',
      },
    },
    'ok',
  );
  assert.equal(view.profile.configured, null);
  assert.equal(view.profile.running, null);
  assert.deepEqual(view.switchState.sources, []);
  assert.deepEqual(view.tuples, []);
  const queued = view.metrics.find((m) => m.name === 'queued_bytes');
  assert.equal(queued.available, false);
  assert.equal(queued.value, null);
  assert.equal(queued.reason, 'not_sampled');
  const overflow = view.metrics.find((m) => m.name === 'overflow_refusals');
  assert.equal(overflow.value, null);
  assert.equal(overflow.reason, 'not_observed');
  assert.ok(view.capabilities.every((c) => c.value === 'unknown'));
});
