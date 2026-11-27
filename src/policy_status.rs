//! ADR-0047: the Policy Pack's read side.
//!
//! Two halves, both compiled in every build:
//!
//! * [`start_publisher`] / [`note_row`] let the tunnel process publish what it resolved into
//!   `$RUNTIME_DIRECTORY/policy.json`, following ADR-0024 decision 1's "the unit publishes
//!   evidence, the agent only reads it" pattern;
//! * [`compose`] builds the read-only `policy` member of `GET /v1/status` from three labelled
//!   sources, in the order ADR-0047 decision 2 fixed:
//!
//!   1. **running facts** — the freshest `policy.json` written by the live unit. Only this source
//!      can produce `active` or `fallback`, `profile.running` or an effective adaptive value. A
//!      pid that does not match the unit's `MainPID`, a stale sample or an unreadable file is not
//!      evidence, and the object says `unknown` rather than guessing;
//!   2. **configured intent** — the tunnel config the agent already loaded (`policy.profile`,
//!      `policy.adaptive`) and the same read-only `Environment=` scan of the instance unit, the
//!      template unit and their drop-in directories that the root helper uses before writing;
//!      a source that cannot be read, `EnvironmentFile=` indirection, or two sources that both
//!      set the switch makes the switch `unknown` instead of picking one;
//!   3. **`feature_compiled`** — this binary's own compile-time constant.
//!
//! The agent never writes, never calls `systemctl` for this and never reads `/proc/<pid>/environ`
//! (it is unprivileged, SA-09). A configured profile is never called active: a tunnel whose file
//! says `gaming/1` but that systemd has not restarted into is `unknown`, exactly the state
//! ADR-0057's receipt describes as `restart_required`.
//!
//! Every metric that nothing observes is `null` with `available:false` and a reason, never `0`,
//! including the two the pack's own records reconcile:
//!
//! * `guard_violations` is `not_instrumented` while no `Guarded<TrafficPolicy>` wraps the
//!   scheduler path (TP-4.2's record: the counter is a constant there);
//! * `queueing_delay_estimate_ms` is `signal_blind` while the arbiter's rendezvous cannot expose
//!   the write-side wait (TP-4.1's record), which is "not observable", not "no delay".

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::config::RuntimeConfig;
use crate::policy_profile::PolicyProfile;
use crate::session_status;

/// The unit-published running snapshot.
pub const FILE: &str = "policy.json";
/// Snapshot schema this build writes and reads.
pub const SCHEMA_VERSION: u64 = 1;
/// Sampling interval the snapshot declares; the publisher writes at start and on a policy change.
pub const INTERVAL_SECONDS: u64 = 1;
/// A snapshot older than this many intervals is not evidence of what is running now.
const FRESH_INTERVALS: u64 = 3;
const MAX_FILE_BYTES: u64 = 64 * 1024;
const SWITCH: &str = "BAFT_TUNNEL_POLICY_PACK";
/// Where systemd keeps unit files on the supported baseline (Ubuntu 22.04 / systemd 249).
const UNITS_DIR: &str = "/etc/systemd/system";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// ADR-0044's policy interface version. The constant lives in the feature-gated `policy` module,
/// so the default build reports the same schema version it publishes.
fn interface_version() -> u64 {
    #[cfg(feature = "policy-pack")]
    {
        u64::from(crate::policy::POLICY_INTERFACE_VERSION)
    }
    #[cfg(not(feature = "policy-pack"))]
    {
        SCHEMA_VERSION
    }
}

fn feature_compiled() -> bool {
    cfg!(feature = "policy-pack")
}

/// `unset`, empty or `off` is off. With the feature compiled in, the switch's grammar decides;
/// without it the pack cannot run, so only "is the operator's intent visibly off" matters and the
/// state is `off`/`feature_not_compiled` either way.
#[cfg(feature = "policy-pack")]
fn switch_on(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| crate::policy_relay::PolicyRelay::parse(value).is_some())
}

#[cfg(not(feature = "policy-pack"))]
fn switch_on(raw: Option<&str>) -> bool {
    raw.map(str::trim)
        .is_some_and(|value| !value.is_empty() && !value.eq_ignore_ascii_case("off"))
}

// ---------------------------------------------------------------------------------------------
// Publisher (the tunnel process)
// ---------------------------------------------------------------------------------------------

