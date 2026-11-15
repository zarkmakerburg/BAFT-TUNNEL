//! CodSpeed benchmarks for the BAFT-TUNNEL hot paths.
//!
//! Like the fuzz harness (fuzz/src/lib.rs), this compiles the runtime's own source files (no copies)
//! as modules of the benchmark crate, so the binary crate does not have to become a library.
#![allow(dead_code, unused_imports, clippy::all)]

#[path = "../src/agent.rs"]
mod agent;
#[path = "../src/apply.rs"]
mod apply;
#[path = "../src/authorization.rs"]
mod authorization;
#[path = "../src/baft_engine.rs"]
mod baft_engine;
#[path = "../src/capabilities.rs"]
mod capabilities;
#[path = "../src/config.rs"]
mod config;
#[path = "../src/control_socket.rs"]
mod control_socket;
#[path = "../src/engine.rs"]
mod engine;
#[path = "../src/engine_store.rs"]
mod engine_store;
#[path = "../src/handshake.rs"]
mod handshake;
#[path = "../src/integrity_probe.rs"]
mod integrity_probe;
#[path = "../src/key_custody.rs"]
mod key_custody;
#[path = "../src/noise.rs"]
mod noise;
#[path = "../src/observe.rs"]
mod observe;
#[path = "../src/pki_helper.rs"]
mod pki_helper;
#[path = "../src/runtime_host.rs"]
mod runtime_host;
#[path = "../src/session_status.rs"]
mod session_status;
#[path = "../src/setup.rs"]
mod setup;
#[path = "../src/setup_helper.rs"]
mod setup_helper;
#[path = "../src/tls.rs"]
mod tls;
#[path = "../src/tunnel.rs"]
mod tunnel;

use std::{path::PathBuf, sync::LazyLock};

use authorization::{ConnectionDirection, PHASE1_CAPABILITIES, StreamCapability};
use divan::{Bencher, black_box};
use rustls::pki_types::CertificateDer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn main() {
    divan::main();
}

const FORWARD_CONFIG: &str = include_str!("../examples/forward.json");

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

mod config_bench {
    use super::*;

    #[divan::bench]
    fn parse_forward() -> config::RuntimeConfig {
        serde_json::from_str(black_box(FORWARD_CONFIG)).expect("forward config parses")
    }

    #[divan::bench]
    fn plan(bencher: Bencher) {
        let config: config::RuntimeConfig = serde_json::from_str(FORWARD_CONFIG).unwrap();
        bencher.bench_local(|| black_box(&config).plan());
    }
}

mod setup_bench {
    use super::*;

    fn request() -> setup::PlanRequest {
        setup::PlanRequest {
            engine: config::EngineKind::TcpTls,
            role: setup::Role::Forward,
            direction: ConnectionDirection::Direct,
            protocol_version: authorization::PROTOCOL_V1,
            node_id: "node-iran".into(),
            peer_id: "node-abroad".into(),
            tunnel_id: "demo-tunnel".into(),
            service_port: 18080,
            tunnel_port: 8443,
            peer_host: Some("203.0.113.10".into()),
            server_name: Some("tunnel.example.test".into()),
            dir: PathBuf::from("/etc/baft-tunnel"),
            instance: None,
        }
    }

    fn bundle() -> setup::Bundle {
        setup::Bundle {
            version: 1,
            tunnel_id: "demo-tunnel".into(),
            engine: "tcp-tls".into(),
            direction: "direct".into(),
            protocol_version: authorization::PROTOCOL_V1,
            role: setup::Role::Serve,
            node_id: "node-abroad".into(),
            peer_id: "node-iran".into(),
            service_port: 18080,
            tunnel_port: 8443,
            public: "ab".repeat(32),
            server_name: Some("tunnel.example.test".into()),
            engine_sha256: None,
        }
    }

    #[divan::bench]
    fn plan(bencher: Bencher) {
        let req = request();
        bencher.bench_local(|| setup::plan(black_box(&req)).expect("valid plan"));
    }

    #[divan::bench]
    fn bundle_checksum(bencher: Bencher) {
        let bundle = bundle();
        bencher.bench_local(|| black_box(&bundle).checksum());
    }

    #[divan::bench]
    fn bundle_parse(bencher: Bencher) {
        let json = serde_json::to_string(&bundle()).unwrap();
        bencher.bench_local(|| {
            serde_json::from_str::<setup::Bundle>(black_box(&json)).expect("bundle parses")
        });
    }
}

mod handshake_bench {
    use super::*;

    #[divan::bench]
    fn validate_identity() {
        handshake::validate_identity(black_box("node-iran_1.example")).unwrap();
    }

