//! Root pairing helper and its agent-side client (ADR-0050, stage 5).
//!
//! The agent drops `CONTROL_DIR/NAME.pair` (`offer` or `accept`); the root path unit
//! `baft-tunnel-control-pair@NAME.path` runs `baft-tunnel pair helper --instance NAME`, which
//! consumes the request, runs the same `pair_flow` as the CLI for `/etc/baft-tunnel` and NAME only,
//! and reports progress in `NAME.pair-status` (state, the one-time code while waiting, the joining
//! server for approval, or a failure reason). The user's approval arrives as `NAME.pair-approve`.
//! No key, CA material or bundle is ever written to these files; the code is a 15-minute,
//! single-use secret shown only to the authenticated panel.

use std::{
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

pub const REQUEST_SUFFIX: &str = "pair";
pub const STATUS_SUFFIX: &str = "pair-status";
pub const APPROVE_SUFFIX: &str = "pair-approve";
const MAX_FILE_BYTES: u64 = 4096;
const APPROVAL_WAIT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
pub enum PairRequest {
    Offer {
        name: String,
        service_port: u16,
        #[serde(default = "default_tunnel_port")]
        tunnel_port: u16,
        #[serde(default)]
        host: Option<String>,
        #[serde(default)]
        direction: crate::pair_flow::Direction,
    },
    Accept {
        code: String,
        #[serde(default)]
        location: Option<crate::pair_flow::Location>,
    },
}

fn default_tunnel_port() -> u16 {
    8443
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open {} (symlinks are refused)", path.display()))?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "{} is not a regular file", path.display());
    let mut buf = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    ensure!(
        buf.len() as u64 <= MAX_FILE_BYTES,
        "{} is too large",
        path.display()
    );
    Ok(buf)
}

/// Atomically replace `control_dir/file` (root: 0640, group of the service user).
fn write_status(control_dir: &Path, name: &str, value: &Value) -> Result<()> {
    let path = control_dir.join(format!("{name}.{STATUS_SUFFIX}"));
    let tmp = control_dir.join(format!(".{name}.{STATUS_SUFFIX}.tmp"));
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

/// Wait for the user's decision on this exact join request: the approval file must carry the
/// nonce published in the `approval` status, so a stale or foreign file cannot approve it.
async fn approved(control_dir: &Path, name: &str, nonce: &str) -> bool {
    let path = control_dir.join(format!("{name}.{APPROVE_SUFFIX}"));
    let started = Instant::now();
    while started.elapsed() < APPROVAL_WAIT {
        if let Ok(bytes) = read_bounded(&path) {
            let _ = std::fs::remove_file(&path);
            return serde_json::from_slice::<Value>(&bytes)
                .is_ok_and(|v| v["decision"] == "approve" && v["nonce"] == nonce);
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

/// One helper run (the fixed command of `baft-tunnel-control-pair@NAME.service`).
pub async fn run(name: &str, control_dir: &Path, config_dir: &Path) -> Result<Value> {
    ensure!(crate::apply::valid_instance(name), "invalid instance name");
    let req_path = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let bytes = read_bounded(&req_path);
    let _ = std::fs::remove_file(&req_path);
    let _ = std::fs::remove_file(control_dir.join(format!("{name}.{APPROVE_SUFFIX}")));
    let request: PairRequest = match bytes.and_then(|b| Ok(serde_json::from_slice(&b)?)) {
        Ok(r) => r,
        Err(e) => {
            let v = json!({"state": "failed", "reason": format!("invalid pairing request: {e:#}")});
            write_status(control_dir, name, &v)?;
            return Ok(v);
        }
    };
    let instance = Some(name.to_string());
    let result = match request {
        PairRequest::Offer { name: tunnel, service_port, tunnel_port, host, direction } => {
            crate::pair_flow::offer(
                crate::pair_flow::OfferOptions {
                    dir: config_dir.to_path_buf(),
                    instance,
                    name: tunnel,
                    service_port,
                    tunnel_port,
                    host,
                    direction,
                },
                |p: crate::pair_flow::Pending| async move {
                    let nonce = hex::encode(rand::random::<[u8; 8]>());
                    let _ = write_status(
                        control_dir,
                        name,
                        &json!({"state": "approval", "nonce": nonce, "verify": p.verify,
                                "peer": {"node_id": p.node_id, "address": p.peer.to_string()}}),
                    );
                    approved(control_dir, name, &nonce).await
                },
                |code| {
                    let _ = write_status(
                        control_dir,
                        name,
                        &json!({"state": "waiting", "code": code, "port": tunnel_port, "expires_in": crate::pair_proto::CODE_TTL_SECS}),
                    );
                },
            )
            .await
        }
        PairRequest::Accept { code, location } => {
            let _ = write_status(control_dir, name, &json!({"state": "working"}));
            crate::pair_flow::accept(&code, config_dir, instance, location, |v| {
                let _ = write_status(control_dir, name, &json!({"state": "working", "verify": v}));
            })
            .await
        }
    };
    let status = match result {
        Ok(()) => json!({"state": "done"}),
        Err(e) => json!({"state": "failed", "reason": format!("{e:#}")}),
    };
    write_status(control_dir, name, &status)?;
    Ok(status)
}

/// Agent side: submit a request (refuses when one is pending) and read the status.
pub fn submit(control_dir: &Path, name: &str, request: &Value) -> Result<(), String> {
    if !crate::apply::valid_instance(name) {
        return Err("invalid instance".into());
    }
    if !control_dir.is_dir()
        || !Path::new("/etc/systemd/system/baft-tunnel-control-pair@.service").is_file()
            && control_dir == Path::new(crate::apply::CONTROL_DIR)
    {
        return Err(format!(
            "the root pairing helper is not installed; run sudo scripts/install-agent-control.sh --instance {name}"
        ));
    }
    let _ = std::fs::remove_file(control_dir.join(format!("{name}.{STATUS_SUFFIX}")));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o660)
        .open(control_dir.join(format!("{name}.{REQUEST_SUFFIX}")))
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                "another pairing request is still pending".to_string()
            } else {
                format!("cannot write the pairing request: {e}")
            }
        })?;
    file.write_all(request.to_string().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("cannot write the pairing request: {e}"))
}

pub fn status(control_dir: &Path, name: &str) -> Value {
    match read_bounded(&control_dir.join(format!("{name}.{STATUS_SUFFIX}"))) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or(json!({"state": "idle"})),
        Err(_) => json!({"state": "idle"}),
    }
}

