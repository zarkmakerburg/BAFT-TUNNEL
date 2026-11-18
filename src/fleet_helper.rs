//! Root helper for Fleet enrollment signing (ADR-0051 M5) and its agent-side client.
//!
//! The Fleet CA key lives only in a root-only directory, so the unprivileged agent cannot sign.
//! Like the pairing helper (ADR-0050), the agent drops `CONTROL_DIR/NAME.fleet`; the root path
//! unit `baft-tunnel-control-fleet@NAME.path` runs `baft-tunnel fleet helper --instance NAME`,
//! which signs one `fleet-node` certificate with `fleet_ca::sign` and writes
//! `NAME.fleet-result`. The request carries a node id and a CSR, never key material; the result
//! carries only the public certificate. The helper never creates the CA: that is a deliberate
//! operator act, so a missing CA is a clean failure.

#![allow(dead_code)] // routed from the agent's confirm route in the same slice

use std::{
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

pub const REQUEST_SUFFIX: &str = "fleet";
pub const RESULT_SUFFIX: &str = "fleet-result";
pub const CA_DIR: &str = "/etc/baft-tunnel/fleet-ca";
const MAX_FILE_BYTES: u64 = 8192;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
enum Request {
    SignNode { node_id: String, csr: String },
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open {} (symlinks are refused)", path.display()))?;
    ensure!(file.metadata()?.is_file(), "not a regular file");
    let mut buf = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    ensure!(buf.len() as u64 <= MAX_FILE_BYTES, "file too large");
    Ok(buf)
}

fn write_result(control_dir: &Path, name: &str, value: &Value) -> Result<()> {
    let path = control_dir.join(format!("{name}.{RESULT_SUFFIX}"));
    let tmp = control_dir.join(format!(".{name}.{RESULT_SUFFIX}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(value.to_string().as_bytes())?;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        if let Some((_, gid)) = crate::setup::service_ids() {
            std::os::unix::fs::fchown(&file, None, Some(gid))?;
        }
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
    drop(file);
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// One helper run: consume the request, sign, publish the result.
pub fn run(name: &str, control_dir: &Path, ca_dir: &Path) -> Result<Value> {
    ensure!(crate::apply::valid_instance(name), "invalid instance name");
    let req_path = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let bytes = read_bounded(&req_path);
    let _ = std::fs::remove_file(&req_path);
    let outcome = bytes
        .and_then(|b| Ok(serde_json::from_slice::<Request>(&b)?))
        .and_then(|Request::SignNode { node_id, csr }| {
            ensure!(
                ca_dir.join("ca.crt").is_file(),
                "no Fleet CA is initialized on this host"
            );
            crate::fleet_ca::sign(ca_dir, &node_id, &csr)
        });
    let result = match outcome {
        Ok(cert) => json!({"state": "done", "certificate": cert}),
        // The reason never contains the CSR or any key: errors come from fixed checks.
        Err(e) => json!({"state": "failed", "reason": format!("{e:#}")}),
    };
    write_result(control_dir, name, &result)?;
    Ok(result)
}

/// Agent side: ask the root helper to sign `csr` for `node_id` and wait for its answer.
pub fn sign(
    control_dir: &Path,
    name: &str,
    node_id: &str,
    csr: &str,
    wait: Duration,
) -> Result<String, String> {
    if !crate::apply::valid_instance(name) || !crate::fleet_ca::valid_node_id(node_id) {
        return Err("invalid instance or node id".into());
    }
    if !control_dir.is_dir() {
        return Err("the root Fleet helper is not installed".into());
    }
    let result_path = control_dir.join(format!("{name}.{RESULT_SUFFIX}"));
    let _ = std::fs::remove_file(&result_path);
    let req_path = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o660)
        .open(&req_path)
        .map_err(|e| format!("a signing request is already pending or cannot be written: {e}"))?;
    file.write_all(
        json!({"op": "sign_node", "node_id": node_id, "csr": csr})
            .to_string()
            .as_bytes(),
    )
    .map_err(|e| format!("cannot write the signing request: {e}"))?;
    drop(file);
    let started = Instant::now();
    while started.elapsed() < wait {
        if let Ok(bytes) = read_bounded(&result_path) {
            let _ = std::fs::remove_file(&result_path);
            let v: Value = serde_json::from_slice(&bytes).map_err(|_| "unreadable result")?;
            return match (v["state"].as_str(), v["certificate"].as_str()) {
                (Some("done"), Some(cert)) => Ok(cert.to_string()),
                _ => Err(v["reason"].as_str().unwrap_or("signing failed").to_string()),
            };
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&req_path);
    Err("the root Fleet helper did not answer in time".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let p =
            std::env::temp_dir().join(format!("fleet-helper-{tag}-{:x}", rand::random::<u64>()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn missing_ca_and_bad_requests_fail_closed() {
        let dir = tmp("a");
        std::fs::write(
            dir.join("t.fleet"),
            b"{\"op\":\"sign_node\",\"node_id\":\"edge-1\",\"csr\":\"x\"}",
        )
        .unwrap();
        let v = run("t", &dir, &dir.join("no-ca")).unwrap();
        assert_eq!(v["state"], "failed");
        assert!(v["reason"].as_str().unwrap().contains("no Fleet CA"));
        assert!(!dir.join("t.fleet").exists(), "request is consumed");
        std::fs::write(dir.join("t.fleet"), b"{\"op\":\"nope\"}").unwrap();
        assert_eq!(
            run("t", &dir, &dir.join("no-ca")).unwrap()["state"],
            "failed"
        );
        assert!(run("../x", &dir, &dir).is_err());
    }

    #[test]
    fn agent_signs_through_the_helper_with_a_real_ca() {
        let dir = tmp("b");
        let ca = dir.join("ca");
        crate::fleet_ca::create(&ca, "Test Fleet CA").unwrap();
        // A real CSR for CN=edge-1 (EC P-256), as the node would generate.
        let key = crate::pair_ca::run(
            &[
                "genpkey",
                "-algorithm",
                "EC",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
            ],
            None,
        )
        .unwrap();
        let kp = dir.join("k.pem");
        std::fs::write(&kp, key).unwrap();
        let csr = String::from_utf8(
            crate::pair_ca::run(
                &[
                    "req",
                    "-new",
                    "-key",
                    &kp.to_string_lossy(),
                    "-subj",
                    "/CN=edge-1",
                ],
                None,
            )
            .unwrap(),
        )
        .unwrap();
        let (d2, ca2, csr2) = (dir.clone(), ca.clone(), csr.clone());
        let agent =
            std::thread::spawn(move || sign(&d2, "t", "edge-1", &csr2, Duration::from_secs(20)));
        // Play the root path unit: wait for the request, run the helper once.
        let started = Instant::now();
        while !dir.join("t.fleet").exists() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(run("t", &dir, &ca2).unwrap()["state"], "done");
        let cert = agent.join().unwrap().unwrap();
        assert!(cert.contains("BEGIN CERTIFICATE"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
