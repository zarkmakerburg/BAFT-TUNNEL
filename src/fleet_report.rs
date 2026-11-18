//! Fleet member reporter (ADR-0045 F1, read-only): an outbound-only mTLS client that posts one
//! `node.status.v1` report per interval to the hub. The member opens no new listening port. The
//! hub is verified with the pinned CA and a mandatory CRL (`tls::client_config`); the report is
//! built by `fleet::build_report`, which keeps Unknown as unknown and copies no secret, path or
//! address. There is no way for the hub to send a command through this channel: the response is
//! read only for its status line and then discarded.
#![allow(dead_code)]

use crate::fleet;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 4096;

#[derive(Debug, Clone)]
pub struct ReporterConfig {
    /// `host:port` the member dials.
    pub hub_addr: String,
    /// TLS server name checked against the hub certificate.
    pub hub_name: String,
    /// Hub identity from the pairing record: the certificate must carry exactly the hub EKU and
    /// the URI `baft-fleet-hub://<hub_id>.fleet.baft.invalid` (F-2). A CA pin alone is not enough.
    pub hub_id: String,
    pub ca: PathBuf,
    pub crl: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub instance: String,
    pub version: String,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `seq` must only grow under one certificate (F-6) and a restart must not reset it, so it is the
/// unix time in seconds, bumped past the previous value when two reports share a second.
pub fn next_seq(last: u64, now: u64) -> u64 {
    now.max(last.saturating_add(1))
}

/// Status line code of an HTTP/1.1 response, or an error if the answer is not one.
fn parse_status(raw: &[u8]) -> Result<u16> {
    let line = std::str::from_utf8(raw)
        .ok()
        .and_then(|s| s.lines().next())
        .context("hub response is not text")?;
    let mut parts = line.split(' ');
    if parts.next() != Some("HTTP/1.1") {
        bail!("hub response is not HTTP/1.1");
    }
    parts
        .next()
        .and_then(|c| c.parse::<u16>().ok())
        .context("hub response has no status code")
}

/// F-2: the peer must be the paired hub, not just any certificate from the same CA. The leaf
/// carries exactly the `fleet-hub` BAFT EKU (no tunnel or node EKU) and the expected hub URI.
pub fn verify_hub_identity(leaf_der: &[u8], hub_id: &str) -> Result<()> {
    use x509_parser::extensions::GeneralName;
    let (rest, cert) = x509_parser::parse_x509_certificate(leaf_der)
        .map_err(|_| anyhow::anyhow!("hub certificate is not valid X.509 DER"))?;
    if !rest.is_empty() {
        bail!("hub certificate has trailing data");
    }
    let eku = cert
        .extended_key_usage()
        .context("parse hub certificate extended key usage")?
        .context("hub certificate has no extended key usage")?
        .value;
    if eku.any {
        bail!("anyExtendedKeyUsage is not accepted for the Fleet hub");
    }
    let ids: Vec<String> = eku.other.iter().map(|o| o.to_id_string()).collect();
    let ekus: Vec<&str> = ids.iter().map(String::as_str).collect();
    if !fleet::profile_allows(&ekus, fleet::Profile::FleetHub) {
        bail!("certificate is not a Fleet hub certificate");
    }
    let want = format!("baft-fleet-hub://{hub_id}.fleet.baft.invalid");
    let san = cert
        .subject_alternative_name()
        .context("parse hub certificate subject alternative name")?
        .context("hub certificate has no subject alternative name")?;
    let found = san
        .value
        .general_names
        .iter()
        .any(|n| matches!(n, GeneralName::URI(u) if *u == want));
    if !found {
        bail!("hub certificate does not carry the paired hub identity");
    }
    Ok(())
}

/// One report over a fresh mTLS connection. Returns the HTTP status (204 means accepted).
pub async fn post_once(cfg: &ReporterConfig, body: &[u8]) -> Result<u16> {
    if body.len() > fleet::MAX_REPORT_BYTES {
        bail!("report is larger than the protocol bound");
    }
    let tls_cfg = crate::tls::client_config(&cfg.ca, &cfg.crl, &cfg.cert, &cfg.key)?;
    let name = crate::tls::server_name(&cfg.hub_name)?;
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect(&cfg.hub_addr))
        .await
        .context("connect to the hub timed out")?
        .context("connect to the hub")?;
    let exchange = async {
        let mut tls = crate::tls::connector(tls_cfg)
            .connect(name, tcp)
            .await
            .context("TLS handshake with the hub")?;
        // Check who answered before any report byte leaves this host.
        let leaf = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|c| c.first())
            .context("the hub presented no certificate")?;
        verify_hub_identity(leaf.as_ref(), &cfg.hub_id)?;
        let head = format!(
            "POST /v1/fleet/report HTTP/1.1\r\nHost: fleet-hub\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        tls.write_all(head.as_bytes()).await?;
        tls.write_all(body).await?;
        tls.flush().await?;
        let mut raw = Vec::new();
        let _ = (&mut tls)
            .take(MAX_RESPONSE_BYTES as u64)
            .read_to_end(&mut raw)
            .await;
        parse_status(&raw)
    };
    timeout(EXCHANGE_TIMEOUT, exchange)
        .await
        .context("report exchange timed out")?
}

