//! Member-side Fleet Join activation: root-owned credential storage, durable bundle publication,
//! pinned TLS enrollment orchestration and crash-safe retry/resume.
//!
//! Layout per instance (all root-managed, Mode 0700 until publication):
//!
//! ```text
//! /etc/baft-tunnel/fleet-member/NAME/
//!   pending/offer.json     one-time offer, Mode 0600
//!   pending/request.csr    exact CSR bound to the offer nonce, Mode 0600
//!   pending/member.key     locally generated EC P-256 key, Mode 0600
//!   generations/<sha256>/  immutable published bundle, Mode 0750
//!     member.crt  member.key  ca.crt  ca.crl  hub.crt
//! ```
//!
//! Invariants this module owns:
//! - A pending transaction is committed by `offer.json` only. Files without it are not a
//!   transaction and are cleared before a new attempt, so a crash before the marker cannot pin
//!   stale state.
//! - A different offer never overwrites a committed pending transaction.
//! - Nothing is published until every PEM, the key/certificate match, the chain to the Fleet CA
//!   and the leaf profile have been validated inside a private staging directory.
//! - Publication is a same-filesystem rename of a fully synced staging directory, so a failure
//!   can only leave an unlisted `.staging-*` directory, never a partial generation.
//! - The Member config update preserves an existing Hub section and rejects unknown fields.
//!
//! Secrets (offer token, nonce, private key) are never formatted, logged or returned by any
//! function here; [`PendingJoin`] keeps a redacting `Debug`.

#![allow(dead_code)] // CLI wiring follows in Task 4

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::fleet::EKU_FLEET_NODE;
use crate::fleet_ca::{self, node_uri, valid_node_id};
use crate::fleet_join::{JoinOffer, JoinResponse};
use crate::pair_ca;

/// Production root of the Member credential store. Tests pass their own base.
pub(crate) const MEMBER_ROOT: &str = "/etc/baft-tunnel/fleet-member";

const MAX_OFFER_BYTES: u64 = 8 * 1024;
const MAX_CSR_BYTES: u64 = 16 * 1024;
const MAX_KEY_BYTES: u64 = 16 * 1024;
const MAX_PEM_BYTES: u64 = 64 * 1024;
const MAX_CONFIG_BYTES: u64 = 16 * 1024;

const PENDING_DIR_MODE: u32 = 0o700;
const PUBLISHED_DIR_MODE: u32 = 0o750;
const SECRET_MODE: u32 = 0o600;
const KEY_MODE: u32 = 0o640;
const PUBLIC_MODE: u32 = 0o644;
const CONFIG_MODE: u32 = 0o640;

/// Immutable per-instance paths. Immutable here means every field is derived once from the
/// instance name; the generation directory is chosen only after the leaf is known. Cloning is
/// cheap and carries no secret, so callers may keep their own copy.
#[derive(Clone)]
pub(crate) struct MemberPaths {
    pub(crate) instance: String,
    pub(crate) root: PathBuf,
    pub(crate) pending: PathBuf,
    pub(crate) generations: PathBuf,
    pub(crate) offer: PathBuf,
    pub(crate) csr: PathBuf,
    pub(crate) key: PathBuf,
    pub(crate) request_config: PathBuf,
    pub(crate) config: PathBuf,
}

impl MemberPaths {
    pub(crate) fn new(base: &Path, config_root: &Path, instance: &str) -> Result<Self> {
        ensure!(
            crate::apply::valid_instance(instance),
            "invalid Fleet member instance name"
        );
        let root = base.join(instance);
        let pending = root.join("pending");
        Ok(Self {
            instance: instance.to_owned(),
            offer: pending.join("offer.json"),
            csr: pending.join("request.csr"),
            key: pending.join("member.key"),
            request_config: pending.join("request.cnf"),
            generations: root.join("generations"),
            config: config_root.join(format!("{instance}.json")),
            pending,
            root,
        })
    }

    /// Production paths for an installed instance.
    pub(crate) fn installed(instance: &str) -> Result<Self> {
        Self::new(
            Path::new(MEMBER_ROOT),
            Path::new(crate::fleet_helper::FLEET_CONFIG_ROOT),
            instance,
        )
    }
}

/// One exact pending Join transaction: the offer, the CSR that must be re-sent byte for byte,
/// and the local private key path. Never prints the token, nonce or key.
pub(crate) struct PendingJoin {
    pub(crate) paths: MemberPaths,
    pub(crate) offer: JoinOffer,
    pub(crate) csr_pem: String,
    pub(crate) key_path: PathBuf,
    /// True when an identical committed transaction was found and reused instead of regenerated.
    pub(crate) reused: bool,
}

impl fmt::Debug for PendingJoin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingJoin")
            .field("instance", &self.paths.instance)
            .field("offer_id_known", &!self.offer.offer_id.is_empty())
            .field("node_id", &self.offer.node_id)
            .field("reused", &self.reused)
            .finish_non_exhaustive()
    }
}

/// Public paths of one published generation.
#[derive(Debug, Clone)]
pub(crate) struct InstalledBundle {
    pub(crate) dir: PathBuf,
    pub(crate) cert: PathBuf,
    pub(crate) key: PathBuf,
    pub(crate) ca: PathBuf,
    pub(crate) crl: PathBuf,
    pub(crate) hub_cert: PathBuf,
    pub(crate) node_id: String,
    /// SHA-256 of the DER leaf; also the immutable generation name.
    pub(crate) leaf_sha256: String,
}

/// Create or load the single pending transaction for `instance`.
///
/// A committed `offer.json` is reused only when it is the same offer; the stored CSR and key are
/// then read back so a retry re-sends the exact same bytes. A different offer, a symlink or an
/// incomplete committed transaction fails closed.
pub(crate) fn prepare_pending(
    base: &Path,
    config_root: &Path,
    instance: &str,
    offer: &JoinOffer,
) -> Result<PendingJoin> {
    ensure!(
        valid_node_id(&offer.node_id),
        "invalid Fleet member node id"
    );
    let paths = MemberPaths::new(base, config_root, instance)?;
    ensure_dir(&paths.root, PENDING_DIR_MODE)?;
    ensure_dir(&paths.pending, PENDING_DIR_MODE)?;

    if let Some(stored) = read_committed_offer(&paths)? {
        ensure!(
            stored == *offer,
            "a different Fleet Join offer is already pending for this instance"
        );
        let csr = read_regular(&paths.csr, MAX_CSR_BYTES, "pending Join CSR")?;
        let csr_pem = String::from_utf8(csr).context("pending Join CSR is not UTF-8")?;
        // The key must still be there and intact; a committed transaction with a missing key
        // cannot produce a first report and must not be silently replaced.
        let key = read_regular(&paths.key, MAX_KEY_BYTES, "pending member key")?;
        ensure!(
            String::from_utf8_lossy(&key).contains("-----BEGIN PRIVATE KEY-----")
                || String::from_utf8_lossy(&key).contains("-----BEGIN EC PRIVATE KEY-----"),
            "pending member key is not a private key"
        );
        let key_path = paths.key.clone();
        return Ok(PendingJoin {
            paths,
            offer: stored,
            csr_pem,
            key_path,
            reused: true,
        });
    }

    // No committed transaction: anything left behind is from an interrupted attempt (a crash
    // before `offer.json`). Those bytes cannot have been ACKed, so they are cleared rather than
    // reused, but an unexpected symlink or directory is refused instead of deleted.
    clear_uncommitted(&paths)?;

    let result = build_pending(&paths, offer);
    if result.is_err() {
        let _ = clear_uncommitted(&paths);
    }
    result
}