struct Published {
    engine: &'static str,
    configured: Option<PolicyProfile>,
    adaptive: bool,
    started: u64,
}

static PUBLISHED: Mutex<Option<Published>> = Mutex::new(None);

/// Publish the start-of-process snapshot. A no-op outside the systemd unit (no `RUNTIME_DIRECTORY`),
/// so a hand-run tunnel writes nothing, as the other runtime evidence files already do.
pub fn start_publisher(engine: &'static str, configured: Option<PolicyProfile>, adaptive: bool) {
    let Some(dir) = session_status::runtime_dir() else {
        return;
    };
    let started = now();
    {
        let mut state = PUBLISHED.lock().unwrap_or_else(|e| e.into_inner());
        *state = Some(Published {
            engine,
            configured,
            adaptive,
            started,
        });
    }
    session_status::publish_json(
        &dir,
        FILE,
        &snapshot(engine, configured, adaptive, started, now(), None),
    );
}

/// Rewrite the snapshot when the adaptive controller moves the row, so `profile.running` never
/// describes a row the session has already left. Called from the relay's accepted-change path.
#[allow(dead_code)] // only the `policy-pack` relay path can change a row
pub fn note_row(row: PolicyProfile) {
    let published = {
        let state = PUBLISHED.lock().unwrap_or_else(|e| e.into_inner());
        state.as_ref().map(|p| Published {
            engine: p.engine,
            configured: p.configured,
            adaptive: p.adaptive,
            started: p.started,
        })
    };
    let Some(published) = published else {
        return;
    };
    let Some(dir) = session_status::runtime_dir() else {
        return;
    };
    session_status::publish_json(
        &dir,
        FILE,
        &snapshot(
            published.engine,
            published.configured,
            published.adaptive,
            published.started,
            now(),
            Some(row),
        ),
    );
}

fn snapshot(
    engine: &str,
    configured: Option<PolicyProfile>,
    adaptive: bool,
    started: u64,
    sampled: u64,
    resolved_override: Option<PolicyProfile>,
) -> Value {
    let raw = std::env::var(SWITCH).ok();
    let on = switch_on(raw.as_deref());
    let resolved = on.then(|| {
        resolved_override
            .or(configured)
            .unwrap_or(PolicyProfile::Balanced)
    });
    json!({
        "schema": SCHEMA_VERSION,
        "interface_version": interface_version(),
        "feature_compiled": feature_compiled(),
        "engine": engine,
        "carrier": "tcp",
        "switch": raw,
        "configured_profile": configured.map(PolicyProfile::id),
        "resolved_profile": resolved.map(PolicyProfile::id),
        "adaptive_configured": adaptive,
        "adaptive_effective": on && adaptive && configured.is_some(),
        // Nothing sets this today: the controller reverts to the configured row, and ADR-0056's
        // opt-in has no live fallback-to-Balanced path yet. Recorded as null rather than invented.
        "fallback": Value::Null,
        "metrics": metrics_block(),
        "pid": std::process::id(),
        "started_unix": started,
        "sampled_unix": sampled,
        "interval_s": INTERVAL_SECONDS,
    })
}

/// One ADR-0024 metric value that nothing observed: no value, never `0`, with the reason.
fn unavailable(unit: &str, provenance: &str, reason: &str) -> Value {
    json!({
        "value": Value::Null,
        "unit": unit,
        "interval_seconds": INTERVAL_SECONDS,
        "provenance": provenance,
        "fresh": false,
        "available": false,
        "reason": reason,
    })
}

fn metrics_block() -> Value {
    json!({
        "queued_bytes": unavailable("bytes", "measured", "not_sampled"),
        "overflow_refusals": unavailable("count", "measured", "not_sampled"),
        // TP-4.2: the scheduler path builds no `Guarded<TrafficPolicy>`, so its counter is a
        // constant there. Showing that constant as a measurement would be fake data.
        "guard_violations": unavailable("count", "measured", "not_instrumented"),
        // TP-4.1: the arbiter's queue is a rendezvous; a zero estimate means "not observable".
        "queueing_delay_estimate_ms": unavailable("ms", "estimated", "signal_blind"),
    })
}

// ---------------------------------------------------------------------------------------------
// Composer (the agent's `GET /v1/status`)
// ---------------------------------------------------------------------------------------------