/// Report loop: every `fleet::REPORT_INTERVAL_S` seconds read the local status (`status` returns
/// the same JSON as the local `/v1/status`) and post it. Failures are logged without any secret
/// and retried at the next interval; nothing here ever blocks a tunnel.
pub async fn run<F>(cfg: ReporterConfig, status: F, mut shutdown: watch::Receiver<bool>)
where
    F: Fn() -> Value,
{
    let mut last_seq = 0u64;
    let mut tick = tokio::time::interval(Duration::from_secs(fleet::REPORT_INTERVAL_S));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shutdown.changed() => return,
            _ = tick.tick() => {}
        }
        last_seq = next_seq(last_seq, unix_now());
        let report = fleet::build_report(&cfg.instance, &cfg.version, &status(), last_seq);
        let Ok(body) = serde_json::to_vec(&report) else {
            continue;
        };
        match post_once(&cfg, &body).await {
            Ok(204) => {}
            Ok(code) => eprintln!("fleet report was not accepted by the hub (HTTP {code})"),
            Err(e) => eprintln!("fleet report failed: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::EKU_FLEET_NODE;
    use rcgen::{
        CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
        KeyUsagePurpose, SanType, string::Ia5String,
    };
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    #[test]
    fn seq_grows_and_survives_a_restart_without_state() {
        assert_eq!(next_seq(0, 1000), 1000);
        assert_eq!(next_seq(1000, 1000), 1001);
        assert_eq!(next_seq(1005, 1000), 1006);
        // A restarted process starts from the clock and is above anything sent earlier.
        assert!(next_seq(0, 2000) > 1999);
        assert_eq!(next_seq(u64::MAX, 5), u64::MAX);
    }

    #[test]
    fn status_line_parsing_is_strict() {
        assert_eq!(
            parse_status(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap(),
            204
        );
        assert!(parse_status(b"HTTP/1.0 204 x\r\n").is_err());
        assert!(parse_status(b"").is_err());
        assert!(parse_status(b"HTTP/1.1 abc\r\n").is_err());
        assert!(parse_status(&[0xff, 0xfe]).is_err());
    }

    fn node_cert(
        dir: &std::path::Path,
        node: &str,
        issuer: &rcgen::Issuer<'static, KeyPair>,
    ) -> (PathBuf, PathBuf) {
        let mut p = CertificateParams::new(Vec::new()).unwrap();
        p.distinguished_name = DistinguishedName::new();
        p.distinguished_name.push(DnType::CommonName, node);
        p.is_ca = IsCa::ExplicitNoCa;
        p.key_usages.push(KeyUsagePurpose::DigitalSignature);
        p.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::Other(
                EKU_FLEET_NODE
                    .split('.')
                    .map(|x| x.parse().unwrap())
                    .collect(),
            ),
        ];
        p.use_authority_key_identifier_extension = true;
        p.subject_alt_names.push(SanType::URI(
            Ia5String::try_from(format!("baft-fleet://{node}.fleet.baft.invalid")).unwrap(),
        ));
        let key = KeyPair::generate().unwrap();
        let cert = p.signed_by(&key, issuer).unwrap();
        let (c, k) = (dir.join("node.pem"), dir.join("node.key"));
        std::fs::write(&c, cert.pem()).unwrap();
        std::fs::write(&k, key.serialize_pem()).unwrap();
        (c, k)
    }

    fn hub_cert(
        dir: &std::path::Path,
        hub: &str,
        ekus: &[&str],
        issuer: &rcgen::Issuer<'static, KeyPair>,
    ) -> (PathBuf, PathBuf) {
        let mut p = CertificateParams::new(Vec::new()).unwrap();
        p.distinguished_name = DistinguishedName::new();
        p.distinguished_name.push(DnType::CommonName, hub);
        p.is_ca = IsCa::ExplicitNoCa;
        p.key_usages.push(KeyUsagePurpose::DigitalSignature);
        let mut u = vec![ExtendedKeyUsagePurpose::ServerAuth];
        u.extend(ekus.iter().map(|e| {
            ExtendedKeyUsagePurpose::Other(e.split('.').map(|x| x.parse().unwrap()).collect())
        }));
        p.extended_key_usages = u;
        p.use_authority_key_identifier_extension = true;
        p.subject_alt_names.push(SanType::DnsName(
            Ia5String::try_from("localhost".to_owned()).unwrap(),
        ));
        p.subject_alt_names.push(SanType::URI(
            Ia5String::try_from(format!("baft-fleet-hub://{hub}.fleet.baft.invalid")).unwrap(),
        ));
        let key = KeyPair::generate().unwrap();
        let cert = p.signed_by(&key, issuer).unwrap();
        let tag = format!("hub-{hub}-{}", ekus.len());
        let (c, k) = (
            dir.join(format!("{tag}.pem")),
            dir.join(format!("{tag}.key")),
        );
        std::fs::write(&c, cert.pem()).unwrap();
        std::fs::write(&k, key.serialize_pem()).unwrap();
        (c, k)
    }

    #[test]
    fn hub_identity_needs_the_hub_eku_and_the_paired_name() {
        let dir = std::env::temp_dir().join(format!("baft-fleet-hubid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ca = crate::tls::tests::make_ca(&dir, "ca");
        let der = |p: &PathBuf| crate::tls::certificates(p).unwrap()[0].as_ref().to_vec();
        let (ok, _) = hub_cert(&dir, "hub-1", &[fleet::EKU_FLEET_HUB], &ca.issuer);
        verify_hub_identity(&der(&ok), "hub-1").unwrap();
        assert!(verify_hub_identity(&der(&ok), "hub-2").is_err());
        // A node certificate, a tunnel certificate and a two-EKU certificate are never the hub.
        let (n, _) = hub_cert(&dir, "hub-1", &[EKU_FLEET_NODE], &ca.issuer);
        assert!(verify_hub_identity(&der(&n), "hub-1").is_err());
        let (t, _) = hub_cert(&dir, "hub-1", &[fleet::EKU_TUNNEL], &ca.issuer);
        assert!(verify_hub_identity(&der(&t), "hub-1").is_err());
        let (m, _) = hub_cert(
            &dir,
            "hub-1",
            &[fleet::EKU_FLEET_HUB, EKU_FLEET_NODE],
            &ca.issuer,
        );
        assert!(verify_hub_identity(&der(&m), "hub-1").is_err());
        let (leaf, _) = crate::tls::tests::leaf(&dir, "tun", "urn:baft-tunnel:node:x", &ca.issuer);
        assert!(verify_hub_identity(&der(&leaf), "hub-1").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn reporter_delivers_a_report_with_rtt_to_the_real_hub_listener() {
        let dir = std::env::temp_dir().join(format!("baft-fleet-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ca = crate::tls::tests::make_ca(&dir, "ca");
        let (hub_cert, hub_key) = hub_cert(&dir, "hub-1", &[fleet::EKU_FLEET_HUB], &ca.issuer);
        let (cert, key) = node_cert(&dir, "node-a", &ca.issuer);
        let acceptor = Arc::new(
            crate::tls::ReloadingAcceptor::new(&ca.ca_pem, &ca.crl_pem, &hub_cert, &hub_key)
                .unwrap(),
        );
        let hub = Arc::new(Mutex::new(fleet::Hub::default()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, rx) = watch::channel(false);
        let server = tokio::spawn(crate::fleet_hub::serve(listener, acceptor, hub.clone(), rx));

        let cfg = ReporterConfig {
            hub_addr: addr.to_string(),
            hub_name: "localhost".into(),
            hub_id: "hub-1".into(),
            ca: ca.ca_pem.clone(),
            crl: ca.crl_pem.clone(),
            cert,
            key,
            instance: "t1".into(),
            version: "0.3.0".into(),
        };
        let status = serde_json::json!({"tunnel": {"state": "carrying_traffic",
            "tiers": {"integrity": {"result": "pass"}},
            "health": {"state": "healthy", "rtt_ms": 12.5, "jitter_ms": 2.0}}});
        let body = serde_json::to_vec(&fleet::build_report("t1", "0.3.0", &status, 1)).unwrap();
        assert_eq!(post_once(&cfg, &body).await.unwrap(), 204);
        {
            let h = hub.lock().unwrap();
            let n = h.nodes().next().unwrap();
            assert_eq!(n.node_id, "node-a");
            assert_eq!(n.instances[0].health.as_deref(), Some("healthy"));
            assert_eq!(n.instances[0].rtt_ms, Some(12.5));
        }
        // The same seq again is a replay, reported as a status, not an error.
        assert_eq!(post_once(&cfg, &body).await.unwrap(), 409);
        // An oversized report is refused before anything is sent.
        let big = vec![b'x'; fleet::MAX_REPORT_BYTES + 1];
        assert!(post_once(&cfg, &big).await.is_err());
        // A hub whose identity differs from the paired one gets no report.
        let mut other = cfg.clone();
        other.hub_id = "hub-2".into();
        assert!(post_once(&other, &body).await.is_err());
        assert_eq!(hub.lock().unwrap().nodes().count(), 1);
        // A hub that cannot be reached is an error, not a hang.
        let mut dead = cfg.clone();
        dead.hub_addr = "127.0.0.1:1".into();
        assert!(post_once(&dead, &body).await.is_err());

        let _ = stop.send(true);
        let _ = server.await;
        let _ = std::fs::remove_dir_all(dir);
    }
}
