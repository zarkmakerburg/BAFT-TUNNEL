//! Root helper for Fleet certificate signing and Hub identity provisioning (ADR-0051 M5).
//!
//! The Fleet CA key lives only in a root-only directory, so the unprivileged agent cannot sign.
//! Like the pairing helper (ADR-0050), the agent drops `CONTROL_DIR/NAME.fleet`; the root path
//! unit `baft-tunnel-control-fleet@NAME.path` runs `baft-tunnel fleet helper --instance NAME`.
//! The helper signs `fleet-node` CSRs and can provision a `fleet-hub` identity: Hub key material
//! is generated inside the helper, retained as a root-only per-instance systemd credential source,
//! and never appears in the request or result. Only the public certificate is returned. The
//! helper never creates the CA: that is a deliberate operator act, so a missing CA is a clean
//! failure.

#![allow(dead_code)] // routed from the agent's confirm route in the same slice

use std::{
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};

use zeroize::Zeroizing;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

pub const REQUEST_SUFFIX: &str = "fleet";
pub const RESULT_SUFFIX: &str = "fleet-result";
pub const CA_DIR: &str = "/etc/baft-tunnel/fleet-ca";
/// Per-instance Hub private keys are systemd credentials, never ordinary agent-readable files.
pub const HUB_ROOT: &str = "/etc/baft-tunnel/fleet-hub";
pub const FLEET_CONFIG_ROOT: &str = "/etc/baft-tunnel/fleet";
pub const AGENT_SYSTEMD_ROOT: &str = "/etc/systemd/system";
pub const HUB_CREDENTIAL_ID: &str = "fleet-hub-key";
const MAX_FILE_BYTES: u64 = 8192;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
enum Request {
    SignNode {
        node_id: String,
        csr: String,
    },
    /// The root helper creates the Hub key locally; the agent never supplies or receives it.
    ProvisionHub {
        hub_name: String,
        dial_san: String,
        listen: String,
    },
    /// Restart only this instance's Agent after a Hub config has been installed.
    RestartAgent,
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

fn ensure_real_directory(path: &Path, create: bool, mode: u32) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                meta.file_type().is_dir(),
                "refusing a symlink or non-directory: {}",
                path.display()
            );
            ensure!(
                std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777 == mode,
                "directory permissions must be {:04o}: {}",
                mode,
                path.display()
            );
            // Production helper runs as root. Keep tests usable as an unprivileged process.
            if unsafe { libc::geteuid() } == 0 {
                ensure!(
                    std::os::unix::fs::MetadataExt::uid(&meta) == 0,
                    "directory must be root-owned: {}",
                    path.display()
                );
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
            std::fs::create_dir(path).with_context(|| format!("create {}", path.display()))?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        }
        Err(e) => return Err(e).with_context(|| format!("inspect {}", path.display())),
    }
    Ok(())
}

fn write_managed_file(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut created = false;
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)
            .with_context(|| format!("create managed file {}", path.display()))?;
        created = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        Ok(())
    })();
    // Never unlink a path we did not create: create_new may have failed because an existing
    // file or symlink won a race after preflight.
    if result.is_err() && created {
        let _ = std::fs::remove_file(path);
    }
    result
}

fn daemon_reload_if_systemd(systemd_root: &Path) -> Result<()> {
    // Unit tests and staging pass synthetic unit roots; never reload the host's systemd for them.
    if systemd_root == Path::new(AGENT_SYSTEMD_ROOT) && Path::new("/run/systemd/system").is_dir() {
        let output = std::process::Command::new("/usr/bin/systemctl")
            .arg("daemon-reload")
            .output()
            .context("run systemctl daemon-reload after installing Hub credential drop-in")?;
        ensure!(
            output.status.success(),
            "systemd daemon-reload failed while installing Hub credential drop-in"
        );
    }
    Ok(())
}

struct HubProvision<'a> {
    instance: &'a str,
    ca_dir: &'a Path,
    hub_root: &'a Path,
    fleet_config_root: &'a Path,
    systemd_root: &'a Path,
    hub_name: &'a str,
    dial_san: &'a str,
    listen: &'a str,
}

