# Policy Pack operator guide (English)

> **Status: preview, off by default, no profile measured.** The pack is behind three gates the
> operator must set: the `policy-pack` cargo feature, the `BAFT_TUNNEL_POLICY_PACK` unit switch and
> an optional `policy.profile`. Every profile record in this repository is harness evidence at the
> **`simulated`** level, no `(engine, carrier, profile)` tuple is `measured`, and nothing here is a
> claim about internet-path latency or a stable-release readiness claim. The `v0.4.0` gate still
> needs TP-3.x per-tuple two-host LAB evidence and TP-4.1/TP-4.2's two-host LAB window; this page
> documents what exists, it does not close those.
>
> This page describes the **read side**: what `GET /v1/status`'s `policy` member carries, how the
> CLI and the panel show it, and what a reader should do when a value is `unknown` or unavailable.
> The design is [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) (profiles),
> [ADR-0047](ADR-0047-policy-status-contract.md) (this status contract),
> [ADR-0056](ADR-0056-adaptive-optimization-controller.md) (adaptive controller) and
> [ADR-0057](ADR-0057-policy-operator-surface.md) (the write surface). Persian version:
> [POLICY_PACK_OPERATOR_GUIDE.fa.md](POLICY_PACK_OPERATOR_GUIDE.fa.md).

## 1. What the pack is, and its gates

The pack is an optional add-on that changes how the Rust engines schedule relayed TCP traffic
(`tcp-tls` and `noise-tcp` on TCP/Yamux). It provides one scheduler pair per direction — a Yamux
stream window and a deficit round-robin quantum/cap — and five versioned profile rows:
`balanced/1`, `gaming/1`, `trading/1`, `streaming/1`, `bulk/1`. The `baft` engine and the UDP
carrier are out of scope: the card and the object report them as `unsupported`, never as a Rust
engine's result.

Activation needs all of the three gates, and no fourth one exists:

1. the binary is compiled with `--features policy-pack`;
2. the unit sets `BAFT_TUNNEL_POLICY_PACK` (the managed drop-in
   `/etc/systemd/system/baft-tunnel@<instance>.service.d/50-policy-pack.conf` with
   `Environment=BAFT_TUNNEL_POLICY_PACK=sched`);
3. the tunnel config optionally names `policy.profile`; absent means `balanced/1`.

With the switch unset or the feature not compiled, the relay takes the old path; the data path is
byte-for-byte today's. `POST /v1/policy/apply` (enable/disable/select/reset) is the write surface;
its receipt says `configured` and `restart_required`, never "active" — only the read below can say
that.

## 2. Where to read it

Both real routes read the same object. Neither adds a field.

### 2.1 CLI

```console
baft-tunnel policy status --instance edge
baft-tunnel policy status --instance edge --json
```

- The command checks the agent's bound instance first, then reads `GET /v1/status` and prints its
  `policy` member (text by default, the raw object with `--json`).
- Exit code **0** whenever the agent answered — including an `unknown` reading. Exit code **2** when
  the agent is unreachable, rejects the token (401), or is bound to a different instance. A failed
  read is never printed as a healthy state.
- `--agent` and `--token-file` follow the other `policy` commands; the URL must be a loopback
  origin.

### 2.2 Panel

- The **default React panel** at `/` (Agent page, "Policy Pack" card) renders the same object in
  Persian and English, RTL included, since PR #472.
- The classic panel at `/classic` deliberately has **no** policy card. It is the one-release
  fallback described in [ADR-0054](ADR-0054-react-local-agent-panel.md); use the CLI (`policy
  status`) or the default panel for this read on such a host.
- The card shows values only; a metric that was not observed reads `unavailable (reason)`, never
  `0`, and a missing member or a failed/401 status read renders as unavailable with a reason.

### 2.3 Raw route

```console
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18080/v1/status
```

The `policy` member is always present on an agent that has the read; `interface_version` is `1`.
An agent older than the read simply has no `policy` member — that is different from `off`.

## 3. The object, field by field