/// What the agent knows when it composes the object. `unit_result` is ADR-0024's `unit.result`
/// (`active`/`inactive`/`unknown`) and `main_pid` is what systemd reports for the instance.
pub struct Inputs<'a> {
    pub instance: Option<&'a str>,
    pub runtime_dir: Option<&'a Path>,
    pub config: &'a RuntimeConfig,
    pub unit_result: Option<&'a str>,
    pub main_pid: Option<u32>,
}

/// Compose the `policy` member. Reads only: no file is written and no command is run.
pub fn compose(inputs: Inputs<'_>) -> Value {
    compose_at(Path::new(UNITS_DIR), &inputs, now())
}

fn compose_at(units: &Path, inputs: &Inputs<'_>, at: u64) -> Value {
    let policy = inputs.config.policy_settings();
    let engine = inputs.config.engine_name();
    let scan = match inputs.instance {
        Some(instance) => scan_switch_in(units, instance),
        None => SwitchScan::default(),
    };
    let running = read_running(inputs.runtime_dir, inputs.main_pid, at);

    let configured_effective = policy.profile.unwrap_or_default();
    let running_profile = match (&running, scan.state()) {
        (Running::Fresh(value), SwitchState::On) if value["resolved_profile"].is_string() => {
            value["resolved_profile"].as_str()
        }
        _ => None,
    };
    let fallback = match (&running, running_profile.is_some()) {
        (Running::Fresh(value), true) if value["fallback"].is_object() => {
            Some(value["fallback"].clone())
        }
        _ => None,
    };

    let (mut state, reason) = state_of(inputs, &scan, &running);
    if state == "active" && fallback.is_some() {
        state = "fallback";
    }
    // A fallback's `reason` is the cause its publisher recorded, which is not one of the closed
    // "why is there no reading" reasons above.
    let reason = if state == "fallback" {
        fallback.as_ref().and_then(|item| item["cause"].as_str())
    } else {
        reason
    };
    let (switch_state, switch_note, switch_value, switch_sources) = (
        scan.label(),
        scan.note(),
        scan.values.first().cloned(),
        scan.sources.clone(),
    );
    let adaptive_effective = match (&running, scan.state(), state) {
        (Running::Fresh(value), SwitchState::On, "active" | "fallback") => {
            value["adaptive_effective"].as_bool()
        }
        _ => None,
    };
    let evidence = match &running {
        Running::Fresh(value) => json!({
            "running": {
                "pid": value["pid"],
                "sampled_unix": value["sampled_unix"],
                "age_s": value["sampled_unix"].as_u64().map(|t| at.saturating_sub(t)),
                "fresh": true,
            }
        }),
        _ => json!({"running": Value::Null}),
    };

    json!({
        "interface_version": interface_version(),
        "feature_compiled": feature_compiled(),
        "state": state,
        "reason": reason,
        "profile": {
            "configured": policy.profile.map(PolicyProfile::id),
            "configured_effective": configured_effective.id(),
            "running": running_profile,
            "class": running_profile,
        },
        "switch": {
            "state": switch_state,
            "value": switch_value,
            "sources": switch_sources,
            "scan": switch_note,
        },
        "adaptive": {
            "configured": policy.adaptive,
            "effective": adaptive_effective,
        },
        "fallback": fallback,
        "tuples": tuples(engine, policy.profile),
        // ADR-0044 decision 4: no capability is assumed, and this surface has no live engine
        // declaration to read, so every one stays `unknown` (never hidden, never "supported").
        "capabilities": {
            "priority_enforcement": "unknown",
            "write_pacing": "unknown",
            "rtt_observation": "unknown",
            "queue_observation": "unknown",
            "datagram": "unknown",
        },
        "metrics": match &running {
            Running::Fresh(value) if value["metrics"].is_object() => value["metrics"].clone(),
            _ => metrics_block(),
        },
        "evidence": evidence,
    })
}

