#[allow(dead_code, unused_imports)]
#[path = "../src/authorization.rs"]
mod authorization;
#[allow(dead_code, unused_imports)]
#[path = "../src/handshake.rs"]
mod handshake;

use authorization::{ConnectionDirection, StreamCapability, authorize_session};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use futures::executor::block_on;

fn protocol_handshake_roundtrip(c: &mut Criterion) {
    c.bench_function("protocol/hello_roundtrip", |b| {
        b.iter(|| {
            block_on(async {
                let (mut client, mut server) = tokio::io::duplex(4096);
                let (client_result, server_result) = tokio::join!(
                    handshake::client_handshake(
                        &mut client,
                        "iran-01",
                        "abroad-01",
                        "benchmark-tunnel",
                        ConnectionDirection::Direct,
                    ),
                    handshake::server_handshake(
                        &mut server,
                        "abroad-01",
                        "iran-01",
                        "benchmark-tunnel",
                        ConnectionDirection::Direct,
                    ),
                );
                client_result.expect("client protocol handshake");
                server_result.expect("server protocol handshake");
            });
        })
    });
}

fn authorization_checks(c: &mut Criterion) {
    let capabilities = [StreamCapability::TcpForward];
    c.bench_function("authorization/session_accept", |b| {
        b.iter(|| {
            authorize_session(
                black_box("iran-01"),
                black_box("iran-01"),
                black_box("benchmark-tunnel"),
                black_box("benchmark-tunnel"),
                ConnectionDirection::Direct,
                ConnectionDirection::Direct,
                black_box(&capabilities),
            )
            .expect("valid enrolled session");
        })
    });
}

fn protocol_input_validation(c: &mut Criterion) {
    let fingerprint = "ab".repeat(32);
    c.bench_function("protocol/fingerprint_validation", |b| {
        b.iter(|| {
            handshake::validate_fingerprint(black_box(&fingerprint))
                .expect("valid SHA-256 peer pin");
        })
    });
}

criterion_group!(
    benches,
    protocol_handshake_roundtrip,
    authorization_checks,
    protocol_input_validation
);
criterion_main!(benches);