fn build_pending(paths: &MemberPaths, offer: &JoinOffer) -> Result<PendingJoin> {
    let key = pair_ca::run(
        &[
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
        ],
        None,
    )
    .context("generate the member private key")?;
    ensure!(
        key.len() as u64 <= MAX_KEY_BYTES,
        "generated member key is too large"
    );
    create_private(&paths.key, &key, SECRET_MODE)?;

    // `openssl req` cannot take the key and the config from the same stdin, so the challenge
    // password travels in a Mode 0600 config file that is removed before the transaction is
    // committed. It is never logged and never leaves the pending directory.
    let cnf = format!(
        "[ req ]\nprompt = no\ndistinguished_name = dn\nattributes = req_attributes\n\
         [ dn ]\nCN = {node}\n[ req_attributes ]\nchallengePassword = {nonce}\n",
        node = offer.node_id,
        nonce = offer.nonce
    );
    create_private(&paths.request_config, cnf.as_bytes(), SECRET_MODE)?;
    let csr = pair_ca::run(
        &[
            "req",
            "-new",
            "-key",
            &paths.key.to_string_lossy(),
            "-config",
            &paths.request_config.to_string_lossy(),
        ],
        None,
    );
    let _ = fs::remove_file(&paths.request_config);
    let csr = String::from_utf8(csr.context("create the member CSR")?)
        .context("member CSR is not UTF-8")?;
    ensure!(csr.len() as u64 <= MAX_CSR_BYTES, "member CSR is too large");
    // The Hub applies the same gate, so a CSR that fails here could never be signed.
    fleet_ca::validate_node_csr_with_challenge(&offer.node_id, &csr, &offer.nonce)
        .context("generated member CSR is not acceptable")?;

    create_private(&paths.csr, csr.as_bytes(), SECRET_MODE)?;
    let encoded = serde_json::to_vec(offer).context("encode the pending Join offer")?;
    create_private(&paths.offer, &encoded, SECRET_MODE)?;
    fsync_dir(&paths.pending)?;

    Ok(PendingJoin {
        offer: JoinOffer {
            v: offer.v,
            host: offer.host.clone(),
            port: offer.port,
            hub_name: offer.hub_name.clone(),
            hub_id: offer.hub_id.clone(),
            hub_pin: offer.hub_pin.clone(),
            offer_id: offer.offer_id.clone(),
            node_id: offer.node_id.clone(),
            token: offer.token.clone(),
            nonce: offer.nonce.clone(),
            expires: offer.expires,
        },
        csr_pem: csr,
        key_path: paths.key.clone(),
        reused: false,
        paths: paths.clone(),
    })
}

/// Validate the issued public material and publish it as an immutable generation.
pub(crate) fn publish_bundle(
    paths: &MemberPaths,
    offer: &JoinOffer,
    response: &JoinResponse,
    key_path: &Path,
) -> Result<InstalledBundle> {
    response
        .validate()
        .context("Fleet Join response is not a valid frame")?;
    ensure!(response.ok, "Fleet Join request was refused");
    ensure!(
        fleet_ca::valid_node_id(&offer.node_id),
        "invalid Fleet node id in Join offer"
    );
    ensure!(
        fleet_ca::valid_node_id(&offer.hub_id),
        "invalid Fleet Hub id in Join offer"
    );
    ensure!(
        offer.hub_name == fleet_ca::dns_name(&offer.hub_id),
        "Fleet Hub TLS identity in Join offer must be derived from hub_id"
    );
    let cert = response
        .member_certificate_pem
        .as_deref()
        .context("Fleet Join response has no member certificate")?;
    let ca = response
        .ca_certificate_pem
        .as_deref()
        .context("Fleet Join response has no CA certificate")?;
    let crl = response
        .crl_pem
        .as_deref()
        .context("Fleet Join response has no CRL")?;
    let hub_cert = response
        .hub_certificate_pem
        .as_deref()
        .context("Fleet Join response has no Hub certificate")?;
    for (label, pem) in [
        ("member certificate", cert),
        ("CA certificate", ca),
        ("Hub certificate", hub_cert),
    ] {
        single_pem("CERTIFICATE", pem).with_context(|| format!("{label} is not one PEM block"))?;
    }
    single_pem("X509 CRL", crl).context("CRL is not one PEM block")?;

    let key = read_regular(key_path, MAX_KEY_BYTES, "member private key")?;
    let key = String::from_utf8(key).context("member private key is not UTF-8")?;
    let key_public = openssl_stdin(&["pkey", "-pubout"], key.as_bytes())
        .context("read the member public key")?;
    let cert_public = openssl_stdin(&["x509", "-noout", "-pubkey"], cert.as_bytes())
        .context("read the issued certificate public key")?;
    ensure!(
        key_public.trim() == cert_public.trim(),
        "issued certificate does not belong to the local member key"
    );

    let subject = openssl_stdin(
        &["x509", "-noout", "-subject", "-nameopt", "RFC2253"],
        cert.as_bytes(),
    )
    .context("read the issued certificate subject")?;
    ensure!(
        subject.trim() == format!("subject=CN={}", offer.node_id),
        "issued certificate is not for this Fleet node id"
    );
    let der = pair_ca::run(&["x509", "-outform", "DER"], Some(cert.as_bytes()))
        .context("encode the issued member certificate")?;
    crate::fleet_ca::validate_identity_sans(
        &der,
        &fleet_ca::dns_name(&offer.node_id),
        &node_uri(&offer.node_id),
    )?;
    let eku = openssl_stdin(
        &["x509", "-noout", "-ext", "extendedKeyUsage"],
        cert.as_bytes(),
    )
    .context("read the issued certificate EKU")?;
    ensure!(
        eku.contains(EKU_FLEET_NODE),
        "issued certificate is not a fleet-node certificate"
    );
    openssl_stdin(&["crl", "-noout", "-text"], crl.as_bytes()).context("parse the CRL")?;
    let hub_der = pair_ca::run(&["x509", "-outform", "DER"], Some(hub_cert.as_bytes()))
        .context("encode the paired Hub certificate")?;
    crate::fleet_report::verify_hub_identity(&hub_der, &offer.hub_id)
        .context("paired Hub certificate identity does not match the Join offer")?;

    let leaf_sha256 = hex::encode(Sha256::digest(&der));
    let generation = paths.generations.join(&leaf_sha256);
    if symlink_metadata_opt(&generation)?.is_some() {
        return existing_generation(
            paths,
            &generation,
            &leaf_sha256,
            offer,
            cert,
            ca,
            crl,
            hub_cert,
            &key,
        );
    }

    ensure_dir(&paths.generations, PENDING_DIR_MODE)?;
    let staging = paths
        .generations
        .join(format!(".staging-{:x}", rand::random::<u64>()));
    publish_staging(&staging, &generation, cert, ca, crl, hub_cert, &key)?;

    Ok(InstalledBundle {
        cert: generation.join("member.crt"),
        key: generation.join("member.key"),
        ca: generation.join("ca.crt"),
        crl: generation.join("ca.crl"),
        hub_cert: generation.join("hub.crt"),
        node_id: offer.node_id.clone(),
        leaf_sha256,
        dir: generation,
    })
}