| Field | Values | Meaning |
|---|---|---|
| `interface_version` | `1` | ADR-0044 policy interface version this object follows. |
| `feature_compiled` | `true`/`false` | Whether **this agent binary** was built with `policy-pack`. With `false`, no switch can make the pack run. |
| `state` | `off` `active` `fallback` `unknown` | Exactly one of the closed four; see §3.1. |
| `reason` | closed set, or `null` | Why the state is not `active`; `null` only for `active`. See §3.1. |
| `profile.configured` | a versioned id, or `null` | What the tunnel **config** asks for; absent is `null` (and reported effective as `balanced/1`). |
| `profile.configured_effective` | `balanced/1` … `bulk/1` | The configured id after the schema's absent-means-Balanced rule. |
| `profile.running` | a versioned id, or `null` | The row the **running unit** published. `null` unless fresh running evidence exists; never copied from configuration. |
| `profile.class` | same as `running`, or `null` | The traffic class the running session reports; one class per session. |
| `switch.state` | `on` `off` `absent` `unknown` | Result of the read-only unit/drop-in `Environment=` scan. |
| `switch.value` | the switch string, or `null` | The value found, e.g. `sched` or `off`. |
| `switch.sources` | list of paths | Every unit/drop-in file that sets the switch. More than one makes the state `unknown`. |
| `switch.scan` | `null`, `set_in_multiple_sources`, `unreadable` | Why the switch reading is not a simple on/off. |
| `adaptive.configured` | `true`/`false` | The `policy.adaptive` opt-in from the config. |
| `adaptive.effective` | `true`/`false`/`null` | Effective only with fresh running evidence and an `active`/`fallback` state; otherwise `null`. |
| `fallback` | `null`, or `{cause, since_unix}` | The last fallback the publisher recorded, with its cause and start time. |
| `tuples[]` | one row per `(engine, carrier, profile)` | `claim` is `unsupported`, `unknown` or `measured`; `evidence` names the record or is `null`. See §3.2. |
| `capabilities` | five names → `supported`/`unsupported`/`unknown` | The engine's declaration; this surface assumes nothing and reports `unknown` until a live declaration exists. |
| `metrics` | four names → metric objects | See §3.3. |
| `evidence.running` | `{pid, sampled_unix, age_s, fresh}` or `null` | The published snapshot the running fields came from. |

An unobserved value is `null` with a reason. It is never a fabricated `0`, an empty string, or a
default profile shown as running.

### 3.1 States and reasons

| `state` | Meaning | Possible `reason` |
|---|---|---|
| `active` | Fresh running evidence says the pack is running with a resolved profile. | `null` |
| `fallback` | The publisher recorded a fallback to the configured row. | the fallback `cause` (e.g. `guard_violation`) |
| `off` | The pack is not intended to run. | `feature_not_compiled`, `switch_absent`, `switch_off` |
| `unknown` | The reading cannot prove what is running. | `no_instance`, `no_running_evidence`, `stale_running_evidence`, `unit_not_running`, `config_unreadable`, `switch_set_in_multiple_sources`, `unrecognized_state`* |

\* `unrecognized_state` and `reason_not_reported` are added by the **panel's view model**, not by the
agent, when a payload is malformed or outside the contract. The agent itself never sends them.

`active` requires all of: the switch on, the feature compiled, a `policy.json` snapshot no older than
three sampling intervals (1 s each), and — when systemd reports one — a `MainPID` matching the
snapshot's `pid`. A configured profile that was never restarted into is therefore `unknown`, which
is the same fact the write receipt's `restart_required` describes.

### 3.2 Tuple claims

A claim belongs to one exact `(engine, carrier, profile)` and only counts when a record ran that
engine. Today:

- `tcp-tls` and `noise-tcp` each list all five profiles with `claim: "unknown"` and `evidence: null`,
  because every record in the repository is a loopback harness run at `simulated` level and a
  harness line is not a tuple;
- `baft` lists one row with `claim: "unsupported"` (ADR-0031 decision 17), and never inherits a Rust
  engine's row;
- no row is `measured`. A `simulated` record can never satisfy this table.

### 3.3 Metrics

