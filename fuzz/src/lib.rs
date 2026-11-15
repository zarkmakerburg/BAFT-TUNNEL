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
#[path = "../../src/integrity_probe.rs"]
pub mod integrity_probe;
#[path = "../../src/key_custody.rs"]
pub mod key_custody;
#[path = "../../src/noise.rs"]
pub mod noise;
#[path = "../../src/pki_helper.rs"]
pub mod pki_helper;
#[path = "../../src/observe.rs"]
pub mod observe;
#[path = "../../src/runtime_host.rs"]
pub mod runtime_host;
#[path = "../../src/session_status.rs"]
pub mod session_status;
#[path = "../../src/setup.rs"]
pub mod setup;
#[path = "../../src/setup_helper.rs"]
pub mod setup_helper;
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
