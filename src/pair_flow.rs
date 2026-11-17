//! Automatic pairing orchestration (ADR-0050, stage 4): `pair offer` on the listening node and
//! `pair accept CODE` on the other node. Reuses `setup::init`/`setup::join`, so the resulting
//! configuration is exactly what guided setup produces; only the way the PKI files and the pairing
//! bundles reach the nodes changes. Direct, `tcp-tls`, protocol v2 (Reverse is a later stage).

use std::{
    fs,
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use tokio::net::TcpListener;

use crate::{
    authorization::ConnectionDirection,
    config::EngineKind,
    pair_ca::{self, Member},
    pair_proto::{self, JoinResponse, PairCode},
    setup::{self, Bundle, PlanRequest, Role},
};

pub struct OfferOptions {
    pub dir: PathBuf,
    pub instance: Option<String>,
    /// Human name; becomes the tunnel id and prefix of the generated node ids.
    pub name: String,
    pub service_port: u16,
    pub tunnel_port: u16,
    /// Address the other node dials; detected when absent.
    pub host: Option<String>,
}

fn slug(name: &str) -> Result<String> {
    let s: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    ensure!(
        !s.is_empty() && s.len() <= 32,
        "name must be 1-32 letters, digits or dashes"
    );
    Ok(s)
}

fn short() -> String {
    use rand::RngCore;
    let mut b = [0u8; 3];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// The address this host uses to reach the internet (no packet is sent).
pub fn detect_host() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

fn gen_key_and_csr(dir: &Path, node_id: &str) -> Result<String> {
    let key = pair_ca::run(
        &[
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
        ],
        None,
    )?;
    let key_path = dir.join("node.key");
    pair_ca::write_private(&key_path, &key, 0o600)?;
    let csr = pair_ca::run(
        &[
            "req",
            "-new",
            "-key",
            &key_path.to_string_lossy(),
            "-subj",
            &format!("/CN={node_id}"),
        ],
        None,
    )?;
    Ok(String::from_utf8(csr)?)
}

fn install_public(dir: &Path, leaf: &str, ca: &str, crl: &str) -> Result<()> {
    for (name, body) in [("node.crt", leaf), ("ca.crt", ca), ("ca.crl", crl)] {
        pair_ca::write_private(&dir.join(name), body.as_bytes(), 0o644)?;
    }
    Ok(())
}

fn req(
    dir: &Path,
    instance: &Option<String>,
    role: Role,
    node: &str,
    peer: &str,
    code: &PairCode,
) -> PlanRequest {
    PlanRequest {
        engine: EngineKind::TcpTls,
        role,
        direction: ConnectionDirection::Direct,
        protocol_version: setup::default_protocol_for(EngineKind::TcpTls),
        node_id: node.into(),
        peer_id: peer.into(),
        tunnel_id: code.tunnel_id.clone(),
        service_port: code.service_port,
        tunnel_port: code.tunnel_port,
        peer_host: (role == Role::Forward).then(|| code.host.clone()),
        backup_peer_hosts: vec![],
        server_name: None,
        dir: dir.to_path_buf(),
        instance: instance.clone(),
    }
}

fn adopt(dir: &Path, instance: &Option<String>) -> Result<()> {
    if let Some(name) = instance {
        for line in setup::adopt_for_service(dir, &format!("{name}.json"))? {
            println!("Owner/mode set for the service: {line}");
        }
    }
    Ok(())
}

/// Listening node: prepare everything, print the code, wait for one approved join, write config.
pub async fn offer(
    opts: OfferOptions,
    mut approve: impl FnMut(&str, SocketAddr) -> bool,
    announce: impl FnOnce(&str),
) -> Result<()> {
    ensure!(
        opts.service_port >= 1024 && opts.tunnel_port >= 1024,
        "ports must be 1024-65535"
    );
    let name = slug(&opts.name)?;
    let host = match opts.host.clone().or_else(detect_host) {
        Some(h) => h,
        None => bail!("cannot detect this server's address; pass --host"),
    };
    let server_name = if host.parse::<std::net::IpAddr>().is_ok() {
        format!("IP:{host}")
    } else {
        format!("DNS:{host}")
    };
    let (listener_id, joiner_id) = (
        format!("{name}-s-{}", short()),
        format!("{name}-f-{}", short()),
    );
    let members = [
        Member {
            node_id: listener_id.clone(),
            san: server_name.clone(),
        },
        Member {
            node_id: joiner_id.clone(),
            san: format!("DNS:{joiner_id}.pair.invalid"),
        },
    ];
    let dir = &opts.dir;
    if !dir.exists() {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o750)
            .create(dir)?;
    }
    let ca_dir = dir.join("pair-ca");
    let ca_pem = pair_ca::create(&ca_dir, &format!("BAFT pair {name}"), &members)?;
    let csr = gen_key_and_csr(dir, &listener_id)?;
    let leaf = pair_ca::sign(&ca_dir, &listener_id, &csr)?;
    install_public(
        dir,
        &leaf,
        &ca_pem,
        &fs::read_to_string(ca_dir.join("ca.crl"))?,
    )?;

    let (acceptor, pin) = pair_proto::pairing_identity(&dir.join("pair-id"))?;
    let _ = fs::remove_dir_all(dir.join("pair-id"));
    let code = PairCode {
        v: 1,
        host,
        port: opts.tunnel_port,
        pin,
        token: pair_proto::new_token(),
        tunnel_id: name,
        direction: "direct".into(),
        listener_id: listener_id.clone(),
        joiner_id: joiner_id.clone(),
        service_port: opts.service_port,
        tunnel_port: opts.tunnel_port,
        server_name,
        exp: pair_proto::now_secs() + pair_proto::CODE_TTL_SECS,
    };
    setup::init(
        &req(
            dir,
            &opts.instance,
            Role::Serve,
            &listener_id,
            &joiner_id,
            &code,
        ),
        None,
    )?;
    let own_bundle = fs::read_to_string(dir.join("bundle.json"))?;
    let listener = TcpListener::bind(("0.0.0.0", opts.tunnel_port))
        .await
        .with_context(|| format!("cannot listen on port {} (is it free?)", opts.tunnel_port))?;
    announce(&code.encode()?);

    let mut joiner_bundle: Option<Bundle> = None;
    let work = dir.join("pair-join");
    pair_proto::serve(listener, acceptor, &code, |request, peer| {
        ensure!(approve(&request.node_id, peer), "not approved");
        let leaf = pair_ca::sign(&ca_dir, &request.node_id, &request.csr_pem)?;
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&work)?;
        let crt = work.join("joiner.crt");
        let _ = fs::remove_file(&crt);
        fs::write(&crt, &leaf)?;
        let (pin, _) = crate::tls::node_certificate_summary(&crt, &request.node_id)?;
        joiner_bundle = Some(Bundle {
            version: 1,
            tunnel_id: code.tunnel_id.clone(),
            engine: "tcp-tls".into(),
            direction: "direct".into(),
            protocol_version: setup::default_protocol_for(EngineKind::TcpTls),
            role: Role::Forward,
            node_id: joiner_id.clone(),
            peer_id: listener_id.clone(),
            service_port: code.service_port,
            tunnel_port: code.tunnel_port,
            public: pin,
            server_name: None,
            engine_sha256: None,
        });
        Ok(JoinResponse {
            ok: true,
            certificate_pem: Some(leaf),
            ca_pem: Some(ca_pem.clone()),
            crl_pem: Some(fs::read_to_string(ca_dir.join("ca.crl"))?),
            peer_bundle: Some(own_bundle.clone()),
            reason: None,
        })
    })
    .await?;
    let _ = fs::remove_dir_all(&work);
    let bundle = joiner_bundle.context("pairing finished without a joiner bundle")?;
    let path = dir.join("peer-bundle.json");
    fs::write(&path, serde_json::to_vec_pretty(&bundle)?)?;
    let cfg = setup::join(dir, &path, &bundle.checksum())?;
    let _ = fs::remove_file(&path);
    println!("Configuration written: {}", cfg.display());
    adopt(dir, &opts.instance)?;
    Ok(())
}