Each metric carries `value`, `unit`, `interval_seconds`, `provenance`, `fresh`, `available` and
`reason`. `value` is a number only when `available` is `true`; otherwise it is `null` and `reason`
says why.

| Metric | Unit | Provenance | Today |
|---|---|---|---|
| `queued_bytes` | bytes | measured | `available: false`, `reason: "not_sampled"` |
| `overflow_refusals` | count | measured | `available: false`, `reason: "not_sampled"` |
| `guard_violations` | count | measured | `available: false`, `reason: "not_instrumented"` — the scheduler path has no `Guarded` wrapper, so its counter is a constant, not a measurement |
| `queueing_delay_estimate_ms` | ms | estimated | `available: false`, `reason: "signal_blind"` — the arbiter queue is a rendezvous, so the write-side wait is not observable |

Do not read a missing value as zero. A `0` in this table would mean "observed, and it was zero"; a
`null` means "not observed".

## 4. What to do when a reading is not `active`

| Reading | Operator action |
|---|---|
| `off` / `feature_not_compiled` | This binary cannot run the pack. Use a build with `policy-pack` before expecting any state; nothing else is wrong. |
| `off` / `switch_absent` | The switch is not set. `policy enable` writes the managed drop-in; no profile is required (`balanced/1` is the identity row). |
| `off` / `switch_off` | Someone set the switch off deliberately. Re-enable only if that is intended. |
| `unknown` / `no_instance` | The agent is not bound to a tunnel instance; pass `--instance` and use an agent installed for that unit. |
| `unknown` / `no_running_evidence` | The config says the pack is on, but the unit has not published a fresh snapshot: restart the unit (the write receipt's `restart_required`) and re-read. |
| `unknown` / `stale_running_evidence` | The snapshot is older than three intervals: the unit stopped publishing. Check the unit is alive before trusting any running field. |
| `unknown` / `unit_not_running` | Start the tunnel unit; there is nothing to describe until it runs. |
| `unknown` / `config_unreadable` | The agent could not read the unit or a drop-in (or it uses `EnvironmentFile=`). Inspect the files with root; the read will not guess. |
| `unknown` / `switch_set_in_multiple_sources` | More than one unit/drop-in sets the switch. Keep exactly one; `policy disable` refuses to remove a file it does not own. |
| unavailable / `no_policy_member` (panel) | The agent predates the read. Upgrade the agent; the CLI on that host will fail the same way. |
| unavailable / `status_error` (panel), exit 2 (CLI) | The status endpoint is unreachable or rejected the token. Fix connectivity/credential; no state is being reported. |

## 5. The five profiles

All rows are accepted design values (ADR-0031, schema version 1). The evidence column is the
repository's own label: `simulated` harness evidence, never `measured`; the two-host LAB criteria
remain open for all four non-Balanced rows.

| Profile | Window | Quantum | Cap | Intended use | Evidence (all `simulated`) |
|---|---|---|---|---|---|
| `balanced/1` | 256 KiB | 16 KiB | 64 KiB | Default and fail-safe; identity with the measured scheduler arm | TP-2.2: level with the window arm; no reproducible benefit |
| `gaming/1` | 256 KiB | 4 KiB | 16 KiB | Smaller grants and buffers so a bulk burst occupies a smaller share of the queue | TP-3.2 record; loss/jitter/RSS/60-min and real-engine tuples not measured |
| `trading/1` | 256 KiB | 2 KiB | 8 KiB | Finest interleaving the pack can produce; throughput deliberately sacrificed | TP-3.3: no benefit over Balanced shown (p99 1.5–21.2% higher; CPU ≈4.4×) |
| `streaming/1` | 512 KiB | 32 KiB | 128 KiB | Sustained medium-bitrate delivery with a larger but bounded standing buffer | TP-3.4: loaded p50/p95/p99 2.1–2.2× Balanced; CPU lower but repeats do not overlap; delivered goodput not measured |
| `bulk/1` | 2 MiB | 64 KiB | 256 KiB | Throughput for bulk transfer, latency accepted; half of the unmanaged 4 MiB window | TP-3.5: loaded p50/p95/p99 8.3–8.9× Balanced; CPU 48.9% lower; **delivered goodput not measured** |

**2026-11-28 LAB update:** `(tcp-tls, TCP/Yamux)` is now measured for all five rows on the two LAB hosts ([run 46150559605](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/46150559605)): 128 MiB byte-exact per arm, sink goodput 1.82 / 1.80 / 1.86 / 3.47 / 13.04 MiB/s for Balanced / Gaming / Trading / Streaming / Bulk against 19.44 for the control, loaded interactive p50 108-117 ms against the control's 126 ms — one run per arm, a bulk-flow cost, **no benefit claimed**. Everything else in this guide's limits still stands: `(noise-tcp, TCP, …)`, the UDP/datagram carrier (**NOT TESTED**), loss/jitter, RSS, the 60-minute run and repeats are unmeasured, and the adaptive controller was off in every arm.


No profile is "recommended", "accepted as measured" or "ready". Choose one only as a configured
intent after reading its record, and expect `unknown` until the unit is restarted into it. The
records are [TP-3.2](POLICY_PACK_TP-3.2_GAMING_EVIDENCE.md),
[TP-3.3](POLICY_PACK_TP-3.3_TRADING_EVIDENCE.md),
[TP-3.4](POLICY_PACK_TP-3.4_STREAMING_EVIDENCE.md) and
[TP-3.5](POLICY_PACK_TP-3.5_BULK_EVIDENCE.md).

## 6. Adaptive control

`policy.adaptive` opts in to the ADR-0056 controller. It samples the session's own arbiter counters
at 1 Hz, moves at most one rung between the five accepted rows with hysteresis, persistence, dwell
and rate limits, verifies each change, and rolls back or fails safe to the configured row. It is
inert without a configured profile, off by default, and reported — never toggled — by this read.

Its harness record ([TP-4.1](POLICY_PACK_TP-4.1_ADAPTIVE_EVIDENCE.md)) is `simulated`: the loop made
**0 changes** because the rendezvous queue makes the delay signal blind (`0 ms` estimate in every
sample), so it stayed level with the static row. The fail-safe matrix
([TP-4.2](POLICY_PACK_TP-4.2_FAILSAFE_EVIDENCE.md)) is likewise `simulated`, forced and
deterministic. Both wait on the same suspended two-host LAB window.

## 7. Honest limitations

- **No path-latency or distance claim.** All measurements are single-host loopback harness runs at
  `simulated` level; they say nothing about an internet path, an ISP, or a real two-host setup.
- **TCP carrier only.** Gaming/Trading gains that would need to avoid TCP head-of-line blocking are
  not available; the UDP carrier (#244) is a separate prerequisite with its own ADR, audit and LAB.
- **The `baft` engine is not covered.** It reports `unsupported`; it does not inherit a Rust row.
- **Two metrics are not instrumented.** `guard_violations` is a constant on the scheduler path and
  `queueing_delay_estimate_ms` cannot see the write-side wait; the object says so instead of showing
  those constants as data.
- **The queue metrics are not sampled** by this surface today (`not_sampled`).
- **Not measured anywhere:** loss, jitter/impairment, RSS, the 60-minute run, real-engine tuples,
  delivered goodput for Streaming/Bulk, and any impact on co-running interactive flows.
- **The pack is off by default and no profile is accepted as measured.** The `v0.4.0` gate is
  TP-3.x per-tuple two-host LAB evidence plus TP-4.1/TP-4.2's two-host LAB window, both suspended;
  this page and the retired harness records do not substitute for them.

## 8. Related documents

- [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) — the plan and item-by-item
  state.
- [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) — profiles, engine scope and decisions.
- [ADR-0047](ADR-0047-policy-status-contract.md) — the status contract this page documents.
- [ADR-0057](ADR-0057-policy-operator-surface.md) — enable/disable/select/reset and its receipts.
- [ADR-0056](ADR-0056-adaptive-optimization-controller.md) — the adaptive controller.
- [ADR-0054](ADR-0054-react-local-agent-panel.md) — the default React panel and the classic
  fallback that has no policy card.