/// ADR-0047 decision 3's table, in order.
fn state_of(
    inputs: &Inputs<'_>,
    scan: &SwitchScan,
    running: &Running,
) -> (&'static str, Option<&'static str>) {
    if inputs.instance.is_none() {
        return ("unknown", Some("no_instance"));
    }
    match scan.state() {
        SwitchState::Ambiguous => ("unknown", Some("switch_set_in_multiple_sources")),
        SwitchState::Unknown => ("unknown", Some("config_unreadable")),
        SwitchState::Absent | SwitchState::Off => {
            if feature_compiled() {
                (
                    "off",
                    Some(if scan.state() == SwitchState::Absent {
                        "switch_absent"
                    } else {
                        "switch_off"
                    }),
                )
            } else {
                ("off", Some("feature_not_compiled"))
            }
        }
        SwitchState::On => {
            if !feature_compiled() {
                // A switch an operator set cannot make a pack that is not compiled in run, and the
                // write surface refuses `enable` for the same reason.
                return ("off", Some("feature_not_compiled"));
            }
            match running {
                Running::Fresh(value) if value["resolved_profile"].is_string() => ("active", None),
                Running::Fresh(_) | Running::Unreadable => ("unknown", Some("no_running_evidence")),
                Running::Stale => ("unknown", Some("stale_running_evidence")),
                Running::Missing => {
                    if inputs.unit_result == Some("inactive") {
                        ("unknown", Some("unit_not_running"))
                    } else {
                        ("unknown", Some("no_running_evidence"))
                    }
                }
            }
        }
    }
}

/// Per-tuple claims (ADR-0031 decision 14, ADR-0047 decision 5). A record counts only for its own
/// engine, carrier and profile, and only when its run exercised that engine; the pack's records are
/// all loopback harness lines, so no tuple is `measured` and none becomes so by inference.
fn tuples(engine: &str, configured: Option<PolicyProfile>) -> Value {
    const ALL: [PolicyProfile; 5] = [
        PolicyProfile::Balanced,
        PolicyProfile::Gaming,
        PolicyProfile::Trading,
        PolicyProfile::Streaming,
        PolicyProfile::Bulk,
    ];
    if matches!(engine, "tcp-tls" | "noise-tcp") {
        Value::Array(
            ALL.iter()
                .map(|profile| {
                    json!({
                        "engine": engine,
                        "carrier": "tcp",
                        "profile": profile.id(),
                        "claim": "unknown",
                        "evidence": Value::Null,
                    })
                })
                .collect(),
        )
    } else {
        // The BAFT engine is out of scope (ADR-0031 decision 17); shown, never inherited from a
        // Rust engine's tuple.
        json!([{
            "engine": engine,
            "carrier": "tcp",
            "profile": configured.unwrap_or_default().id(),
            "claim": "unsupported",
            "evidence": Value::Null,
        }])
    }
}

enum Running {
    Missing,
    Stale,
    Unreadable,
    Fresh(Value),
}

fn read_running(runtime_dir: Option<&Path>, main_pid: Option<u32>, at: u64) -> Running {
    let Some(dir) = runtime_dir else {
        return Running::Missing;
    };
    let path = dir.join(FILE);
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Running::Missing,
        Err(_) => return Running::Unreadable,
    };
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Running::Unreadable;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return Running::Unreadable;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Running::Unreadable;
    };
    // A snapshot this build cannot interpret is not evidence of anything (ADR-0047 decision 11).
    if value["schema"].as_u64() != Some(SCHEMA_VERSION) {
        return Running::Unreadable;
    }
    if let Some(pid) = main_pid {
        if value["pid"].as_u64() != Some(u64::from(pid)) {
            return Running::Unreadable;
        }
    }
    let sampled = value["sampled_unix"].as_u64().unwrap_or(0);
    let interval = value["interval_s"]
        .as_u64()
        .unwrap_or(INTERVAL_SECONDS)
        .max(1);
    if at.saturating_sub(sampled) > interval.saturating_mul(FRESH_INTERVALS) {
        return Running::Stale;
    }
    Running::Fresh(value)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SwitchState {
    Absent,
    Off,
    On,
    Ambiguous,
    Unknown,
}

#[derive(Default)]
struct SwitchScan {
    values: Vec<String>,
    sources: Vec<String>,
    unreadable: bool,
}

impl SwitchScan {
    fn state(&self) -> SwitchState {
        if self.unreadable {
            return SwitchState::Unknown;
        }
        match self.values.len() {
            0 => SwitchState::Absent,
            1 => {
                if self.values[0].trim().is_empty() || self.values[0].eq_ignore_ascii_case("off") {
                    SwitchState::Off
                } else {
                    SwitchState::On
                }
            }
            _ => SwitchState::Ambiguous,
        }
    }

