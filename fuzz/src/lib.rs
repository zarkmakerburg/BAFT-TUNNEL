//! Fuzz harness: compiles the runtime's own source files (no copies) as modules of this crate so the
//! parsers can be exercised without turning the binary crate into a library.
#![allow(dead_code, unused_imports)]

#[path = "../../src/agent.rs"]
pub mod agent;
#[path = "../../src/apply.rs"]
pub mod apply;
#[path = "../../src/authorization.rs"]
pub mod authorization;
#[path = "../../src/baft_engine.rs"]
pub mod baft_engine;
#[path = "../../src/capabilities.rs"]
pub mod capabilities;
#[path = "../../src/config.rs"]
pub mod config;
#[path = "../../src/control_socket.rs"]
pub mod control_socket;
#[path = "../../src/engine.rs"]
pub mod engine;
#[path = "../../src/engine_store.rs"]
pub mod engine_store;
#[path = "../../src/handshake.rs"]
pub mod handshake;
#[path = "../../src/history_store.rs"]
pub mod history_store;
#[path = "../../src/integrity_probe.rs"]
pub mod integrity_probe;
#[path = "../../src/key_custody.rs"]
pub mod key_custody;
#[path = "../../src/link_policy.rs"]
pub mod link_policy;
#[path = "../../src/policy_profile.rs"]
pub mod policy_profile;
#[path = "../../src/noise.rs"]
pub mod noise;
#[path = "../../src/observe.rs"]
pub mod observe;
#[path = "../../src/fleet.rs"]
#[allow(dead_code)]
pub mod fleet;
#[path = "../../src/fleet_api.rs"]
#[allow(dead_code)]
pub mod fleet_api;
#[path = "../../src/fleet_ca.rs"]
#[allow(dead_code)]
pub mod fleet_ca;
#[path = "../../src/fleet_enroll.rs"]
#[allow(dead_code)]
pub mod fleet_enroll;
#[path = "../../src/fleet_helper.rs"]
#[allow(dead_code)]
pub mod fleet_helper;
#[path = "../../src/fleet_hub.rs"]
#[allow(dead_code)]
pub mod fleet_hub;
#[path = "../../src/fleet_report.rs"]
#[allow(dead_code)]
pub mod fleet_report;
#[path = "../../src/fleet_run.rs"]
#[allow(dead_code)]
pub mod fleet_run;
#[path = "../../src/probe_adapters.rs"]
#[allow(dead_code)]
pub mod probe_adapters;
#[path = "../../src/probe_api.rs"]
#[allow(dead_code)]
pub mod probe_api;
#[path = "../../src/probe_http.rs"]
#[allow(dead_code)]
pub mod probe_http;
#[path = "../../src/geoip.rs"]
pub mod geoip;
#[path = "../../src/target_guard.rs"]
#[allow(dead_code)]
pub mod target_guard;
#[path = "../../src/pair_ca.rs"]
pub mod pair_ca;
#[path = "../../src/pair_flow.rs"]
pub mod pair_flow;
#[path = "../../src/pair_helper.rs"]
pub mod pair_helper;
#[path = "../../src/pair_proto.rs"]
pub mod pair_proto;
#[path = "../../src/pki_helper.rs"]
pub mod pki_helper;
#[path = "../../src/resumable.rs"]
pub mod resumable;
#[path = "../../src/resume_core.rs"]
pub mod resume_core;
#[path = "../../src/resume_registry.rs"]
pub mod resume_registry;
#[path = "../../src/resume_stream.rs"]
pub mod resume_stream;
#[path = "../../src/resume_tunnel.rs"]
pub mod resume_tunnel;
#[path = "../../src/resume_wire.rs"]
pub mod resume_wire;
#[path = "../../src/runtime_host.rs"]
pub mod runtime_host;
#[path = "../../src/transport_health.rs"]
pub mod transport_health;
#[path = "../../src/session_status.rs"]
pub mod session_status;
#[path = "../../src/setup.rs"]
pub mod setup;
#[path = "../../src/setup_helper.rs"]
pub mod setup_helper;
#[path = "../../src/tunnel_helper.rs"]
#[allow(dead_code)]
pub mod tunnel_helper;
#[path = "../../src/tls.rs"]
pub mod tls;
#[path = "../../src/tunnel.rs"]
pub mod tunnel;