#[allow(clippy::too_many_arguments)]
fn existing_generation(
    _paths: &MemberPaths,
    generation: &Path,
    leaf_sha256: &str,
    offer: &JoinOffer,
    cert: &str,
    ca: &str,
    crl: &str,
    hub_cert: &str,
    key: &str,
) -> Result<InstalledBundle> {
    let meta = fs::symlink_metadata(generation)?;
    ensure!(
        meta.file_type().is_dir(),
        "refusing a non-directory generation path: {}",
        generation.display()
    );
    for (name, expected) in [
        ("member.crt", cert),
        ("ca.crt", ca),
        ("ca.crl", crl),
        ("hub.crt", hub_cert),
        ("member.key", key),
    ] {
        let path = generation.join(name);
        let stored = read_regular(&path, MAX_PEM_BYTES, name)?;
        ensure!(
            stored == expected.as_bytes(),
            "existing generation {} conflicts with the issued material",
            generation.display()
        );
    }
    Ok(InstalledBundle {
        cert: generation.join("member.crt"),
        key: generation.join("member.key"),
        ca: generation.join("ca.crt"),
        crl: generation.join("ca.crl"),
        hub_cert: generation.join("hub.crt"),
        node_id: offer.node_id.clone(),
        leaf_sha256: leaf_sha256.to_owned(),
        dir: generation.to_path_buf(),
    })
}