/// Generate the Hub key in the root helper, issue its server-only certificate, and store the
/// key in a root-only per-instance directory for systemd LoadCredential delivery. Neither the
/// request nor the result contains private-key bytes.
fn provision_hub(request: HubProvision<'_>) -> Result<Value> {
    let HubProvision {
        instance,
        ca_dir,
        hub_root,
        fleet_config_root,
        systemd_root,
        hub_name,
        dial_san,
        listen,
    } = request;
    ensure!(crate::fleet_ca::valid_node_id(hub_name), "invalid hub name");
    crate::pair_ca::validate_san(dial_san)?;
    let listen_addr = listen
        .parse::<std::net::SocketAddr>()
        .context("Fleet Hub listen must be an ip:port socket address")?;
    ensure!(
        listen_addr.port() != 0,
        "Fleet Hub listen port must not be zero"
    );
    ensure_real_directory(ca_dir, false, 0o711)?;
    for name in ["ca.key", "ca.crt", "ca.crl", "openssl.cnf"] {
        let path = ca_dir.join(name);
        let meta = std::fs::symlink_metadata(&path)
            .with_context(|| format!("Fleet CA is missing {name}"))?;
        ensure!(
            meta.file_type().is_file(),
            "Fleet CA {name} is not a regular file"
        );
        let expected_mode = match name {
            "ca.key" | "openssl.cnf" => 0o600,
            "ca.crt" | "ca.crl" => 0o644,
            _ => unreachable!(),
        };
        ensure!(
            std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777 == expected_mode,
            "Fleet CA {name} must be mode {expected_mode:04o}"
        );
        if unsafe { libc::geteuid() } == 0 {
            ensure!(
                std::os::unix::fs::MetadataExt::uid(&meta) == 0,
                "Fleet CA {name} must be root-owned"
            );
        }
    }

    // Preflight all shared directories and destinations before creating an instance identity.
    // 0711 permits traversal to the public certificate but denies directory listing. The key
    // itself remains 0600 root:root and is readable by the Agent only via systemd credentials.
    ensure_real_directory(hub_root, true, 0o711)?;
    ensure_real_directory(fleet_config_root, true, 0o755)?;
    ensure_real_directory(systemd_root, true, 0o755)?;
    let dropin_dir = systemd_root.join(format!("baft-tunnel-agent@{instance}.service.d"));
    ensure_real_directory(&dropin_dir, true, 0o755)?;
    let config_path = fleet_config_root.join(format!("{instance}.json"));
    let dropin_path = dropin_dir.join("fleet-hub.conf");
    for path in [&config_path, &dropin_path] {
        match std::fs::symlink_metadata(path) {
            Ok(_) => anyhow::bail!(
                "Hub configuration already exists; refusing to replace {}",
                path.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("inspect {}", path.display())),
        }
    }
    let instance_dir = hub_root.join(instance);
    match std::fs::symlink_metadata(&instance_dir) {
        Ok(_) => anyhow::bail!("Hub identity directory already exists; refusing reuse"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("inspect Hub identity directory"),
    }
    let key_path = instance_dir.join("hub-key");
    let cert_path = instance_dir.join("hub.crt");

    let key = Zeroizing::new(crate::pair_ca::run(
        &[
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
        ],
        None,
    )?);
    let subject = format!("/CN={hub_name}");
    let csr = crate::pair_ca::run(
        &["req", "-new", "-key", "/dev/stdin", "-subj", &subject],
        Some(key.as_slice()),
    )
    .context("create Hub CSR from the locally generated key")?;
    let csr = String::from_utf8(csr).context("Hub CSR is not UTF-8")?;
    let cert = crate::fleet_ca::sign_hub(ca_dir, hub_name, dial_san, &csr)?;
    let fleet_config = json!({
        "hub": {
            "listen": listen_addr.to_string(),
            "ca": ca_dir.join("ca.crt"),
            "crl": ca_dir.join("ca.crl"),
            "cert": cert_path,
            "key_credential": HUB_CREDENTIAL_ID
        }
    });
    let config_bytes = serde_json::to_vec_pretty(&fleet_config)?;
    let dropin = format!(
        "# Managed by BAFT-TUNNEL fleet helper; do not edit.\n[Service]\nLoadCredential={HUB_CREDENTIAL_ID}:/etc/baft-tunnel/fleet-hub/%i/hub-key\n"
    );

    std::fs::create_dir(&instance_dir).context("create per-instance Hub identity directory")?;
    if let Err(error) =
        std::fs::set_permissions(&instance_dir, std::fs::Permissions::from_mode(0o711))
    {
        let _ = std::fs::remove_dir(&instance_dir);
        return Err(error).context("secure per-instance Hub identity directory");
    }

    // create_new prevents replacement races. Each later failure removes earlier files so no
    // partially configured Hub is left behind. A crash still leaves a root-only incomplete
    // identity directory, which the next attempt refuses to reuse.
    if let Err(error) = write_managed_file(&key_path, key.as_slice(), 0o600) {
        let _ = std::fs::remove_dir(&instance_dir);
        return Err(error);
    }
    if let Err(error) = write_managed_file(&cert_path, cert.as_bytes(), 0o644) {
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_dir(&instance_dir);
        return Err(error).context("publish Hub certificate");
    }
    if let Err(error) = write_managed_file(&config_path, &config_bytes, 0o644) {
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_dir(&instance_dir);
        return Err(error).context("write Fleet Hub configuration");
    }
    if let Err(error) = write_managed_file(&dropin_path, dropin.as_bytes(), 0o644) {
        let _ = std::fs::remove_file(&config_path);
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_dir(&instance_dir);
        return Err(error).context("write per-instance Agent systemd credential drop-in");
    }
    if let Err(error) = daemon_reload_if_systemd(systemd_root) {
        let _ = std::fs::remove_file(&dropin_path);
        let _ = std::fs::remove_file(&config_path);
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_dir(&instance_dir);
        return Err(error);
    }
    Ok(json!({
        "state": "done",
        "certificate": cert,
        "certificate_path": cert_path,
        "fleet_config_path": config_path,
        "credential_id": HUB_CREDENTIAL_ID,
        "restart_required": true
    }))
}

fn restart_agent_unit(instance: &str, systemd_root: &Path) -> Result<()> {
    ensure!(
        systemd_root == Path::new(AGENT_SYSTEMD_ROOT) && Path::new("/run/systemd/system").is_dir(),
        "systemd is not available for this Agent unit root"
    );
    let unit = format!("baft-tunnel-agent@{instance}.service");
    let output = std::process::Command::new("/usr/bin/systemctl")
        .args(["--no-block", "restart", &unit])
        .output()
        .context("queue restart of the instance Agent")?;
    ensure!(
        output.status.success(),
        "systemd refused to queue the instance Agent restart"
    );
    Ok(())
}

/// One helper run: consume the request, sign/provision, and publish the result.
pub fn run(name: &str, control_dir: &Path, ca_dir: &Path) -> Result<Value> {
    run_with_paths(
        name,
        control_dir,
        ca_dir,
        Path::new(HUB_ROOT),
        Path::new(FLEET_CONFIG_ROOT),
        Path::new(AGENT_SYSTEMD_ROOT),
    )
}

fn run_with_hub_root(
    name: &str,
    control_dir: &Path,
    ca_dir: &Path,
    hub_root: &Path,
) -> Result<Value> {
    let parent = hub_root.parent().context("test Hub root needs a parent")?;
    run_with_paths(
        name,
        control_dir,
        ca_dir,
        hub_root,
        &parent.join("fleet-config"),
        &parent.join("systemd"),
    )
}

fn run_with_paths(
    name: &str,
    control_dir: &Path,
    ca_dir: &Path,
    hub_root: &Path,
    fleet_config_root: &Path,
    systemd_root: &Path,
) -> Result<Value> {
    ensure!(crate::apply::valid_instance(name), "invalid instance name");
    let req_path = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let bytes = read_bounded(&req_path);
    let _ = std::fs::remove_file(&req_path);
    let mut restart_agent = false;
    let outcome = bytes
        .and_then(|b| Ok(serde_json::from_slice::<Request>(&b)?))
        .and_then(|request| match request {
            Request::SignNode { node_id, csr } => {
                ensure!(
                    ca_dir.join("ca.crt").is_file(),
                    "no Fleet CA is initialized on this host"
                );
                let cert = crate::fleet_ca::sign(ca_dir, &node_id, &csr)?;
                Ok(json!({"state": "done", "certificate": cert}))
            }
            Request::ProvisionHub {
                hub_name,
                dial_san,
                listen,
            } => provision_hub(HubProvision {
                instance: name,
                ca_dir,
                hub_root,
                fleet_config_root,
                systemd_root,
                hub_name: &hub_name,
                dial_san: &dial_san,
                listen: &listen,
            }),
            Request::RestartAgent => {
                let config_path = fleet_config_root.join(format!("{name}.json"));
                let config: Value = serde_json::from_slice(
                    &std::fs::read(&config_path).context("Fleet Hub config is not installed")?,
                )?;
                ensure!(
                    config["hub"]["key_credential"] == HUB_CREDENTIAL_ID,
                    "refusing Agent restart without the managed Hub credential config"
                );
                restart_agent = true;
                Ok(json!({"state": "done", "restart_queued": true}))
            }
        });
    let result = match outcome {
        Ok(value) => value,
        // The reason never contains a CSR or any key: errors come from fixed checks.
        Err(e) => json!({"state": "failed", "reason": format!("{e:#}")}),
    };
    if restart_agent {
        if result["state"] == "done" {
            // The API route does not wait for a result: systemd may terminate its Agent while
            // the browser is reconnecting. The helper itself is a separate root service.
            restart_agent_unit(name, systemd_root)?;
        }
        return Ok(result);
    }
    write_result(control_dir, name, &result)?;
    Ok(result)
}

/// Publish a complete request atomically. The path unit watches the final `.fleet` name, so it
/// must never observe a partially written JSON document. A hard link provides create-if-absent
/// semantics on the same control filesystem without replacing a pending request.
fn publish_request(control_dir: &Path, instance: &str, body: &[u8]) -> Result<(), String> {
    let request_path = control_dir.join(format!("{instance}.{REQUEST_SUFFIX}"));
    let temporary_path = control_dir.join(format!(
        "{instance}.{REQUEST_SUFFIX}.tmp-{:016x}",
        rand::random::<u64>()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o660)
            .open(&temporary_path)
            .map_err(|e| format!("cannot create a private Fleet request file: {e}"))?;
        file.write_all(body)
            .map_err(|e| format!("cannot write Fleet request: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("cannot persist Fleet request: {e}"))?;
        drop(file);
        std::fs::hard_link(&temporary_path, &request_path).map_err(|e| {
            format!("a Fleet request is already pending or cannot be published: {e}")
        })?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temporary_path);
    result
}

fn ensure_no_pending_result(control_dir: &Path, instance: &str) -> Result<(), String> {
    let result_path = control_dir.join(format!("{instance}.{RESULT_SUFFIX}"));
    match std::fs::symlink_metadata(&result_path) {
        Ok(_) => Err("a previous Fleet helper result is still pending".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect pending Fleet result: {error}")),
    }
}

/// Agent side: queue a restart after the operator explicitly accepts Hub activation.
/// The caller returns immediately; the separate root helper restarts only this Agent instance.
pub fn queue_agent_restart(control_dir: &Path, instance: &str) -> Result<(), String> {
    if !crate::apply::valid_instance(instance) {
        return Err("invalid instance name".into());
    }
    if !control_dir.is_dir() {
        return Err("the root Fleet helper is not installed".into());
    }
    ensure_no_pending_result(control_dir, instance)?;
    publish_request(control_dir, instance, br#"{"op":"restart_agent"}"#)
}

/// Agent side: ask the root helper to create a Hub identity. The key is generated and retained
/// by root; this response contains only public certificate metadata.
pub fn request_hub_identity(
    control_dir: &Path,
    instance: &str,
    hub_name: &str,
    dial_san: &str,
    listen: &str,
    wait: Duration,
) -> Result<Value, String> {
    if !crate::apply::valid_instance(instance)
        || !crate::fleet_ca::valid_node_id(hub_name)
        || crate::pair_ca::validate_san(dial_san).is_err()
        || listen
            .parse::<std::net::SocketAddr>()
            .map(|addr| addr.port() == 0)
            .unwrap_or(true)
    {
        return Err("invalid instance, hub name or dial SAN".into());
    }
    if !control_dir.is_dir() {
        return Err("the root Fleet helper is not installed".into());
    }
    let result_path = control_dir.join(format!("{instance}.{RESULT_SUFFIX}"));
    ensure_no_pending_result(control_dir, instance)?;
    let request = json!({
        "op": "provision_hub",
        "hub_name": hub_name,
        "dial_san": dial_san,
        "listen": listen
    })
    .to_string();
    publish_request(control_dir, instance, request.as_bytes())?;
    let started = Instant::now();
    while started.elapsed() < wait {
        if let Ok(bytes) = read_bounded(&result_path) {
            let _ = std::fs::remove_file(&result_path);
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| "unreadable result")?;
            if value["state"] == "done" && value["certificate"].is_string() {
                return Ok(value);
            }
            return Err(value["reason"]
                .as_str()
                .unwrap_or("Hub provisioning failed")
                .to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("the root Fleet helper did not answer in time".into())
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
    ensure_no_pending_result(control_dir, name)?;
    let request = json!({"op": "sign_node", "node_id": node_id, "csr": csr}).to_string();
    publish_request(control_dir, name, request.as_bytes())?;
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
    fn request_publication_is_atomic_and_never_replaces_pending_request() {
        let dir = tmp("request-atomic");
        queue_agent_restart(&dir, "edge").unwrap();
        assert_eq!(
            std::fs::read(dir.join("edge.fleet")).unwrap(),
            br#"{"op":"restart_agent"}"#
        );
        let before = std::fs::read(dir.join("edge.fleet")).unwrap();
        assert!(publish_request(&dir, "edge", br#"{"op":"sign_node"}"#).is_err());
        assert_eq!(std::fs::read(dir.join("edge.fleet")).unwrap(), before);
        assert!(std::fs::read_dir(&dir).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn root_helper_provisions_hub_key_and_server_only_certificate() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tmp("hub");
        let ca = dir.join("ca");
        crate::fleet_ca::create(&ca, "Test Fleet CA").unwrap();
        let control = dir.join("control");
        std::fs::create_dir(&control).unwrap();
        let hub_root = dir.join("fleet-hub");
        std::fs::write(
            control.join("edge.fleet"),
            serde_json::json!({
                "op": "provision_hub",
                "hub_name": "hub-1",
                "dial_san": "IP:203.0.113.7",
                "listen": "0.0.0.0:18443"
            })
            .to_string(),
        )
        .unwrap();

        let result = run_with_hub_root("edge", &control, &ca, &hub_root).unwrap();
        assert_eq!(result["state"], "done", "Hub provisioning failed: {result}");
        assert_eq!(result["credential_id"], HUB_CREDENTIAL_ID);
        let certificate = result["certificate"].as_str().unwrap();
        assert!(certificate.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(!result.to_string().contains("PRIVATE KEY"));

        let instance_dir = hub_root.join("edge");
        let key_path = instance_dir.join("hub-key");
        let cert_path = instance_dir.join("hub.crt");
        assert_eq!(
            std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&cert_path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(
            std::fs::metadata(&instance_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o711
        );
        assert_eq!(std::fs::read_to_string(&cert_path).unwrap(), certificate);
        let config_path = dir.join("fleet-config/edge.json");
        let config: Value = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        assert_eq!(config["hub"]["listen"], "0.0.0.0:18443");
        assert_eq!(config["hub"]["key_credential"], HUB_CREDENTIAL_ID);
        assert_eq!(config["hub"]["cert"], cert_path.to_string_lossy().as_ref());
        assert!(config["hub"]["key"].is_null());
        let dropin = std::fs::read_to_string(
            dir.join("systemd/baft-tunnel-agent@edge.service.d/fleet-hub.conf"),
        )
        .unwrap();
        assert!(
            dropin.contains("LoadCredential=fleet-hub-key:/etc/baft-tunnel/fleet-hub/%i/hub-key")
        );
        assert!(
            !dropin.contains("ExecStart="),
            "Hub drop-in must preserve the managed Agent ExecStart"
        );

        let key_public = crate::pair_ca::run(
            &["pkey", "-pubout"],
            Some(&std::fs::read(&key_path).unwrap()),
        )
        .unwrap();
        let cert_public = crate::pair_ca::run(
            &[
                "x509",
                "-pubkey",
                "-noout",
                "-in",
                &cert_path.to_string_lossy(),
            ],
            None,
        )
        .unwrap();
        assert_eq!(
            key_public, cert_public,
            "certificate must match root-held key"
        );

        let details = crate::pair_ca::run(
            &[
                "x509",
                "-noout",
                "-text",
                "-in",
                &cert_path.to_string_lossy(),
            ],
            None,
        )
        .unwrap();
        let details = String::from_utf8(details).unwrap();
        assert!(details.contains("TLS Web Server Authentication"));
        assert!(!details.contains("TLS Web Client Authentication"));
        assert!(details.contains("baft-fleet-hub://hub-1.fleet.baft.invalid"));
        assert!(details.contains("203.0.113.7"));

        let result_file = std::fs::read_to_string(control.join("edge.fleet-result")).unwrap();
        assert!(!result_file.contains("PRIVATE KEY"));
        assert!(!result_file.contains("BEGIN EC PRIVATE KEY"));
        assert!(result_file.contains("BEGIN CERTIFICATE"));

        // A second request cannot silently rotate or overwrite the provisioned identity.
        let original_key = std::fs::read(&key_path).unwrap();
        std::fs::write(
            control.join("edge.fleet"),
            serde_json::json!({
                "op": "provision_hub",
                "hub_name": "hub-1",
                "dial_san": "IP:203.0.113.7",
                "listen": "0.0.0.0:18443"
            })
            .to_string(),
        )
        .unwrap();
        let second = run_with_hub_root("edge", &control, &ca, &hub_root).unwrap();
        assert_eq!(second["state"], "failed");
        assert_eq!(std::fs::read(&key_path).unwrap(), original_key);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hub_keys_and_credentials_are_isolated_per_instance() {
        let dir = tmp("hub-instance-isolation");
        let ca = dir.join("ca");
        crate::fleet_ca::create(&ca, "Test Fleet CA").unwrap();
        let control = dir.join("control");
        std::fs::create_dir(&control).unwrap();
        let hub_root = dir.join("fleet-hub");

        for instance in ["edge-a", "edge-b"] {
            std::fs::write(
                control.join(format!("{instance}.fleet")),
                serde_json::json!({
                    "op": "provision_hub",
                    "hub_name": format!("hub-{instance}"),
                    "dial_san": "DNS:hub.example.test",
                    "listen": "0.0.0.0:18443"
                })
                .to_string(),
            )
            .unwrap();
            let result = run_with_hub_root(instance, &control, &ca, &hub_root).unwrap();
            assert_eq!(result["state"], "done", "{instance}: {result}");
            let _ = std::fs::remove_file(control.join(format!("{instance}.fleet-result")));
        }

        let key_a = std::fs::read(hub_root.join("edge-a/hub-key")).unwrap();
        let key_b = std::fs::read(hub_root.join("edge-b/hub-key")).unwrap();
        assert_ne!(
            key_a, key_b,
            "different Agent instances must never share Hub keys"
        );
        for instance in ["edge-a", "edge-b"] {
            let config = dir.join(format!("fleet-config/{instance}.json"));
            let value: Value = serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
            assert_eq!(value["hub"]["key_credential"], HUB_CREDENTIAL_ID);
            let dropin = dir.join(format!(
                "systemd/baft-tunnel-agent@{instance}.service.d/fleet-hub.conf"
            ));
            let text = std::fs::read_to_string(dropin).unwrap();
            assert!(
                text.contains("LoadCredential=fleet-hub-key:/etc/baft-tunnel/fleet-hub/%i/hub-key")
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hub_provision_derives_join_host_from_dial_san() {
        let dir = tmp("hub-join-derived");
        let ca = dir.join("ca");
        crate::fleet_ca::create(&ca, "Test Fleet CA").unwrap();
        let control = dir.join("control");
        std::fs::create_dir(&control).unwrap();
        let hub_root = dir.join("fleet-hub");
        std::fs::write(
            control.join("edge.fleet"),
            serde_json::json!({
                "op": "provision_hub", "hub_name": "hub-1",
                "dial_san": "DNS:hub.example.test", "listen": "0.0.0.0:18443",
                "join_listen": "0.0.0.0:18444"
            })
            .to_string(),
        )
        .unwrap();
        let result = run_with_hub_root("edge", &control, &ca, &hub_root).unwrap();
        assert_eq!(result["state"], "done", "{result}");
        let config: Value =
            serde_json::from_slice(&std::fs::read(dir.join("fleet-config/edge.json")).unwrap())
                .unwrap();
        assert_eq!(config["hub"]["join"]["listen"], "0.0.0.0:18444");
        assert_eq!(config["hub"]["join"]["advertise_host"], "hub.example.test");
        assert_eq!(config["hub"]["join"]["advertise_port"], 18444);
        assert_eq!(config["hub"]["join"]["hub_id"], "hub-1");
        assert_eq!(config["hub"]["join"]["hub_name"], "hub.example.test");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hub_provision_rejects_invalid_join_bind() {
        let dir = tmp("hub-join-invalid");
        let ca = dir.join("ca");
        crate::fleet_ca::create(&ca, "Test Fleet CA").unwrap();
        let control = dir.join("control");
        std::fs::create_dir(&control).unwrap();
        let hub_root = dir.join("fleet-hub");
        std::fs::write(
            control.join("edge.fleet"),
            serde_json::json!({
                "op": "provision_hub", "hub_name": "hub-1",
                "dial_san": "DNS:hub.example.test", "listen": "0.0.0.0:18443",
                "join_listen": "0.0.0.0:0"
            })
            .to_string(),
        )
        .unwrap();
        let result = run_with_hub_root("edge", &control, &ca, &hub_root).unwrap();
        assert_eq!(result["state"], "failed");
        assert!(
            result["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("Join")
        );
        assert!(
            !hub_root.exists(),
            "invalid Join bind must not create Hub identity material"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hub_provisioning_without_ca_creates_no_key_or_certificate() {
        let dir = tmp("hub-no-ca");
        let control = dir.join("control");
        std::fs::create_dir(&control).unwrap();
        let hub_root = dir.join("fleet-hub");
        std::fs::write(
            control.join("edge.fleet"),
            br#"{"op":"provision_hub","hub_name":"hub-1","dial_san":"DNS:hub.example.test","listen":"0.0.0.0:18443"}"#,
        )
        .unwrap();
        let result =
            run_with_hub_root("edge", &control, &dir.join("missing-ca"), &hub_root).unwrap();
        assert_eq!(result["state"], "failed");
        assert!(!hub_root.exists());
        assert!(!result.to_string().contains("PRIVATE KEY"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_hub_identity_request_round_trips_through_the_root_helper() {
        let dir = tmp("hub-roundtrip");
        let ca = dir.join("ca");
        crate::fleet_ca::create(&ca, "Test Fleet CA").unwrap();
        let control = dir.join("control");
        std::fs::create_dir(&control).unwrap();
        let hub_root = dir.join("fleet-hub");

        let control_for_agent = control.clone();
        let agent = std::thread::spawn(move || {
            request_hub_identity(
                &control_for_agent,
                "edge",
                "hub-1",
                "DNS:hub.example.test",
                "0.0.0.0:18443",
                Duration::from_secs(10),
            )
        });
        let request_path = control.join("edge.fleet");
        let started = Instant::now();
        while !request_path.exists() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(request_path.exists(), "agent did not publish its request");
        let helper_result = run_with_hub_root("edge", &control, &ca, &hub_root).unwrap();
        assert_eq!(
            helper_result["state"], "done",
            "root helper failed: {helper_result}"
        );
        let agent_result = agent.join().unwrap().unwrap();
        assert_eq!(agent_result["credential_id"], HUB_CREDENTIAL_ID);
        assert_eq!(agent_result["certificate"], helper_result["certificate"]);
        assert!(!agent_result.to_string().contains("PRIVATE KEY"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restart_helper_refuses_synthetic_systemd_roots() {
        let d = tmp("restart-systemd-root");
        assert!(restart_agent_unit("edge", &d).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn hub_provisioning_refuses_invalid_identity_and_symlinked_directories() {
        let dir = tmp("hub-refuse");
        let ca = dir.join("ca");
        crate::fleet_ca::create(&ca, "Test Fleet CA").unwrap();
        let control = dir.join("control");
        std::fs::create_dir(&control).unwrap();
        let hub_root = dir.join("fleet-hub");
        std::fs::write(
            control.join("edge.fleet"),
            br#"{"op":"provision_hub","hub_name":"Hub_BAD","dial_san":"DNS:hub.example","listen":"0.0.0.0:18443"}"#,
        )
        .unwrap();
        let invalid = run_with_hub_root("edge", &control, &ca, &hub_root).unwrap();
        assert_eq!(invalid["state"], "failed");
        assert!(!hub_root.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = dir.join("outside");
            std::fs::create_dir(&outside).unwrap();
            symlink(&outside, &hub_root).unwrap();
            std::fs::write(
                control.join("edge.fleet"),
                br#"{"op":"provision_hub","hub_name":"hub-1","dial_san":"DNS:hub.example","listen":"0.0.0.0:18443"}"#,
            )
            .unwrap();
            let refused = run_with_hub_root("edge", &control, &ca, &hub_root).unwrap();
            assert_eq!(refused["state"], "failed");
            assert!(std::fs::read_dir(outside).unwrap().next().is_none());
        }
        let _ = std::fs::remove_dir_all(dir);
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