/// Untrusted JSON configuration: must never panic, whatever the bytes.
pub fn config_json(data: &[u8]) {
    if let Ok(config) = serde_json::from_slice::<config::RuntimeConfig>(data) {
        let _ = config.plan();
        let _ = config.support_report();
        let _ = config.engine_name();
    }
}

/// A pairing bundle from the peer host (untrusted file).
pub fn pairing_bundle(data: &[u8]) {
    if let Ok(bundle) = serde_json::from_slice::<setup::Bundle>(data) {
        let _ = bundle.checksum();
    }
}

/// Bytes arriving on the local API port, read by the real request reader (`read_request`).
///
/// Unlike `agent_http`, which starts at routing, this drives framing and header validation
/// themselves: the bytes are written into an in-memory duplex stream and read back by
/// `read_request`, which asserts the S5-AUD-01 invariant (no accepted field line carries SP or
/// HTAB in its name) for every head it accepts.
pub fn read_request(data: &[u8]) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        use tokio::io::AsyncWriteExt;
        // Capacity is at least the whole input, so the write cannot block on an unread buffer:
        // the reader may stop as soon as it has a head and the declared body.
        let (mut ours, mut theirs) = tokio::io::duplex(data.len() + 1);
        let _ = theirs.write_all(data).await;
        let _ = theirs.shutdown().await;
        drop(theirs);
        let outcome = agent::fuzz_read_request(&mut ours).await;
        std::hint::black_box(outcome);
    });
}

/// Raw HTTP head and body arriving on the local API port.
pub fn agent_http(data: &[u8]) {
    let split = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    let head = String::from_utf8_lossy(&data[..split]).into_owned();
    let body = data.get(split + 1..).unwrap_or_default();
    let _ = agent::fuzz_route(&head, body);
}

/// Bytes arriving on the tunnel port before any authorization is complete.
pub fn handshake_server(data: &[u8]) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let (mut ours, mut theirs) = tokio::io::duplex(8192);
        let feed = data.to_vec();
        let writer = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let _ = theirs.write_all(&feed).await;
            let _ = theirs.shutdown().await;
            theirs
        });
        let _ = handshake::server_handshake(
            &mut ours,
            "node-a",
            "node-b",
            "tunnel-1",
            authorization::ConnectionDirection::Direct,
        )
        .await;
        let _ = handshake::server_stream_open(
            &mut ours,
            "tunnel-1",
            authorization::ConnectionDirection::Direct,
        )
        .await;
        let _ = writer.await;
    });
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("baft-fuzz-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Authenticated routes that parse a request body (plan, preflight, bundle inspect, desired, verify).
pub fn agent_authed(data: &[u8]) {
    let (selector, body) = data.split_first().map_or((0, &[][..]), |(s, b)| (*s, b));
    let dir = scratch_dir("authed");
    let _ = agent::fuzz_authed(selector, body, &dir);
}

/// The Apply request body and the plan inside it.
pub fn apply_body(data: &[u8]) {
    agent::fuzz_apply_body(data);
}

/// Runtime evidence files a unit publishes (session, counters, last failure) as the agent reads them.
pub fn evidence_files(data: &[u8]) {
    let dir = scratch_dir("evidence");
    for name in [
        session_status::FILE,
        session_status::COUNTERS_FILE,
        session_status::FAILURE_FILE,
        session_status::HEALTH_FILE,
    ] {
        let _ = std::fs::write(dir.join(name), data);
    }
    let ev = observe::read(&dir, None);
    if let Some(c) = ev.counters.as_ref().and_then(observe::sample) {
        let _ = observe::rates(&[c, c]);
    }
}

/// The persisted history file as read back on agent start.
pub fn history_file(data: &[u8]) {
    let dir = scratch_dir("history");
    let _ = std::fs::write(dir.join("history.json"), data);
    let h = history_store::LongHistory::open(Some(&dir), 1_700_000_000);
    let _ = h.view(1_700_000_000);
    let _ = h.alerts(1_700_000_000);
}

/// State-directory files read back by `/v1/update` and `/v1/host/tunnels`.
pub fn state_files(data: &[u8]) {
    let dir = scratch_dir("statefiles");
    agent::fuzz_state_files(data, &dir);
}