fn publish_staging(
    staging: &Path,
    generation: &Path,
    cert: &str,
    ca: &str,
    crl: &str,
    hub_cert: &str,
    key: &str,
) -> Result<()> {
    DirBuilder::new()
        .mode(PENDING_DIR_MODE)
        .create(staging)
        .with_context(|| format!("create staging directory {}", staging.display()))?;
    let result = (|| {
        for (name, bytes, mode) in [
            ("member.crt", cert.as_bytes(), PUBLIC_MODE),
            ("ca.crt", ca.as_bytes(), PUBLIC_MODE),
            ("ca.crl", crl.as_bytes(), PUBLIC_MODE),
            ("hub.crt", hub_cert.as_bytes(), PUBLIC_MODE),
            ("member.key", key.as_bytes(), KEY_MODE),
        ] {
            create_private(&staging.join(name), bytes, mode)?;
        }
        // Chain checks run on the staged files so a bad chain never reaches the generation name.
        verify_chain(
            &staging.join("ca.crt"),
            &staging.join("member.crt"),
            "member certificate",
        )?;
        verify_chain(
            &staging.join("ca.crt"),
            &staging.join("hub.crt"),
            "Hub certificate",
        )?;
        set_owner_and_mode(staging, PUBLISHED_DIR_MODE)?;
        fsync_dir(staging)?;
        fs::rename(staging, generation)
            .with_context(|| format!("publish generation {}", generation.display()))?;
        if let Some(parent) = generation.parent() {
            fsync_dir(parent)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

fn verify_chain(ca: &Path, leaf: &Path, label: &str) -> Result<()> {
    let output = pair_ca::run(
        &[
            "verify",
            "-CAfile",
            &ca.to_string_lossy(),
            &leaf.to_string_lossy(),
        ],
        None,
    )
    .with_context(|| format!("{label} does not chain to the Fleet CA"))?;
    let text = String::from_utf8_lossy(&output);
    ensure!(
        text.trim_end().ends_with("OK"),
        "{label} does not chain to the Fleet CA"
    );
    Ok(())
}

/// Atomically add or replace the Member section of `NAME.json`, preserving any Hub section.
pub(crate) fn write_member_config(
    paths: &MemberPaths,
    offer: &JoinOffer,
    bundle: &InstalledBundle,
) -> Result<()> {
    let dir = paths
        .config
        .parent()
        .context("the Fleet config path has no parent directory")?;
    let meta = fs::symlink_metadata(dir).with_context(|| format!("inspect {}", dir.display()))?;
    ensure!(
        meta.file_type().is_dir(),
        "refusing a symlinked or non-directory Fleet config root: {}",
        dir.display()
    );

    let mut root = match symlink_metadata_opt(&paths.config)? {
        None => serde_json::Map::new(),
        Some(meta) => {
            ensure!(
                meta.file_type().is_file() && meta.len() <= MAX_CONFIG_BYTES,
                "existing Fleet config must be a regular file of at most {MAX_CONFIG_BYTES} bytes"
            );
            let text =
                fs::read_to_string(&paths.config).context("read the existing Fleet config")?;
            crate::fleet_run::FleetConfig::parse(&text)
                .context("existing Fleet config is not valid; refusing to replace it")?;
            let value: Value =
                serde_json::from_str(&text).context("existing Fleet config is not JSON")?;
            value
                .as_object()
                .context("existing Fleet config is not a JSON object")?
                .clone()
        }
    };
    for key in root.keys() {
        ensure!(
            key == "hub" || key == "member",
            "existing Fleet config has an unknown field: {key}"
        );
    }

    let member = member_config_for(offer, bundle);
    ensure!(
        member.ca.is_absolute()
            && member.crl.is_absolute()
            && member.cert.is_absolute()
            && member.key.is_absolute(),
        "published Fleet member paths must be absolute"
    );
    root.insert(
        "member".to_owned(),
        serde_json::json!({
            "hub_addr": member.hub_addr,
            "hub_name": member.hub_name,
            "hub_id": member.hub_id,
            "ca": member.ca,
            "crl": member.crl,
            "cert": member.cert,
            "key": member.key,
        }),
    );

    let mut bytes =
        serde_json::to_vec_pretty(&Value::Object(root)).context("encode the Fleet config")?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "written Fleet config would exceed {MAX_CONFIG_BYTES} bytes"
    );

    let tmp = dir.join(format!(
        ".{}.json.tmp-{:x}",
        paths.instance,
        rand::random::<u64>()
    ));
    let result = (|| {
        create_private(&tmp, &bytes, CONFIG_MODE)?;
        fs::rename(&tmp, &paths.config).context("publish the Fleet config")?;
        fsync_dir(dir)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinOutcome {
    RestartQueued,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Join one Hub using the production root-managed Member store and Fleet helper control path.
pub(crate) async fn join_from_offer(instance: &str, encoded_offer: &str) -> Result<JoinOutcome> {
    let paths = MemberPaths::installed(instance)?;
    join_from_offer_with_paths(&paths, encoded_offer, Path::new(crate::apply::CONTROL_DIR)).await
}

/// Complete one Member Join transaction. The caller provides the already-bounded offer line.
pub(crate) async fn join_from_offer_with_paths(
    paths: &MemberPaths,
    encoded_offer: &str,
    control_dir: &Path,
) -> Result<JoinOutcome> {
    let offer = JoinOffer::decode(encoded_offer, unix_now())?;
    let base = paths
        .root
        .parent()
        .context("Member store path has no parent")?;
    let config_root = paths
        .config
        .parent()
        .context("Fleet config path has no parent")?;
    let pending = prepare_pending(base, config_root, &paths.instance, &offer)?;

    // The Join client authenticates the exact pinned Hub leaf and TLS identity before sending
    // this CSR. No Member private-key bytes are part of the transport request or response.
    let session = crate::fleet_join::join(&offer, &pending.csr_pem).await?;
    let response = session.response().clone();
    let bundle = match publish_bundle(&pending.paths, &offer, &response, &pending.key_path) {
        Ok(bundle) => bundle,
        Err(error) => {
            let _ = session.ack(false).await;
            return Err(error).context("validate and publish the Fleet Member bundle");
        }
    };
    if let Err(error) = write_member_config(&pending.paths, &offer, &bundle) {
        let _ = session.ack(false).await;
        return Err(error).context("publish the Fleet Member configuration");
    }

    // The Hub may bind Enrolled only after its later first mandatory-mTLS report. This ACK only
    // confirms that the public bundle and Member config are durable on this host.
    session
        .ack(true)
        .await
        .context("acknowledge durable Fleet Member installation")?;
    crate::fleet_helper::queue_agent_restart(control_dir, &paths.instance)
        .map_err(anyhow::Error::msg)
        .context("queue the per-instance Agent restart")?;
    finish_pending(&pending.paths).context("remove completed one-time Join state")?;
    Ok(JoinOutcome::RestartQueued)
}

/// Resume a pending transaction, or restart an already installed Member without a new offer.
pub(crate) async fn resume(instance: &str) -> Result<JoinOutcome> {
    let paths = MemberPaths::installed(instance)?;
    resume_with_paths(&paths, Path::new(crate::apply::CONTROL_DIR)).await
}

async fn resume_with_paths(paths: &MemberPaths, control_dir: &Path) -> Result<JoinOutcome> {
    if symlink_metadata_opt(&paths.offer)?.is_some() {
        let bytes = read_regular(&paths.offer, MAX_OFFER_BYTES, "pending Join offer")?;
        let offer: JoinOffer =
            serde_json::from_slice(&bytes).context("pending Join offer is not valid")?;
        let encoded = offer.encode()?;
        return join_from_offer_with_paths(paths, &encoded, control_dir).await;
    }

    let config = crate::fleet_run::FleetConfig::load(&paths.config)
        .context("no pending Join offer or valid installed Fleet config is available")?;
    ensure!(
        config.member.is_some(),
        "installed Fleet config does not contain a Member identity"
    );
    crate::fleet_helper::queue_agent_restart(control_dir, &paths.instance)
        .map_err(anyhow::Error::msg)
        .context("queue the per-instance Agent restart")?;
    finish_pending(paths).context("clean interrupted one-time Join state")?;
    Ok(JoinOutcome::RestartQueued)
}

/// Remove the one-time offer, CSR and pending key copy after a successful ACK and restart queue.
pub(crate) fn finish_pending(paths: &MemberPaths) -> Result<()> {
    for path in [&paths.offer, &paths.csr, &paths.key, &paths.request_config] {
        match symlink_metadata_opt(path)? {
            None => continue,
            Some(meta) => {
                ensure!(
                    meta.file_type().is_file(),
                    "refusing to remove a non-regular pending path: {}",
                    path.display()
                );
                fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
            }
        }
    }
    if symlink_metadata_opt(&paths.pending)?.is_some() {
        fsync_dir(&paths.pending)?;
    }
    Ok(())
}

fn read_committed_offer(paths: &MemberPaths) -> Result<Option<JoinOffer>> {
    match symlink_metadata_opt(&paths.offer)? {
        None => Ok(None),
        Some(meta) => {
            ensure!(
                meta.file_type().is_file(),
                "refusing a non-regular pending offer: {}",
                paths.offer.display()
            );
            ensure!(
                meta.len() <= MAX_OFFER_BYTES,
                "pending Join offer is too large"
            );
            let bytes = fs::read(&paths.offer).context("read the pending Join offer")?;
            let offer: JoinOffer =
                serde_json::from_slice(&bytes).context("pending Join offer is not valid")?;
            Ok(Some(offer))
        }
    }
}

fn clear_uncommitted(paths: &MemberPaths) -> Result<()> {
    for path in [&paths.offer, &paths.csr, &paths.key, &paths.request_config] {
        match symlink_metadata_opt(path)? {
            None => continue,
            Some(meta) => {
                ensure!(
                    meta.file_type().is_file(),
                    "refusing to clear a non-regular pending path: {}",
                    path.display()
                );
                fs::remove_file(path).with_context(|| format!("clear {}", path.display()))?;
            }
        }
    }
    Ok(())
}

fn symlink_metadata_opt(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
    }
}

fn ensure_dir(path: &Path, mode: u32) -> Result<()> {
    match symlink_metadata_opt(path)? {
        Some(meta) => {
            ensure!(
                meta.file_type().is_dir(),
                "refusing a symlinked or non-directory path: {}",
                path.display()
            );
            ensure!(
                meta.permissions().mode() & 0o777 == mode,
                "directory permissions must be {mode:04o}: {}",
                path.display()
            );
            ensure_root_owned(&meta, path)?;
        }
        None => {
            DirBuilder::new()
                .recursive(true)
                .mode(mode)
                .create(path)
                .with_context(|| format!("create {}", path.display()))?;
            fs::set_permissions(path, fs::Permissions::from_mode(mode))
                .with_context(|| format!("tighten {}", path.display()))?;
        }
    }
    Ok(())
}

fn ensure_root_owned(meta: &fs::Metadata, path: &Path) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Ok(());
    }
    use std::os::unix::fs::MetadataExt;
    ensure!(
        meta.uid() == 0,
        "path must be root-owned: {}",
        path.display()
    );
    Ok(())
}

fn read_regular(path: &Path, max: u64, label: &str) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)
        .with_context(|| format!("inspect {label} {}", path.display()))?;
    ensure!(
        meta.file_type().is_file(),
        "{label} must be a regular file, not a symlink: {}",
        path.display()
    );
    ensure!(
        meta.len() <= max,
        "{label} is too large: {}",
        path.display()
    );
    fs::read(path).with_context(|| format!("read {label} {}", path.display()))
}

/// Create a file that must not already exist, with an exact mode, synced before returning.
fn create_private(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    let result = (|| {
        file.write_all(bytes)?;
        apply_group(&file)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn set_owner_and_mode(dir: &Path, mode: u32) -> Result<()> {
    if let Some(gid) = service_gid()? {
        let file = std::fs::File::open(dir)?;
        std::os::unix::fs::fchown(&file, None, Some(gid))?;
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn apply_group(file: &std::fs::File) -> Result<()> {
    if let Some(gid) = service_gid()? {
        std::os::unix::fs::fchown(file, None, Some(gid))?;
    }
    Ok(())
}

fn service_gid() -> Result<Option<u32>> {
    if unsafe { libc::geteuid() } != 0 {
        return Ok(None);
    }
    let (_, gid) =
        crate::setup::service_ids().context("the baft-tunnel service account does not exist")?;
    Ok(Some(gid))
}

fn fsync_dir(dir: &Path) -> Result<()> {
    std::fs::File::open(dir)
        .and_then(|file| file.sync_all())
        .with_context(|| format!("sync {}", dir.display()))
}

fn single_pem(label: &str, pem: &str) -> Result<()> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    ensure!(
        pem.matches(&begin).count() == 1 && pem.matches(&end).count() == 1,
        "expected exactly one {label} PEM block"
    );
    ensure!(
        pem.trim_start().starts_with(&begin) && pem.trim_end().ends_with(&end),
        "{label} PEM block is not cleanly delimited"
    );
    Ok(())
}

fn openssl_stdin(args: &[&str], input: &[u8]) -> Result<String> {
    let out = pair_ca::run(args, Some(input))?;
    String::from_utf8(out).context("openssl output is not UTF-8")
}

/// The Member section this module writes. Kept as a value object so the config writer owns the
/// JSON shape and `fleet_run` keeps owning the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemberConfig {
    pub hub_addr: String,
    pub hub_name: String,
    pub hub_id: String,
    pub ca: PathBuf,
    pub crl: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
}

pub(crate) fn member_config_for(offer: &JoinOffer, bundle: &InstalledBundle) -> MemberConfig {
    MemberConfig {
        hub_addr: format!("{}:{}", offer.host, offer.port),
        hub_name: offer.hub_name.clone(),
        hub_id: offer.hub_id.clone(),
        ca: bundle.ca.clone(),
        crl: bundle.crl.clone(),
        cert: bundle.cert.clone(),
        key: bundle.key.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_join::Admission;
    use crate::{fleet_join, fleet_run};
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::{
        net::TcpListener,
        sync::{mpsc, watch},
    };

    const NODE_ID: &str = "edge-1";

    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "fleet-member-{tag}-{:x}-{n}-{:x}",
                std::process::id(),
                rand::random::<u64>()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        fn base(&self) -> PathBuf {
            self.0.join("fleet-member")
        }
        fn config_root(&self) -> PathBuf {
            let d = self.0.join("fleet");
            fs::create_dir_all(&d).unwrap();
            d
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn hex(bytes: usize, seed: u8) -> String {
        let mut out = String::with_capacity(bytes * 2);
        for i in 0..bytes {
            out.push_str(&format!("{:02x}", seed.wrapping_add(i as u8)));
        }
        out
    }

    fn offer(nonce: &str) -> JoinOffer {
        JoinOffer {
            v: 1,
            host: "hub.fleet.baft.invalid".to_owned(),
            port: 4443,
            hub_name: "hub-1.fleet.baft.invalid".to_owned(),
            hub_id: "hub-1".to_owned(),
            hub_pin: hex(32, 0x10),
            offer_id: hex(8, 0xa0),
            node_id: NODE_ID.to_owned(),
            token: hex(16, 0x40),
            nonce: nonce.to_owned(),
            expires: 4_000_000_000,
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn openssl(args: &[&str], stdin: Option<&[u8]>) -> String {
        String::from_utf8(pair_ca::run(args, stdin).unwrap()).unwrap()
    }

    fn ca(dir: &Path) -> PathBuf {
        let ca = dir.join("ca");
        fleet_ca::create(&ca, "test-fleet").unwrap();
        ca
    }

    fn issue_hub_certificate(ca_dir: &Path, hub_id: &str) -> String {
        let key_path = ca_dir.join(format!("{hub_id}.test.key"));
        let key = openssl(
            &["ecparam", "-name", "prime256v1", "-genkey", "-noout"],
            None,
        );
        fs::write(&key_path, key).unwrap();
        let csr = openssl(
            &[
                "req",
                "-new",
                "-key",
                &key_path.to_string_lossy(),
                "-subj",
                &format!("/CN={hub_id}"),
            ],
            None,
        );
        fleet_ca::sign_hub(ca_dir, hub_id, &csr).unwrap()
    }

    fn sign_member_with_dns(ca_dir: &Path, node_id: &str, csr_pem: &str, dns: &str) -> String {
        let ext_path = ca_dir.join("test-member.ext");
        let csr_path = ca_dir.join("test-member.csr");
        fs::write(
            &ext_path,
            format!(
                "[node_ext]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth,{EKU_FLEET_NODE}\nsubjectAltName=DNS:{dns},URI:{}\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n",
                fleet_ca::node_uri(node_id)
            ),
        )
        .unwrap();
        fs::write(&csr_path, csr_pem).unwrap();
        openssl(
            &[
                "ca",
                "-batch",
                "-notext",
                "-config",
                &ca_dir.join("openssl.cnf").to_string_lossy(),
                "-extfile",
                &ext_path.to_string_lossy(),
                "-extensions",
                "node_ext",
                "-days",
                "30",
                "-in",
                &csr_path.to_string_lossy(),
                "-out",
                "/dev/stdout",
            ],
            None,
        )
    }

    fn fixture(tag: &str) -> (Temp, MemberPaths, PendingJoin, JoinResponse) {
        let tmp = Temp::new(tag);
        let base = tmp.base();
        let cfg = tmp.config_root();
        let o = offer(&hex(16, 0x70));
        let pending = prepare_pending(&base, &cfg, "edge", &o).unwrap();
        let ca_dir = ca(tmp.path());
        let cert = fleet_ca::sign(&ca_dir, NODE_ID, &pending.csr_pem).unwrap();
        let read = |name: &str| fs::read_to_string(ca_dir.join(name)).unwrap();
        let response = JoinResponse {
            ok: true,
            member_certificate_pem: Some(cert),
            ca_certificate_pem: Some(read("ca.crt")),
            crl_pem: Some(read("ca.crl")),
            hub_certificate_pem: Some(issue_hub_certificate(&ca_dir, &o.hub_id)),
            reason: None,
        };
        (tmp, pending.paths.clone(), pending, response)
    }

    fn hub_material(root: &Path) -> (PathBuf, PathBuf, PathBuf, String) {
        let ca_dir = root.join("join-ca");
        fleet_ca::create(&ca_dir, "Member Join Test CA").unwrap();
        let hub_key = root.join("join-hub.key");
        let key = pair_ca::run(
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
        fs::write(&hub_key, key).unwrap();
        let csr = pair_ca::run(
            &[
                "req",
                "-new",
                "-key",
                &hub_key.to_string_lossy(),
                "-subj",
                "/CN=hub-1",
            ],
            None,
        )
        .unwrap();
        let cert_pem =
            fleet_ca::sign_hub(&ca_dir, "hub-1", &String::from_utf8(csr).unwrap()).unwrap();
        let hub_cert = root.join("join-hub.crt");
        fs::write(&hub_cert, &cert_pem).unwrap();
        let der = pair_ca::run(&["x509", "-outform", "DER"], Some(cert_pem.as_bytes())).unwrap();
        let pin = hex::encode(Sha256::digest(der));
        (ca_dir, hub_cert, hub_key, pin)
    }

    fn live_offer(address: std::net::SocketAddr, pin: String, expires: u64) -> JoinOffer {
        JoinOffer {
            v: 1,
            host: address.ip().to_string(),
            port: address.port(),
            hub_name: fleet_ca::dns_name("hub-1"),
            hub_id: "hub-1".to_owned(),
            hub_pin: pin,
            offer_id: hex(8, 0x10),
            node_id: NODE_ID.to_owned(),
            token: hex(16, 0x40),
            nonce: hex(16, 0x70),
            expires,
        }
    }

    fn unix_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[tokio::test]
    async fn resume_pending_join_reuses_csr_and_publishes_before_positive_ack() {
        let tmp = Temp::new("join-orchestration");
        let config_root = tmp.config_root();
        let paths = MemberPaths::new(&tmp.base(), &config_root, "edge").unwrap();
        let control = tmp.path().join("control");
        fs::create_dir(&control).unwrap();
        let (ca_dir, hub_cert, hub_key, pin) = hub_material(tmp.path());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 60;
        let offer = live_offer(address, pin, expires);
        let pending = prepare_pending(&tmp.base(), &config_root, "edge", &offer).unwrap();
        let original_csr = pending.csr_pem.clone();
        let acceptor = crate::tls::fleet_join_acceptor(&hub_cert, &hub_key).unwrap();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (ack_tx, mut ack_rx) = mpsc::unbounded_channel();
        let config_path = paths.config.clone();
        let generations = paths.generations.clone();
        let ca_for_admission = ca_dir.clone();
        let hub_cert_for_admission = hub_cert.clone();
        let expected_csr = original_csr.clone();
        let server = tokio::spawn(fleet_join::serve_shared(
            listener,
            acceptor,
            move |request, _peer| {
                let ca_dir = ca_for_admission.clone();
                let hub_cert = hub_cert_for_admission.clone();
                let expected_csr = expected_csr.clone();
                async move {
                    assert_eq!(
                        request.csr_pem, expected_csr,
                        "pending resume must reuse the exact CSR"
                    );
                    let wire = serde_json::to_string(&request).unwrap();
                    assert!(!wire.contains("PRIVATE KEY"));
                    assert!(
                        serde_json::from_str::<serde_json::Value>(&wire)
                            .unwrap()
                            .get("private_key")
                            .is_none()
                    );
                    let cert = fleet_ca::sign(&ca_dir, &request.node_id, &request.csr_pem).unwrap();
                    let response = JoinResponse {
                        ok: true,
                        member_certificate_pem: Some(cert),
                        ca_certificate_pem: Some(
                            fs::read_to_string(ca_dir.join("ca.crt")).unwrap(),
                        ),
                        crl_pem: Some(fs::read_to_string(ca_dir.join("ca.crl")).unwrap()),
                        hub_certificate_pem: Some(fs::read_to_string(hub_cert).unwrap()),
                        reason: None,
                    };
                    let response_wire = serde_json::to_string(&response).unwrap();
                    assert!(!response_wire.contains("PRIVATE KEY"));
                    assert!(
                        serde_json::from_str::<serde_json::Value>(&response_wire)
                            .unwrap()
                            .get("private_key")
                            .is_none()
                    );
                    Admission::Deliver { response, expires }
                }
            },
            move |offer_id, node_id| {
                let ack_tx = ack_tx.clone();
                let config_path = config_path.clone();
                let generations = generations.clone();
                async move {
                    assert!(
                        config_path.is_file(),
                        "positive ACK arrived before Member config publication"
                    );
                    assert!(generations.is_dir());
                    assert_eq!(
                        fs::read_dir(&generations).unwrap().count(),
                        1,
                        "positive ACK arrived before bundle publication"
                    );
                    let _ = ack_tx.send((offer_id, node_id));
                }
            },
            shutdown_rx,
        ));

        let outcome = resume_with_paths(&paths, &control).await.unwrap();
        assert_eq!(outcome, JoinOutcome::RestartQueued);
        assert_eq!(
            ack_rx.recv().await,
            Some((offer.offer_id, NODE_ID.to_owned()))
        );
        assert_eq!(
            fs::read(control.join("edge.fleet")).unwrap(),
            br#"{"op":"restart_agent"}"#
        );
        assert!(!paths.offer.exists());
        assert!(!paths.csr.exists());
        assert!(!paths.key.exists());
        let config = fleet_run::FleetConfig::load(&paths.config).unwrap();
        assert!(config.member.is_some());
        shutdown_tx.send(true).unwrap();
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn invalid_join_response_never_receives_a_positive_member_ack() {
        let tmp = Temp::new("join-invalid-response");
        let config_root = tmp.config_root();
        let paths = MemberPaths::new(&tmp.base(), &config_root, "edge").unwrap();
        let control = tmp.path().join("control");
        fs::create_dir(&control).unwrap();
        let (_ca_dir, hub_cert, hub_key, pin) = hub_material(tmp.path());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expires = unix_now() + 60;
        let offer = live_offer(address, pin, expires);
        let pending = prepare_pending(&tmp.base(), &config_root, "edge", &offer).unwrap();
        let original_csr = pending.csr_pem.clone();
        let acceptor = crate::tls::fleet_join_acceptor(&hub_cert, &hub_key).unwrap();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (ack_tx, mut ack_rx) = mpsc::unbounded_channel();
        let server = tokio::spawn(fleet_join::serve_shared(
            listener,
            acceptor,
            move |_request, _peer| async move {
                Admission::Deliver {
                    response: JoinResponse {
                        ok: true,
                        member_certificate_pem: Some("not-a-certificate".to_owned()),
                        ca_certificate_pem: None,
                        crl_pem: None,
                        hub_certificate_pem: None,
                        reason: None,
                    },
                    expires,
                }
            },
            move |offer_id, node_id| {
                let ack_tx = ack_tx.clone();
                async move {
                    let _ = ack_tx.send((offer_id, node_id));
                }
            },
            shutdown_rx,
        ));

        assert!(resume_with_paths(&paths, &control).await.is_err());
        assert!(
            ack_rx.try_recv().is_err(),
            "invalid response must not receive a positive ACK"
        );
        assert!(
            paths.offer.is_file(),
            "failed Join must retain the exact pending transaction"
        );
        assert_eq!(
            fs::read(&paths.csr).unwrap(),
            original_csr.as_bytes(),
            "pending resume must reuse the exact CSR"
        );
        assert!(
            !control.join("edge.fleet").exists(),
            "restart must not be queued after invalid response"
        );
        shutdown_tx.send(true).unwrap();
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn resume_installed_member_only_config_queues_restart_without_new_offer() {
        let tmp = Temp::new("resume-installed-member");
        let config_root = tmp.config_root();
        let paths = MemberPaths::new(&tmp.base(), &config_root, "edge").unwrap();
        let control = tmp.path().join("control");
        fs::create_dir(&control).unwrap();
        let bundle = tmp.path().join("installed-generation");
        fs::create_dir(&bundle).unwrap();
        for name in ["ca.crt", "ca.crl", "member.crt", "member.key"] {
            fs::write(bundle.join(name), b"test material").unwrap();
        }
        let config = serde_json::json!({
            "member": {
                "hub_addr": "127.0.0.1:4443",
                "hub_name": fleet_ca::dns_name("hub-1"),
                "hub_id": "hub-1",
                "ca": bundle.join("ca.crt"),
                "crl": bundle.join("ca.crl"),
                "cert": bundle.join("member.crt"),
                "key": bundle.join("member.key")
            }
        });
        fs::write(&paths.config, serde_json::to_vec(&config).unwrap()).unwrap();

        let outcome = resume_with_paths(&paths, &control).await.unwrap();
        assert_eq!(outcome, JoinOutcome::RestartQueued);
        assert_eq!(
            fs::read(control.join("edge.fleet")).unwrap(),
            br#"{"op":"restart_agent"}"#
        );
        assert!(
            !paths.offer.exists(),
            "installed resume must not create a new offer"
        );
    }

    #[test]
    fn paths_reject_unsafe_instance_names() {
        let tmp = Temp::new("names");
        for bad in ["", "edge/1", ".hidden", "-edge", "edge.1", &"e".repeat(64)] {
            assert!(
                MemberPaths::new(&tmp.base(), &tmp.config_root(), bad).is_err(),
                "{bad:?} must be refused"
            );
        }
        let ok = MemberPaths::new(&tmp.base(), &tmp.config_root(), "edge-1").unwrap();
        assert_eq!(ok.config.file_name().unwrap(), "edge-1.json");
        assert!(ok.root.ends_with("fleet-member/edge-1"));
    }

    #[test]
    fn prepare_pending_creates_root_only_state() {
        let tmp = Temp::new("prepare");
        let base = tmp.base();
        let cfg = tmp.config_root();
        let o = offer(&hex(16, 0x70));
        let pending = prepare_pending(&base, &cfg, "edge", &o).unwrap();
        assert!(!pending.reused);

        assert_eq!(mode(&pending.paths.root), PENDING_DIR_MODE);
        assert_eq!(mode(&pending.paths.pending), PENDING_DIR_MODE);
        for path in [&pending.paths.offer, &pending.paths.csr, &pending.paths.key] {
            let meta = fs::symlink_metadata(path).unwrap();
            assert!(meta.file_type().is_file(), "{}", path.display());
            assert_eq!(mode(path), SECRET_MODE, "{}", path.display());
        }

        let csr = fs::read_to_string(&pending.paths.csr).unwrap();
        assert_eq!(
            csr.matches("-----BEGIN CERTIFICATE REQUEST-----").count(),
            1
        );
        assert_eq!(csr, pending.csr_pem);

        let text = openssl(&["req", "-noout", "-text"], Some(csr.as_bytes()));
        let challenges: Vec<&str> = text
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(name, _)| name.trim() == "challengePassword")
            .map(|(_, value)| value.trim())
            .collect();
        assert_eq!(challenges, vec![o.nonce.as_str()]);

        let key = fs::read(&pending.paths.key).unwrap();
        let info = openssl(&["pkey", "-text", "-noout"], Some(&key));
        assert!(info.contains("prime256v1"), "key must be EC P-256");

        let stored: JoinOffer =
            serde_json::from_slice(&fs::read(&pending.paths.offer).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&stored).unwrap(),
            serde_json::to_value(&o).unwrap()
        );

        let names: Vec<String> = fs::read_dir(&pending.paths.pending)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 3, "no request.cnf may survive: {names:?}");
    }

    #[test]
    fn prepare_pending_reuses_the_exact_offer_and_csr() {
        let tmp = Temp::new("reuse");
        let base = tmp.base();
        let cfg = tmp.config_root();
        let o = offer(&hex(16, 0x70));
        let first = prepare_pending(&base, &cfg, "edge", &o).unwrap();
        let first_csr = first.csr_pem.clone();
        let first_key = fs::read(&first.paths.key).unwrap();

        let second = prepare_pending(&base, &cfg, "edge", &o).unwrap();
        assert!(second.reused);
        assert_eq!(
            second.csr_pem, first_csr,
            "retry must reuse the same CSR bytes"
        );
        assert_eq!(fs::read(&second.paths.key).unwrap(), first_key);
        assert_eq!(
            fs::read(&second.paths.csr).unwrap(),
            first_csr.as_bytes().to_vec()
        );
    }

    #[test]
    fn prepare_pending_refuses_a_different_offer() {
        let tmp = Temp::new("mismatch");
        let base = tmp.base();
        let cfg = tmp.config_root();
        let first = prepare_pending(&base, &cfg, "edge", &offer(&hex(16, 0x70))).unwrap();
        let csr = first.csr_pem.clone();

        let err = prepare_pending(&base, &cfg, "edge", &offer(&hex(16, 0x80)));
        assert!(
            err.is_err(),
            "a different nonce must not overwrite pending state"
        );
        assert_eq!(fs::read_to_string(&first.paths.csr).unwrap(), csr);

        let mut other = offer(&hex(16, 0x70));
        other.offer_id = hex(8, 0xb1);
        assert!(prepare_pending(&base, &cfg, "edge", &other).is_err());
    }

    #[test]
    fn prepare_pending_refuses_a_symlinked_member_root() {
        let tmp = Temp::new("symlink");
        let base = tmp.base();
        let cfg = tmp.config_root();
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::create_dir_all(&base).unwrap();
        std::os::unix::fs::symlink(&elsewhere, base.join("edge")).unwrap();

        assert!(prepare_pending(&base, &cfg, "edge", &offer(&hex(16, 0x70))).is_err());
        assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
    }

    #[test]
    fn prepare_pending_clears_uncommitted_files() {
        let tmp = Temp::new("stray");
        let base = tmp.base();
        let cfg = tmp.config_root();
        let paths = MemberPaths::new(&base, &cfg, "edge").unwrap();
        fs::create_dir_all(&paths.pending).unwrap();
        fs::set_permissions(&paths.root, fs::Permissions::from_mode(PENDING_DIR_MODE)).unwrap();
        fs::set_permissions(&paths.pending, fs::Permissions::from_mode(PENDING_DIR_MODE)).unwrap();
        // A crash before offer.json: key written, transaction not committed.
        fs::write(&paths.key, b"-----BEGIN PRIVATE KEY-----\nstale\n").unwrap();
        fs::set_permissions(&paths.key, fs::Permissions::from_mode(SECRET_MODE)).unwrap();

        let pending = prepare_pending(&base, &cfg, "edge", &offer(&hex(16, 0x70))).unwrap();
        assert!(!pending.reused);
        assert_ne!(
            fs::read(&pending.paths.key).unwrap(),
            b"-----BEGIN PRIVATE KEY-----\nstale\n".to_vec()
        );
        assert!(fs::read_to_string(&pending.paths.offer).is_ok());
    }

    #[test]
    fn publish_bundle_publishes_an_immutable_generation() {
        let (_tmp, paths, pending, response) = fixture("publish");
        let bundle = publish_bundle(&paths, &pending.offer, &response, &pending.key_path).unwrap();

        assert_eq!(bundle.node_id, NODE_ID);
        assert_eq!(bundle.leaf_sha256.len(), 64);
        assert_eq!(
            bundle.dir.file_name().unwrap().to_string_lossy(),
            bundle.leaf_sha256
        );
        assert!(bundle.dir.starts_with(&paths.generations));
        assert_eq!(mode(&bundle.dir), PUBLISHED_DIR_MODE);
        assert_eq!(mode(&bundle.key), KEY_MODE);
        for public in [&bundle.cert, &bundle.ca, &bundle.crl, &bundle.hub_cert] {
            assert_eq!(mode(public), PUBLIC_MODE, "{}", public.display());
        }
        assert_eq!(
            fs::read_to_string(&bundle.cert).unwrap(),
            response.member_certificate_pem.as_deref().unwrap()
        );

        let mut entries: Vec<String> = fs::read_dir(&paths.generations)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        entries.sort();
        assert_eq!(entries, vec![bundle.leaf_sha256.clone()]);

        // Idempotent: republishing the same bundle returns the same immutable generation.
        let again = publish_bundle(&paths, &pending.offer, &response, &pending.key_path).unwrap();
        assert_eq!(again.dir, bundle.dir);
        assert_eq!(fs::read_dir(&paths.generations).unwrap().count(), 1);
    }

    #[test]
    fn publish_bundle_refuses_a_member_certificate_with_mismatched_dns_identity() {
        let (_tmp, paths, pending, mut response) = fixture("mismatched-dns");
        let ca_dir = _tmp.path().join("ca");
        response.member_certificate_pem = Some(sign_member_with_dns(
            &ca_dir,
            NODE_ID,
            &pending.csr_pem,
            "other.fleet.baft.invalid",
        ));

        assert!(publish_bundle(&paths, &pending.offer, &response, &pending.key_path).is_err());
        assert_no_generation(&paths);
    }

    #[test]
    fn publish_bundle_refuses_a_hub_certificate_for_another_identity() {
        let (_tmp, paths, pending, mut response) = fixture("wrong-hub-identity");
        let ca_dir = _tmp.path().join("ca");
        response.hub_certificate_pem = Some(issue_hub_certificate(&ca_dir, "hub-2"));

        assert!(publish_bundle(&paths, &pending.offer, &response, &pending.key_path).is_err());
        assert_no_generation(&paths);
    }

    #[test]
    fn publish_bundle_refuses_a_certificate_for_another_key() {
        let (_tmp, paths, pending, mut response) = fixture("wrongkey");
        let ca_dir = ca(&_tmp.path().join("other"));
        let foreign_key = openssl(
            &[
                "genpkey",
                "-algorithm",
                "EC",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
            ],
            None,
        );
        let dir = _tmp.path().join("foreign");
        fs::create_dir_all(&dir).unwrap();
        let key_path = dir.join("k.pem");
        fs::write(&key_path, foreign_key).unwrap();
        let csr = openssl(
            &[
                "req",
                "-new",
                "-key",
                &key_path.to_string_lossy(),
                "-subj",
                &format!("/CN={NODE_ID}"),
            ],
            None,
        );
        response.member_certificate_pem = Some(fleet_ca::sign(&ca_dir, NODE_ID, &csr).unwrap());

        assert!(publish_bundle(&paths, &pending.offer, &response, &pending.key_path).is_err());
        assert_no_generation(&paths);
    }

    #[test]
    fn publish_bundle_refuses_a_certificate_that_does_not_chain_to_the_ca() {
        let (_tmp, paths, pending, mut response) = fixture("wrongca");
        let foreign_ca = ca(&_tmp.path().join("foreign-ca"));
        let csr = pending.csr_pem.clone();
        response.member_certificate_pem = Some(fleet_ca::sign(&foreign_ca, NODE_ID, &csr).unwrap());

        assert!(publish_bundle(&paths, &pending.offer, &response, &pending.key_path).is_err());
        assert_no_generation(&paths);
    }

    #[test]
    fn publish_bundle_refuses_a_symlinked_key() {
        let (_tmp, paths, pending, response) = fixture("symlink-key");
        let real = pending.paths.root.join("real.key");
        fs::copy(&pending.key_path, &real).unwrap();
        fs::remove_file(&pending.key_path).unwrap();
        std::os::unix::fs::symlink(&real, &pending.key_path).unwrap();

        assert!(publish_bundle(&paths, &pending.offer, &response, &pending.key_path).is_err());
        assert_no_generation(&paths);
    }

    fn assert_no_generation(paths: &MemberPaths) {
        if let Ok(entries) = fs::read_dir(&paths.generations) {
            let names: Vec<String> = entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert!(
                names.is_empty(),
                "no generation or staging may remain: {names:?}"
            );
        }
    }

    #[test]
    fn write_member_config_preserves_the_hub_section() {
        let (_tmp, paths, pending, response) = fixture("config");
        let bundle = publish_bundle(&paths, &pending.offer, &response, &pending.key_path).unwrap();
        let ca_dir = ca(&_tmp.path().join("hub-ca"));
        let hub = serde_json::json!({
            "listen": "10.0.0.1:7443",
            "ca": ca_dir.join("ca.crt"),
            "crl": ca_dir.join("ca.crl"),
            "cert": ca_dir.join("ca.crt"),
            "key_credential": crate::fleet_helper::HUB_CREDENTIAL_ID
        });
        fs::write(
            &paths.config,
            serde_json::to_vec_pretty(&serde_json::json!({ "hub": hub.clone() })).unwrap(),
        )
        .unwrap();
        fs::set_permissions(&paths.config, fs::Permissions::from_mode(CONFIG_MODE)).unwrap();

        write_member_config(&paths, &pending.offer, &bundle).unwrap();

        let text = fs::read_to_string(&paths.config).unwrap();
        crate::fleet_run::FleetConfig::parse(&text).expect("written config must parse");
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["hub"], hub,
            "the Hub section must be preserved verbatim"
        );
        assert_eq!(
            value["member"]["cert"],
            bundle.cert.to_string_lossy().as_ref()
        );
        assert_eq!(
            value["member"]["key"],
            bundle.key.to_string_lossy().as_ref()
        );
        assert_eq!(value["member"]["ca"], bundle.ca.to_string_lossy().as_ref());
        assert_eq!(
            value["member"]["crl"],
            bundle.crl.to_string_lossy().as_ref()
        );
        assert_eq!(value["member"]["hub_id"], pending.offer.hub_id);
        assert_eq!(
            value["member"]["hub_addr"],
            format!("{}:{}", pending.offer.host, pending.offer.port)
        );
        assert_eq!(mode(&paths.config), CONFIG_MODE);

        let leftovers: Vec<String> = fs::read_dir(paths.config.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temporary config may remain: {leftovers:?}"
        );
    }

    #[test]
    fn write_member_config_refuses_malformed_or_unknown_existing_config() {
        let (_tmp, paths, pending, response) = fixture("bad-config");
        let bundle = publish_bundle(&paths, &pending.offer, &response, &pending.key_path).unwrap();

        for bad in [
            b"not json".to_vec(),
            serde_json::to_vec(&serde_json::json!({
                "hub": { "listen": "1.2.3.4:5" }, "extra": true
            }))
            .unwrap(),
            serde_json::to_vec(&serde_json::json!({ "member": {} })).unwrap(),
        ] {
            fs::write(&paths.config, &bad).unwrap();
            assert!(
                write_member_config(&paths, &pending.offer, &bundle).is_err(),
                "unexpected acceptance of {}",
                String::from_utf8_lossy(&bad)
            );
            assert_eq!(
                fs::read(&paths.config).unwrap(),
                bad,
                "user config must be untouched"
            );
        }
    }

    #[test]
    fn finish_pending_removes_only_the_one_time_secrets() {
        let (_tmp, paths, pending, response) = fixture("finish");
        let bundle = publish_bundle(&paths, &pending.offer, &response, &pending.key_path).unwrap();
        write_member_config(&paths, &pending.offer, &bundle).unwrap();
        fs::write(&paths.request_config, b"[ req ]\n").unwrap();

        finish_pending(&paths).unwrap();

        for gone in [&paths.offer, &paths.csr, &paths.key, &paths.request_config] {
            assert!(
                fs::symlink_metadata(gone).is_err(),
                "{} must be removed",
                gone.display()
            );
        }
        assert!(bundle.dir.is_dir());
        assert_eq!(mode(&bundle.key), KEY_MODE);
        assert!(paths.pending.is_dir());
        assert!(paths.config.is_file());
        // Idempotent: a second finish is a no-op, not an error.
        finish_pending(&paths).unwrap();
    }
}