    #[divan::bench]
    fn validate_fingerprint_rollover() {
        let pins = concat!(
            "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99",
            ",",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        handshake::validate_fingerprint(black_box(pins)).unwrap();
    }

    #[divan::bench]
    fn authorize_session() {
        authorization::authorize_session(
            black_box("node-iran"),
            "node-iran",
            black_box("demo-tunnel"),
            "demo-tunnel",
            ConnectionDirection::Direct,
            ConnectionDirection::Direct,
            black_box(&PHASE1_CAPABILITIES),
        )
        .unwrap();
    }

    /// Full client/server protocol handshake plus one stream-open over an in-memory pipe.
    #[divan::bench]
    fn protocol_handshake_and_stream_open(bencher: Bencher) {
        let rt = runtime();
        bencher.bench_local(|| {
            rt.block_on(async {
                let (mut client, mut server) = tokio::io::duplex(8192);
                let server_side = async {
                    handshake::server_handshake(
                        &mut server,
                        "node-abroad",
                        "node-iran",
                        "demo-tunnel",
                        ConnectionDirection::Direct,
                    )
                    .await?;
                    handshake::server_stream_open(
                        &mut server,
                        "demo-tunnel",
                        ConnectionDirection::Direct,
                    )
                    .await
                };
                let client_side = async {
                    handshake::client_handshake(
                        &mut client,
                        "node-iran",
                        "node-abroad",
                        "demo-tunnel",
                        ConnectionDirection::Direct,
                    )
                    .await?;
                    handshake::client_stream_open(
                        &mut client,
                        "demo-tunnel",
                        ConnectionDirection::Direct,
                        StreamCapability::TcpForward,
                    )
                    .await
                };
                let (s, c) = tokio::join!(server_side, client_side);
                s.unwrap();
                c.unwrap();
            })
        });
    }
    /// Full client/server hello handshake only, without the follow-up StreamOpen frame.
    #[divan::bench]
    fn protocol_handshake_only(bencher: Bencher) {
        let rt = runtime();
        bencher.bench_local(|| {
            rt.block_on(async {
                let (mut client, mut server) = tokio::io::duplex(4096);
                let server_side = handshake::server_handshake(
                    &mut server,
                    "node-abroad",
                    "node-iran",
                    "demo-tunnel",
                    ConnectionDirection::Direct,
                );
                let client_side = handshake::client_handshake(
                    &mut client,
                    "node-iran",
                    "node-abroad",
                    "demo-tunnel",
                    ConnectionDirection::Direct,
                );
                let (s, c) = tokio::join!(server_side, client_side);
                s.unwrap();
                c.unwrap();
            })
        });
    }

    /// Baseline validation for one SHA-256 peer pin, separate from rollover validation.
    #[divan::bench]
    fn validate_fingerprint_single_pin() {
        let pin = "ab".repeat(32);
        handshake::validate_fingerprint(black_box(&pin)).unwrap();
    }
}

mod peer_certificate_bench {
    use super::*;
    use rcgen::{CertificateParams, KeyPair, SanType, string::Ia5String};
    use sha2::{Digest, Sha256};

    static PEER_CERT: LazyLock<CertificateDer<'static>> = LazyLock::new(|| {
        let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        params.subject_alt_names.push(SanType::URI(
            Ia5String::try_from("urn:baft-tunnel:node:node-abroad".to_owned()).unwrap(),
        ));
        let key = KeyPair::generate().unwrap();
        params.self_signed(&key).unwrap().der().clone()
    });

    #[divan::bench]
    fn verify_identity(bencher: Bencher) {
        let certs = [PEER_CERT.clone()];
        bencher.bench_local(|| {
            handshake::verify_peer_identity(Some(black_box(&certs[..])), "node-abroad").unwrap()
        });
    }

    #[divan::bench]
    fn verify_fingerprint(bencher: Bencher) {
        let certs = [PEER_CERT.clone()];
        let pin = hex::encode(Sha256::digest(certs[0].as_ref()));
        bencher.bench_local(|| {
            handshake::verify_peer_fingerprint(Some(black_box(&certs[..])), black_box(&pin))
                .unwrap()
        });
    }
}

mod noise_bench {
    use super::*;

    const PROLOGUE: &[u8] = b"baft-tunnel/data-plane/v1|node-a|node-b|tunnel-1";

    struct Keys {
        initiator: snow::Keypair,
        responder: snow::Keypair,
    }

    fn keys() -> Keys {
        Keys {
            initiator: noise::generate_static_keypair().unwrap(),
            responder: noise::generate_static_keypair().unwrap(),
        }
    }

    /// Noise IK handshake, then `size` bytes sent through the encrypted stream and echoed back.
    fn round_trip(rt: &tokio::runtime::Runtime, keys: &Keys, size: usize) {
        rt.block_on(async {
            let (left, right) = tokio::io::duplex(128 * 1024);
            let responder_private = keys.responder.private.clone();
            let initiator_public = keys.initiator.public.clone();
            let server = tokio::spawn(async move {
                let mut secure =
                    noise::accept(right, &responder_private, &initiator_public, PROLOGUE)
                        .await
                        .unwrap();
                let mut buffer = vec![0_u8; size];
                secure.read_exact(&mut buffer).await.unwrap();
                secure.write_all(&buffer).await.unwrap();
                secure.flush().await.unwrap();
            });
            let mut secure = noise::initiate(
                left,
                &keys.initiator.private,
                &keys.responder.public,
                PROLOGUE,
            )
            .await
            .unwrap();
            let payload = vec![0x5a_u8; size];
            secure.write_all(&payload).await.unwrap();
            let mut echoed = vec![0_u8; size];
            secure.read_exact(&mut echoed).await.unwrap();
            assert_eq!(echoed, payload);
            server.await.unwrap();
        });
    }

    #[divan::bench(args = [16, 64 * 1024])]
    fn ik_handshake_and_echo(bencher: Bencher, size: usize) {
        let rt = runtime();
        let keys = keys();
        bencher.bench_local(|| round_trip(&rt, &keys, size));
    }
}

mod agent_bench {
    use super::*;

    #[divan::bench]
    fn engines_catalog() -> serde_json::Value {
        capabilities::catalog()
    }
}