    fn label(&self) -> &'static str {
        match self.state() {
            SwitchState::Absent => "absent",
            SwitchState::Off => "off",
            SwitchState::On => "on",
            SwitchState::Ambiguous | SwitchState::Unknown => "unknown",
        }
    }

    fn note(&self) -> Option<&'static str> {
        match self.state() {
            SwitchState::Ambiguous => Some("set_in_multiple_sources"),
            SwitchState::Unknown => Some("unreadable"),
            _ => None,
        }
    }
}

/// The same read-only scan the root helper performs before writing (ADR-0057 decision 2): the
/// instance unit, the template unit that applies to every instance, and the `.conf` drop-ins of
/// both. Only `Environment=` lines are read; `EnvironmentFile=` cannot be resolved without root,
/// so its presence makes the switch `unknown`.
fn scan_switch_in(units: &Path, instance: &str) -> SwitchScan {
    let mut scan = SwitchScan::default();
    for path in [
        units.join(format!("baft-tunnel@{instance}.service")),
        units.join("baft-tunnel@.service"),
    ] {
        if path.is_file() {
            collect_switch(&path, &mut scan);
        }
    }
    for dir in [
        units.join(format!("baft-tunnel@{instance}.service.d")),
        units.join("baft-tunnel@.service.d"),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("conf"))
            .collect();
        paths.sort();
        for path in paths {
            if path.is_file() {
                collect_switch(&path, &mut scan);
            }
        }
    }
    scan
}