/// Record the user's decision for the join request currently awaiting approval (`nonce` comes from
/// the `approval` status). Refused when no approval is pending or the nonce differs.
pub fn approve(control_dir: &Path, name: &str, approve: bool, nonce: &str) -> Result<(), String> {
    let current = status(control_dir, name);
    if current["state"] != "approval" || current["nonce"] != nonce {
        return Err("no matching approval is pending".into());
    }
    let path = control_dir.join(format!("{name}.{APPROVE_SUFFIX}"));
    let _ = std::fs::remove_file(&path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o660)
        .open(&path)
        .map_err(|e| format!("cannot write the approval: {e}"))?;
    file.write_all(
        json!({"decision": if approve { "approve" } else { "deny" }, "nonce": nonce})
            .to_string()
            .as_bytes(),
    )
    .map_err(|e| format!("cannot write the approval: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::Arc};

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("pair-helper-{tag}-{:x}", rand::random::<u64>()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn helper_offer_and_accept_round_trip_through_request_files() {
        // The helper adopts files for the service user, which only exists on installed hosts.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let ctl = Arc::new(tmp("ctl"));
        let (da, db) = (tmp("a"), tmp("b"));
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        submit(&ctl, "ta", &json!({"op": "offer", "name": "demo", "service_port": 9000, "tunnel_port": port, "host": "127.0.0.1"})).unwrap();
        assert!(
            submit(
                &ctl,
                "ta",
                &json!({"op": "offer", "name": "x", "service_port": 9000})
            )
            .is_err(),
            "one pending request at a time"
        );
        let (c1, d1) = (ctl.clone(), da.clone());
        let offer = tokio::spawn(async move { run("ta", &c1, &d1).await.unwrap() });
        let code = loop {
            let s = status(&ctl, "ta");
            if s["state"] == "waiting" {
                break s["code"].as_str().unwrap().to_string();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        submit(&ctl, "tb", &json!({"op": "accept", "code": code})).unwrap();
        let (c2, d2) = (ctl.clone(), db.clone());
        let accept = tokio::spawn(async move { run("tb", &c2, &d2).await.unwrap() });
        loop {
            let st = status(&ctl, "ta");
            if st["state"] == "approval" {
                assert!(approve(&ctl, "ta", true, "wrong-nonce").is_err());
                assert_eq!(st["verify"].as_str().unwrap().len(), 14);
                approve(&ctl, "ta", true, st["nonce"].as_str().unwrap()).unwrap();
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(offer.await.unwrap()["state"], "done");
        assert_eq!(accept.await.unwrap()["state"], "done");
        assert!(da.join("ta.json").exists() && db.join("tb.json").exists());
        // the status file never carries key material
        let text = std::fs::read_to_string(ctl.join("ta.pair-status")).unwrap();
        assert!(!text.contains("PRIVATE") && !text.contains("token"));
    }

    #[tokio::test]
    async fn invalid_or_unknown_requests_fail_closed() {
        let ctl = tmp("bad");
        std::fs::write(
            ctl.join("t1.pair"),
            br#"{"op":"offer","name":"x","service_port":9000,"evil":1}"#,
        )
        .unwrap();
        let r = run("t1", &ctl, &tmp("c")).await.unwrap();
        assert_eq!(r["state"], "failed");
        assert!(!ctl.join("t1.pair").exists(), "request is consumed");
        assert!(run("../x", &ctl, &tmp("d")).await.is_err());
    }
}