/// Other node: decode the code, enroll through the listener, write config.
pub async fn accept(code_text: &str, dir: &Path, instance: Option<String>) -> Result<()> {
    let code = PairCode::decode(code_text)?;
    if !dir.exists() {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o750)
            .create(dir)?;
    }
    let csr = gen_key_and_csr(dir, &code.joiner_id)?;
    let resp = match pair_proto::join(&code, &csr).await {
        Ok(r) => r,
        Err(e) => {
            let _ = fs::remove_file(dir.join("node.key"));
            return Err(e);
        }
    };
    let cleanup = |dir: &Path| {
        for f in [
            "node.key",
            "node.crt",
            "ca.crt",
            "ca.crl",
            "bundle.json",
            "pairing.json",
        ] {
            let _ = fs::remove_file(dir.join(f));
        }
    };
    let (Some(leaf), Some(ca), Some(crl), Some(peer)) = (
        resp.certificate_pem,
        resp.ca_pem,
        resp.crl_pem,
        resp.peer_bundle,
    ) else {
        cleanup(dir);
        bail!("the other server refused the pairing (code used, expired or not approved)");
    };
    let result = (|| -> Result<()> {
        install_public(dir, &leaf, &ca, &crl)?;
        crate::tls::verify_local_certificate(
            &dir.join("ca.crt"),
            &dir.join("ca.crl"),
            &dir.join("node.crt"),
            &format!("DNS:{}.pair.invalid", code.joiner_id),
        )?;
        setup::init(
            &req(
                dir,
                &instance,
                Role::Forward,
                &code.joiner_id,
                &code.listener_id,
                &code,
            ),
            None,
        )?;
        let bundle: Bundle = serde_json::from_str(&peer).context("peer bundle is invalid")?;
        ensure!(
            bundle.node_id == code.listener_id && bundle.tunnel_id == code.tunnel_id,
            "peer bundle does not match the code"
        );
        let path = dir.join("peer-bundle.json");
        fs::write(&path, &peer)?;
        let cfg = setup::join(dir, &path, &bundle.checksum())?;
        let _ = fs::remove_file(&path);
        println!("Configuration written: {}", cfg.display());
        adopt(dir, &instance)
    })();
    if result.is_err() {
        cleanup(dir);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pair-flow-{tag}-{:x}", rand::random::<u64>()))
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test]
    async fn offer_and_accept_produce_matching_working_configs() {
        let (da, db) = (tmp("a"), tmp("b"));
        let port = free_port();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let offer_task = tokio::spawn({
            let da = da.clone();
            async move {
                offer(
                    OfferOptions {
                        dir: da,
                        instance: None,
                        name: "Demo Tunnel".into(),
                        service_port: 9000,
                        tunnel_port: port,
                        host: Some("127.0.0.1".into()),
                    },
                    |_, _| true,
                    |code| {
                        let _ = tx.send(code.to_string());
                    },
                )
                .await
            }
        });
        let code = rx.await.unwrap();
        assert!(code.starts_with("BAFT1-"));
        accept(&code, &db, None).await.unwrap();
        offer_task.await.unwrap().unwrap();
        let ca: serde_json::Value =
            serde_json::from_slice(&fs::read(da.join("config.json")).unwrap()).unwrap();
        let cb: serde_json::Value =
            serde_json::from_slice(&fs::read(db.join("config.json")).unwrap()).unwrap();
        assert_eq!(ca["mode"], "serve");
        assert_eq!(cb["mode"], "forward");
        assert_eq!(cb["remote"], format!("127.0.0.1:{port}"));
        // each side pins the other's real leaf and its own CA never left its host
        assert!(ca["peer_fingerprint"].as_str().unwrap().len() == 64);
        assert!(cb["peer_fingerprint"].as_str().unwrap().len() == 64);
        assert!(da.join("pair-ca/ca.key").exists());
        assert!(!db.join("pair-ca").exists() && !db.join("ca.key").exists());
        let _ = fs::remove_dir_all(&da);
        let _ = fs::remove_dir_all(&db);
    }

    #[tokio::test]
    async fn unapproved_join_leaves_no_files_on_the_joiner() {
        let (da, db) = (tmp("na"), tmp("nb"));
        let port = free_port();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let t = tokio::spawn({
            let da = da.clone();
            async move {
                let _ = offer(
                    OfferOptions {
                        dir: da,
                        instance: None,
                        name: "x".into(),
                        service_port: 9000,
                        tunnel_port: port,
                        host: Some("127.0.0.1".into()),
                    },
                    |_, _| false,
                    |code| {
                        let _ = tx.send(code.to_string());
                    },
                )
                .await;
            }
        });
        let code = rx.await.unwrap();
        assert!(accept(&code, &db, None).await.is_err());
        for f in ["node.key", "node.crt", "config.json"] {
            assert!(!db.join(f).exists(), "{f}");
        }
        t.abort();
        let _ = fs::remove_dir_all(&da);
        let _ = fs::remove_dir_all(&db);
    }
}