fn collect_switch(path: &Path, scan: &mut SwitchScan) {
    let Ok(text) = std::fs::read_to_string(path) else {
        scan.unreadable = true;
        return;
    };
    let prefix = format!("{SWITCH}=");
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("EnvironmentFile=") {
            scan.unreadable = true;
            continue;
        }
        let Some(rest) = line.strip_prefix("Environment=") else {
            continue;
        };
        for token in rest.split_whitespace() {
            let token = token.trim_matches('"');
            if let Some(value) = token.strip_prefix(&prefix) {
                scan.values.push(value.to_string());
                scan.sources.push(path.display().to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(policy: &str) -> RuntimeConfig {
        let mut body = json!({
            "mode": "forward", "listen": "127.0.0.1:1", "remote": "192.0.2.1:8443",
            "server_name": "x", "local_id": "a", "peer_id": "b", "tunnel_id": "t",
            "direction": "direct", "peer_fingerprint": "00", "key": "/secret/key.pem"
        });
        if !policy.is_empty() {
            body["policy"] = serde_json::from_str(policy).unwrap();
        }
        serde_json::from_value(body).unwrap()
    }

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("baft-policy-status-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_switch(units: &Path, file: &str, value: &str) {
        let path = units.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("[Service]\nEnvironment={SWITCH}={value}\n")).unwrap();
    }

    #[cfg(feature = "policy-pack")]
    fn write_snapshot(runtime: &Path, pid: u32, sampled: u64, resolved: Option<&str>) {
        let mut body = json!({
            "schema": SCHEMA_VERSION,
            "interface_version": interface_version(),
            "feature_compiled": true,
            "engine": "tcp-tls",
            "carrier": "tcp",
            "switch": "sched",
            "configured_profile": resolved,
            "resolved_profile": resolved,
            "adaptive_configured": false,
            "adaptive_effective": false,
            "fallback": Value::Null,
            "metrics": metrics_block(),
            "pid": pid,
            "started_unix": sampled,
            "sampled_unix": sampled,
            "interval_s": INTERVAL_SECONDS,
        });
        if resolved.is_none() {
            body["resolved_profile"] = Value::Null;
        }
        std::fs::write(runtime.join(FILE), body.to_string()).unwrap();
    }

    fn compose_with(
        units: &Path,
        runtime: &Path,
        cfg: &RuntimeConfig,
        pid: Option<u32>,
        at: u64,
    ) -> Value {
        compose_at(
            units,
            &Inputs {
                instance: Some("edge"),
                runtime_dir: Some(runtime),
                config: cfg,
                unit_result: Some("active"),
                main_pid: pid,
            },
            at,
        )
    }

    #[test]
    fn without_an_instance_every_state_is_unknown() {
        let units = dir("no-instance-u");
        let runtime = dir("no-instance-r");
        let value = compose_at(
            &units,
            &Inputs {
                instance: None,
                runtime_dir: Some(&runtime),
                config: &config(""),
                unit_result: None,
                main_pid: None,
            },
            1_700_000_000,
        );
        assert_eq!(value["state"], "unknown");
        assert_eq!(value["reason"], "no_instance");
        assert_eq!(value["profile"]["running"], Value::Null);
    }

    #[test]
    fn a_configured_profile_without_running_evidence_is_never_active() {
        let units = dir("configured-u");
        let runtime = dir("configured-r");
        write_switch(
            &units,
            "baft-tunnel@edge.service.d/50-policy-pack.conf",
            "sched",
        );
        let cfg = config(r#"{"profile":"gaming/1"}"#);
        let value = compose_with(&units, &runtime, &cfg, Some(4242), 1_700_000_000);
        assert_eq!(value["switch"]["state"], "on");
        assert_eq!(value["profile"]["configured"], "gaming/1");
        assert_eq!(value["profile"]["running"], Value::Null);
        // The feature gate decides between `off` and `unknown`; either way it is not `active`.
        if feature_compiled() {
            assert_eq!(value["state"], "unknown");
            assert_eq!(value["reason"], "no_running_evidence");
        } else {
            assert_eq!(value["state"], "off");
            assert_eq!(value["reason"], "feature_not_compiled");
        }
    }

    #[cfg(feature = "policy-pack")]
    #[test]
    fn a_fresh_matching_snapshot_is_the_only_source_of_active() {
        let units = dir("fresh-u");
        let runtime = dir("fresh-r");
        write_switch(
            &units,
            "baft-tunnel@edge.service.d/50-policy-pack.conf",
            "sched",
        );
        let at = 1_700_000_000;
        write_snapshot(&runtime, 4242, at - 1, Some("gaming/1"));
        let cfg = config(r#"{"profile":"gaming/1"}"#);
        let value = compose_with(&units, &runtime, &cfg, Some(4242), at);
        assert_eq!(value["state"], "active");
        assert_eq!(value["reason"], Value::Null);
        assert_eq!(value["profile"]["configured"], "gaming/1");
        assert_eq!(value["profile"]["running"], "gaming/1");
        assert_eq!(value["evidence"]["running"]["pid"], 4242);
    }

    #[cfg(feature = "policy-pack")]
    #[test]
    fn a_foreign_pid_or_a_stale_sample_is_not_evidence() {
        let units = dir("stale-u");
        let runtime = dir("stale-r");
        write_switch(
            &units,
            "baft-tunnel@edge.service.d/50-policy-pack.conf",
            "sched",
        );
        let cfg = config(r#"{"profile":"gaming/1"}"#);
        let at = 1_700_000_000;

        write_snapshot(&runtime, 999, at - 1, Some("gaming/1"));
        let foreign = compose_with(&units, &runtime, &cfg, Some(4242), at);
        assert_eq!(foreign["state"], "unknown");
        assert_eq!(foreign["reason"], "no_running_evidence");

        write_snapshot(&runtime, 4242, at - 10, Some("gaming/1"));
        let stale = compose_with(&units, &runtime, &cfg, Some(4242), at);
        assert_eq!(stale["state"], "unknown");
        assert_eq!(stale["reason"], "stale_running_evidence");
        assert_eq!(stale["profile"]["running"], Value::Null);
    }

    #[cfg(feature = "policy-pack")]
    #[test]
    fn a_snapshot_that_reports_a_fallback_keeps_its_cause() {
        let units = dir("fallback-u");
        let runtime = dir("fallback-r");
        write_switch(
            &units,
            "baft-tunnel@edge.service.d/50-policy-pack.conf",
            "sched",
        );
        let at = 1_700_000_000;
        write_snapshot(&runtime, 4242, at, Some("balanced/1"));
        let path = runtime.join(FILE);
        let mut body: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        body["fallback"] = json!({"cause": "guard_violation", "since_unix": at - 5});
        std::fs::write(&path, body.to_string()).unwrap();
        let cfg = config(r#"{"profile":"balanced/1"}"#);
        let value = compose_with(&units, &runtime, &cfg, Some(4242), at);
        assert_eq!(value["state"], "fallback");
        assert_eq!(value["reason"], "guard_violation");
        assert_eq!(value["fallback"]["since_unix"], at - 5);
    }

    #[test]
    fn two_sources_that_set_the_switch_are_unknown_not_a_guess() {
        let units = dir("twosrc-u");
        let runtime = dir("twosrc-r");
        write_switch(&units, "baft-tunnel@edge.service", "sched");
        write_switch(
            &units,
            "baft-tunnel@edge.service.d/50-policy-pack.conf",
            "sched",
        );
        let value = compose_with(&units, &runtime, &config(""), None, 1_700_000_000);
        assert_eq!(value["switch"]["state"], "unknown");
        assert_eq!(value["switch"]["scan"], "set_in_multiple_sources");
        assert_eq!(value["state"], "unknown");
        assert_eq!(value["reason"], "switch_set_in_multiple_sources");
    }

    #[test]
    fn a_switch_an_operator_turned_off_is_off() {
        let units = dir("off-u");
        let runtime = dir("off-r");
        write_switch(
            &units,
            "baft-tunnel@edge.service.d/50-policy-pack.conf",
            "off",
        );
        let value = compose_with(
            &units,
            &runtime,
            &config(r#"{"profile":"gaming/1"}"#),
            None,
            1_700_000_000,
        );
        assert_eq!(value["switch"]["state"], "off");
        assert_eq!(value["switch"]["value"], "off");
        if feature_compiled() {
            assert_eq!(value["state"], "off");
            assert_eq!(value["reason"], "switch_off");
        } else {
            assert_eq!(value["reason"], "feature_not_compiled");
        }
    }

    #[test]
    fn an_absent_switch_and_no_evidence_is_off_when_the_feature_is_compiled_in() {
        let units = dir("absent-u");
        let runtime = dir("absent-r");
        let value = compose_with(&units, &runtime, &config(""), None, 1_700_000_000);
        assert_eq!(value["switch"]["state"], "absent");
        assert_eq!(value["state"], "off");
        assert_eq!(
            value["reason"],
            if feature_compiled() {
                "switch_absent"
            } else {
                "feature_not_compiled"
            }
        );
    }

    #[test]
    fn every_tuple_is_unknown_and_none_is_measured() {
        let units = dir("tuple-u");
        let runtime = dir("tuple-r");
        let value = compose_with(&units, &runtime, &config(""), None, 1_700_000_000);
        let rows = value["tuples"].as_array().unwrap();
        assert_eq!(rows.len(), 5);
        for row in rows {
            assert_eq!(row["claim"], "unknown");
            assert_eq!(row["evidence"], Value::Null);
            assert_eq!(row["engine"], "tcp-tls");
        }
        assert_eq!(value["capabilities"]["datagram"], "unknown");
    }

    #[test]
    fn the_baft_engine_is_unsupported_and_never_inherits_rust_rows() {
        let units = dir("baft-u");
        let runtime = dir("baft-r");
        let cfg: RuntimeConfig = serde_json::from_value(json!({
            "mode": "serve", "listen": "127.0.0.1:8443", "target": "127.0.0.1:80",
            "local_id": "a", "peer_id": "b", "tunnel_id": "t", "direction": "direct",
            "peer_fingerprint": "00", "engine": "baft", "key": "/secret/key.pem"
        }))
        .unwrap();
        let value = compose_with(&units, &runtime, &cfg, None, 1_700_000_000);
        let rows = value["tuples"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["claim"], "unsupported");
        assert_eq!(rows[0]["engine"], "baft");
    }

    #[test]
    fn an_unobserved_metric_is_null_with_a_reason_never_zero() {
        let value = metrics_block();
        for (name, reason) in [
            ("queued_bytes", "not_sampled"),
            ("overflow_refusals", "not_sampled"),
            ("guard_violations", "not_instrumented"),
            ("queueing_delay_estimate_ms", "signal_blind"),
        ] {
            assert_eq!(value[name]["value"], Value::Null, "{name}");
            assert_eq!(value[name]["available"], false, "{name}");
            assert_eq!(value[name]["reason"], reason, "{name}");
        }
        assert_eq!(value["guard_violations"]["provenance"], "measured");
        assert_eq!(
            value["queueing_delay_estimate_ms"]["provenance"],
            "estimated"
        );
    }

    #[test]
    fn a_configured_profile_is_never_reported_as_effective_adaptive() {
        let units = dir("adaptive-u");
        let runtime = dir("adaptive-r");
        write_switch(
            &units,
            "baft-tunnel@edge.service.d/50-policy-pack.conf",
            "sched",
        );
        let cfg = config(r#"{"profile":"gaming/1","adaptive":true}"#);
        let value = compose_with(&units, &runtime, &cfg, None, 1_700_000_000);
        assert_eq!(value["adaptive"]["configured"], true);
        assert_eq!(value["adaptive"]["effective"], Value::Null);
    }
}
