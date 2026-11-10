//! Bounded root-only CSR enrollment helper (ADR-0027 D2-B1).
//!
//! This protocol is intentionally separate from setup/lifecycle requests. It creates or reads
//! only the public CSR; private key bytes never enter a request, receipt, Agent response, or log.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const REQUEST_SUFFIX: &str = "pki";
pub const RESULT_SUFFIX: &str = "pki-result";
pub const RECEIPTS_DIR: &str = "/var/lib/baft-tunnel/pki-receipts";
pub const MAX_REQUEST_BYTES: u64 = 16 * 1024;
pub const MAX_CSR_BYTES: u64 = 16 * 1024;
const OPENSSL: &str = "/usr/bin/openssl";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PkiOperation {
    CreateCsr,
    ReadCsr,
}

impl PkiOperation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CreateCsr => "create_csr",
            Self::ReadCsr => "read_csr",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PkiRequest {
    request_version: u8,
    request_id: String,
    operation: PkiOperation,
    node_id: String,
    server_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CsrMetadata {
    metadata_version: u8,
    node_id: String,
    expected_server_san: String,
    public_key_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredReceipt {
    request_digest: String,
    receipt: Value,
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn valid_node_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(crate) fn expected_server_san(value: &str) -> Result<String> {
    if let Ok(ip) = value.parse::<std::net::IpAddr>() {
        return Ok(format!("IP:{ip}"));
    }
    ensure!(
        value.len() <= 253 && value.bytes().any(|b| b.is_ascii_alphabetic()),
        "server name must be a DNS name or IP literal"
    );
    for label in value.split('.') {
        ensure!(
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "server name must be a valid DNS name or IP literal"
        );
    }
    Ok(format!("DNS:{value}"))
}

fn run_openssl(args: &[&std::ffi::OsStr]) -> Result<Output> {
    let output = Command::new(OPENSSL)
        .args(args)
        .output()
        .with_context(|| format!("start fixed OpenSSL executable {OPENSSL}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("OpenSSL operation failed: {}", stderr.trim());
    }
    Ok(output)
}

fn openssl_args<'a>(args: &'a [&'a str]) -> Vec<&'a std::ffi::OsStr> {
    args.iter().map(std::ffi::OsStr::new).collect()
}

fn read_regular(path: &Path, limit: u64, private: bool) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open {} without following symlinks", path.display()))?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "{} is not a regular file", path.display());
    ensure!(
        meta.len() <= limit,
        "{} exceeds {limit} bytes",
        path.display()
    );
    if private {
        ensure!(
            meta.permissions().mode() & 0o077 == 0,
            "{} has unsafe group/other permissions",
            path.display()
        );
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "{} grew beyond {limit} bytes",
        path.display()
    );
    Ok(bytes)
}

fn read_public_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| {
            format!(
                "open public PKI file {} without following symlinks",
                path.display()
            )
        })?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "{} is not a regular file", path.display());
    ensure!(
        meta.len() <= limit,
        "{} exceeds {limit} bytes",
        path.display()
    );
    ensure!(
        meta.permissions().mode() & 0o7777 == 0o644,
        "{} must have public-file mode 0644",
        path.display()
    );
    if let Some((_, gid)) = crate::setup::service_ids() {
        ensure!(
            meta.uid() == 0 && meta.gid() == gid,
            "{} must be owned by root:baft-tunnel",
            path.display()
        );
    } else if unsafe { libc::geteuid() } == 0 {
        bail!("the baft-tunnel service identity is missing; refusing public PKI file");
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "{} grew beyond {limit} bytes",
        path.display()
    );
    Ok(bytes)
}

fn command_output(args: &[&str]) -> Result<Vec<u8>> {
    Ok(run_openssl(&openssl_args(args))?.stdout)
}

fn public_key_der_from_key(key: &Path) -> Result<Vec<u8>> {
    let args = [
        "pkey",
        "-in",
        key.to_str().context("non-UTF8 key path")?,
        "-pubout",
        "-outform",
        "DER",
    ];
    command_output(&args)
}

fn public_key_fingerprint_from_key(key: &Path) -> Result<String> {
    Ok(sha256(&public_key_der_from_key(key)?))
}

fn is_p256_subject_public_key_info(der: &[u8]) -> bool {
    // OpenSSL normally encodes the named-curve OID. LibreSSL may encode the same P-256
    // parameters explicitly; in that case verify the complete defining parameters.
    const P256_OID: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
    if der.windows(P256_OID.len()).any(|window| window == P256_OID) {
        return true;
    }
    const EXPLICIT_P256: [&str; 6] = [
        "022100ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
        "0420ffffffff00000001000000000000000000000000fffffffffffffffffffffffc",
        "04205ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b",
        "0441046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        "022100ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
        "020101",
    ];
    EXPLICIT_P256.iter().all(|hex_value| {
        hex::decode(hex_value)
            .is_ok_and(|marker| der.windows(marker.len()).any(|window| window == marker))
    })
}

fn validate_csr(csr: &Path, node_id: &str, key: &Path, scratch: &Path) -> Result<String> {
    let csr_text = fs::read_to_string(csr).context("read generated CSR")?;
    ensure!(
        csr_text.starts_with("-----BEGIN CERTIFICATE REQUEST-----")
            && csr_text
                .trim_end()
                .ends_with("-----END CERTIFICATE REQUEST-----"),
        "CSR is not a single PEM certificate request"
    );
    let verify_args = [
        "req",
        "-in",
        csr.to_str().context("non-UTF8 CSR path")?,
        "-verify",
        "-noout",
    ];
    run_openssl(&openssl_args(&verify_args))?;

    let subject_args = [
        "req",
        "-in",
        csr.to_str().context("non-UTF8 CSR path")?,
        "-noout",
        "-subject",
        "-nameopt",
        "RFC2253",
    ];
    let subject = String::from_utf8(command_output(&subject_args)?)?;
    ensure!(
        subject.trim() == format!("subject=CN={node_id}"),
        "CSR subject CN does not exactly match requested node ID"
    );

    let key_der = public_key_der_from_key(key)?;
    ensure!(
        is_p256_subject_public_key_info(&key_der),
        "node private key must be EC P-256"
    );

    let csr_pub = scratch.join("csr-public.pem");
    let csr_pub_args = [
        "req",
        "-in",
        csr.to_str().context("non-UTF8 CSR path")?,
        "-pubkey",
        "-noout",
        "-out",
        csr_pub.to_str().context("non-UTF8 CSR public key path")?,
    ];
    run_openssl(&openssl_args(&csr_pub_args))?;
    let csr_der_args = [
        "pkey",
        "-pubin",
        "-in",
        csr_pub.to_str().context("non-UTF8 CSR public key path")?,
        "-outform",
        "DER",
    ];
    let csr_der = command_output(&csr_der_args)?;
    ensure!(
        is_p256_subject_public_key_info(&csr_der),
        "CSR public key must be EC P-256"
    );
    let key_fp = public_key_fingerprint_from_key(key)?;
    ensure!(
        sha256(&csr_der) == key_fp,
        "CSR public key does not match the generated private key"
    );
    let _ = fs::remove_file(csr_pub);
    Ok(key_fp)
}

fn chown_file(path: &Path, uid: u32, gid: u32) -> Result<()> {
    let cpath = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    // SAFETY: cpath is NUL-terminated and points to a path; ownership IDs are obtained locally.
    let rc = unsafe { libc::chown(cpath.as_ptr(), uid, gid) };
    ensure!(
        rc == 0,
        "cannot set fixed service ownership on {}",
        path.display()
    );
    Ok(())
}

fn apply_service_ownership(key: &Path, csr: &Path) -> Result<()> {
    if let Some((uid, gid)) = crate::setup::service_ids() {
        chown_file(key, uid, gid)?;
        chown_file(csr, 0, gid)?;
    } else if unsafe { libc::geteuid() } == 0 {
        bail!("the baft-tunnel service identity is missing; refusing to install a node key");
    }
    fs::set_permissions(key, fs::Permissions::from_mode(0o600))?;
    fs::set_permissions(csr, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

fn ensure_directory(config_dir: &Path, enforce_fixed: bool) -> Result<()> {
    let meta = fs::symlink_metadata(config_dir)
        .with_context(|| format!("inspect fixed PKI directory {}", config_dir.display()))?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "fixed PKI path is not a real directory"
    );
    if enforce_fixed {
        ensure!(
            config_dir == Path::new("/etc/baft-tunnel"),
            "PKI helper only serves /etc/baft-tunnel"
        );
    }
    Ok(())
}

fn create_csr_at(config_dir: &Path, req: &PkiRequest, enforce_fixed: bool) -> Result<Value> {
    ensure_directory(config_dir, enforce_fixed)?;
    let key = config_dir.join("node.key");
    let csr = config_dir.join("node.csr");
    let metadata_path = config_dir.join("node.csr.meta");
    for path in [&key, &csr, &metadata_path, &config_dir.join("node.crt")] {
        ensure!(
            fs::symlink_metadata(path).is_err(),
            "{} already exists; CSR generation never overwrites credentials",
            path.display()
        );
    }

    let scratch = config_dir.join(format!(".pki-enroll-{}", req.request_id));
    fs::create_dir(&scratch).context("create private CSR staging directory")?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
    let staged_key = scratch.join("node.key");
    let staged_csr = scratch.join("node.csr");
    let staged_metadata = scratch.join("node.csr.meta");
    let mut installed_key = false;
    let mut installed_csr = false;
    let mut installed_metadata = false;
    let result = (|| -> Result<Value> {
        let gen_args = [
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-out",
            staged_key.to_str().context("non-UTF8 staged key path")?,
        ];
        run_openssl(&openssl_args(&gen_args))?;
        fs::set_permissions(&staged_key, fs::Permissions::from_mode(0o600))?;

        let req_args = [
            "req",
            "-new",
            "-key",
            staged_key.to_str().context("non-UTF8 staged key path")?,
            "-out",
            staged_csr.to_str().context("non-UTF8 staged CSR path")?,
            "-subj",
            &format!("/CN={}", req.node_id),
        ];
        run_openssl(&openssl_args(&req_args))?;
        fs::set_permissions(&staged_csr, fs::Permissions::from_mode(0o600))?;
        let fingerprint = validate_csr(&staged_csr, &req.node_id, &staged_key, &scratch)?;
        File::open(&staged_key)?.sync_all()?;
        File::open(&staged_csr)?.sync_all()?;
        let csr_bytes = read_regular(&staged_csr, MAX_CSR_BYTES, true)?;
        let server_san = expected_server_san(&req.server_name)?;
        apply_service_ownership(&staged_key, &staged_csr)?;
        let metadata = CsrMetadata {
            metadata_version: 1,
            node_id: req.node_id.clone(),
            expected_server_san: server_san.clone(),
            public_key_sha256: fingerprint.clone(),
        };
        let metadata_bytes = serde_json::to_vec(&metadata)?;
        let mut metadata_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged_metadata)?;
        metadata_file.write_all(&metadata_bytes)?;
        metadata_file.sync_all()?;
        drop(metadata_file);
        if let Some((_, gid)) = crate::setup::service_ids() {
            chown_file(&staged_metadata, 0, gid)?;
        } else if unsafe { libc::geteuid() } == 0 {
            bail!("the baft-tunnel service identity is missing; refusing to install CSR metadata");
        }
        fs::set_permissions(&staged_metadata, fs::Permissions::from_mode(0o644))?;

        // Hard links provide create-if-absent semantics; never replace a raced or pre-existing file.
        fs::hard_link(&staged_key, &key).context("install private key without overwrite")?;
        installed_key = true;
        fs::hard_link(&staged_csr, &csr).context("install CSR without overwrite")?;
        installed_csr = true;
        fs::hard_link(&staged_metadata, &metadata_path)
            .context("install CSR metadata without overwrite")?;
        installed_metadata = true;
        File::open(config_dir)?.sync_all()?;
        Ok(json!({
            "result": "created",
            "node_id": req.node_id,
            "expected_server_san": server_san,
            "public_key_sha256": fingerprint,
            "csr_pem": String::from_utf8(csr_bytes)?,
            "private_key_exported": false
        }))
    })();
    if result.is_err() {
        if installed_metadata {
            let _ = fs::remove_file(&metadata_path);
        }
        if installed_csr {
            let _ = fs::remove_file(&csr);
        }
        if installed_key {
            let _ = fs::remove_file(&key);
        }
        let _ = File::open(config_dir).and_then(|dir| dir.sync_all());
    }
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn read_csr_at(config_dir: &Path, req: &PkiRequest, enforce_fixed: bool) -> Result<Value> {
    ensure_directory(config_dir, enforce_fixed)?;
    let key = config_dir.join("node.key");
    let csr = config_dir.join("node.csr");
    let metadata_path = config_dir.join("node.csr.meta");
    let key_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&key)
        .context("open node private key without following symlinks")?;
    let key_meta = key_file.metadata()?;
    ensure!(key_meta.is_file(), "node private key is not a regular file");
    ensure!(
        key_meta.permissions().mode() & 0o7777 == 0o600,
        "node private key must have mode 0600"
    );
    if let Some((uid, gid)) = crate::setup::service_ids() {
        ensure!(
            key_meta.uid() == uid && key_meta.gid() == gid,
            "node private key does not have baft-tunnel service ownership"
        );
    } else if unsafe { libc::geteuid() } == 0 {
        bail!("the baft-tunnel service identity is missing; refusing CSR retrieval");
    }
    drop(key_file);
    let csr_bytes = read_public_file(&csr, MAX_CSR_BYTES)?;
    let metadata_bytes = read_public_file(&metadata_path, 4096)?;
    let metadata: CsrMetadata =
        serde_json::from_slice(&metadata_bytes).context("stored CSR metadata is invalid")?;
    ensure!(
        metadata.metadata_version == 1,
        "unsupported CSR metadata version"
    );
    ensure!(
        metadata.node_id == req.node_id,
        "requested node ID does not match the stored CSR"
    );
    ensure!(
        metadata.expected_server_san == expected_server_san(&req.server_name)?,
        "requested server SAN does not match the stored CSR enrollment"
    );
    ensure!(
        String::from_utf8_lossy(&csr_bytes).starts_with("-----BEGIN CERTIFICATE REQUEST-----"),
        "stored CSR is not a PEM certificate request"
    );
    let scratch = config_dir.join(format!(".pki-read-{}", req.request_id));
    fs::create_dir(&scratch).context("create private CSR validation directory")?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
    let result = (|| -> Result<Value> {
        let staged_csr = scratch.join("node.csr");
        fs::write(&staged_csr, &csr_bytes)?;
        fs::set_permissions(&staged_csr, fs::Permissions::from_mode(0o600))?;
        let fingerprint = validate_csr(&staged_csr, &req.node_id, &key, &scratch)?;
        ensure!(
            fingerprint == metadata.public_key_sha256,
            "stored CSR metadata fingerprint does not match the local key"
        );
        Ok(json!({
            "result": "retrieved",
            "node_id": req.node_id,
            "expected_server_san": expected_server_san(&req.server_name)?,
            "public_key_sha256": fingerprint,
            "csr_pem": String::from_utf8(csr_bytes)?,
            "private_key_exported": false
        }))
    })();
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn read_helper_receipt(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| {
            format!(
                "open PKI helper receipt {} without following symlinks",
                path.display()
            )
        })?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "PKI helper receipt is not a regular file");
    ensure!(
        meta.len() <= limit,
        "PKI helper receipt exceeds {limit} bytes"
    );
    ensure!(
        meta.permissions().mode() & 0o7777 == 0o640,
        "PKI helper receipt must have mode 0640"
    );
    if let Some((_, gid)) = crate::setup::service_ids() {
        ensure!(
            meta.uid() == 0 && meta.gid() == gid,
            "PKI helper receipt is not owned by root:baft-tunnel"
        );
    } else if unsafe { libc::geteuid() } == 0 {
        bail!("the baft-tunnel service identity is missing; refusing PKI helper receipt");
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "PKI helper receipt grew beyond {limit} bytes"
    );
    Ok(bytes)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "request/receipt is not a regular file");
    ensure!(meta.len() <= limit, "request/receipt exceeds {limit} bytes");
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "request/receipt grew beyond {limit} bytes"
    );
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8], mode: u32, group: Option<u32>) -> Result<()> {
    let parent = path.parent().context("file has no parent directory")?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    match fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("remove stale temporary receipt"),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Some(gid) = group {
        // SAFETY: fchown operates on the already-open temporary file descriptor.
        let rc = unsafe { libc::fchown(std::os::fd::AsRawFd::as_raw_fd(&file), u32::MAX, gid) };
        ensure!(rc == 0, "cannot set receipt group ownership");
    }
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    drop(file);
    fs::rename(&tmp, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn refused(request_id: Option<&str>, operation: Option<PkiOperation>, reason: String) -> Value {
    json!({
        "receipt_version": 1,
        "request_id": request_id,
        "operation": operation,
        "result": "refused",
        "reason": reason,
        "private_key_exported": false
    })
}

fn parse_request(bytes: &[u8]) -> Result<PkiRequest> {
    let req: PkiRequest = serde_json::from_slice(bytes).context("PKI request is not valid")?;
    ensure!(req.request_version == 1, "unsupported PKI request version");
    ensure!(
        crate::apply::valid_key(&req.request_id),
        "invalid PKI request ID"
    );
    ensure!(valid_node_id(&req.node_id), "invalid node ID");
    expected_server_san(&req.server_name)?;
    Ok(req)
}

fn write_result(control_dir: &Path, name: &str, receipt: &Value) -> Result<()> {
    let path = control_dir.join(format!("{name}.{RESULT_SUFFIX}"));
    let tmp = control_dir.join(format!(".{name}.{RESULT_SUFFIX}.tmp"));
    for p in [&path, &tmp] {
        match fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("remove {}", p.display())),
        }
    }
    let bytes = serde_json::to_vec(receipt)?;
    write_atomic(
        &tmp,
        &bytes,
        0o640,
        crate::setup::service_ids().map(|(_, gid)| gid),
    )?;
    fs::rename(&tmp, &path)?;
    File::open(control_dir)?.sync_all()?;
    Ok(())
}

pub fn run(name: &str, control_dir: &Path, config_dir: &Path, receipts: &Path) -> Result<Value> {
    run_at(name, control_dir, config_dir, receipts, true)
}

fn run_at(
    name: &str,
    control_dir: &Path,
    config_dir: &Path,
    receipts: &Path,
    enforce_fixed: bool,
) -> Result<Value> {
    ensure!(crate::apply::valid_instance(name), "invalid instance name");
    let request_path = control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
    let bytes = read_bounded(&request_path, MAX_REQUEST_BYTES);
    let _ = fs::remove_file(&request_path);
    let (request_id, receipt) = match bytes {
        Err(error) => (None, refused(None, None, format!("{error:#}"))),
        Ok(bytes) => match parse_request(&bytes) {
            Err(error) => (None, refused(None, None, format!("{error:#}"))),
            Ok(req) => {
                let request_id = req.request_id.clone();
                let digest = sha256(&bytes);
                let instance_dir = receipts.join(name);
                fs::create_dir_all(&instance_dir)?;
                fs::set_permissions(receipts, fs::Permissions::from_mode(0o700))?;
                fs::set_permissions(&instance_dir, fs::Permissions::from_mode(0o700))?;
                let stored_path = instance_dir.join(format!("{}.json", req.request_id));
                if stored_path.exists() {
                    match read_bounded(&stored_path, MAX_REQUEST_BYTES + MAX_CSR_BYTES + 4096)
                        .and_then(|raw| {
                            serde_json::from_slice::<StoredReceipt>(&raw).map_err(Into::into)
                        }) {
                        Ok(stored) if stored.request_digest == digest => {
                            let mut replay = stored.receipt;
                            replay["replayed"] = json!(true);
                            (Some(request_id), replay)
                        }
                        Ok(_) => (
                            Some(request_id),
                            refused(
                                Some(&req.request_id),
                                Some(req.operation),
                                "request ID was already used for different content".into(),
                            ),
                        ),
                        Err(error) => (
                            Some(request_id),
                            refused(
                                Some(&req.request_id),
                                Some(req.operation),
                                format!("stored receipt is invalid: {error:#}"),
                            ),
                        ),
                    }
                } else {
                    let operation_result = match req.operation {
                        PkiOperation::CreateCsr => create_csr_at(config_dir, &req, enforce_fixed),
                        PkiOperation::ReadCsr => read_csr_at(config_dir, &req, enforce_fixed),
                    };
                    let receipt = match operation_result {
                        Ok(mut data) => {
                            data["receipt_version"] = json!(1);
                            data["request_id"] = json!(req.request_id);
                            data["operation"] = json!(req.operation);
                            data["replayed"] = json!(false);
                            data
                        }
                        Err(error) => refused(
                            Some(&req.request_id),
                            Some(req.operation),
                            format!("{error:#}"),
                        ),
                    };
                    let stored = StoredReceipt {
                        request_digest: digest,
                        receipt: receipt.clone(),
                    };
                    let stored_bytes = serde_json::to_vec(&stored)?;
                    write_atomic(&stored_path, &stored_bytes, 0o600, None)?;
                    (Some(request_id), receipt)
                }
            }
        },
    };
    let _ = request_id;
    write_result(control_dir, name, &receipt)?;
    Ok(receipt)
}

/// Agent-side interface to the separate root PKI helper. The agent can write only bounded
/// requests and read public receipts; it has no access to /etc/baft-tunnel/node.key.
pub trait PkiHelperClient: Send + Sync {
    /// Whether this client serves the Agent's configured PKI directory.
    fn supports_config_dir(&self, _config_dir: &Path) -> bool {
        true
    }
    fn available(&self, name: &str) -> std::result::Result<(), String>;
    fn submit(&self, name: &str, request: &Value) -> std::result::Result<Value, String>;
}

pub struct SystemPkiClient {
    pub control_dir: PathBuf,
    pub unit_dir: PathBuf,
}

impl Default for SystemPkiClient {
    fn default() -> Self {
        Self {
            control_dir: PathBuf::from(crate::apply::CONTROL_DIR),
            unit_dir: PathBuf::from("/etc/systemd/system"),
        }
    }
}

impl PkiHelperClient for SystemPkiClient {
    fn supports_config_dir(&self, config_dir: &Path) -> bool {
        config_dir == Path::new("/etc/baft-tunnel")
    }

    fn available(&self, name: &str) -> std::result::Result<(), String> {
        let service = self.unit_dir.join("baft-tunnel-control-pki@.service");
        let path_unit = self.unit_dir.join("baft-tunnel-control-pki@.path");
        let control_meta = fs::symlink_metadata(&self.control_dir);
        if !control_meta.is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
            || !service.is_file()
            || !path_unit.is_file()
        {
            return Err(format!(
                "the root PKI helper is not installed; run sudo scripts/install-agent-control.sh --instance {name}"
            ));
        }
        let cpath = std::ffi::CString::new(self.control_dir.as_os_str().as_encoded_bytes())
            .map_err(|_| "control path contains a NUL byte".to_string())?;
        // SAFETY: cpath is a valid NUL-terminated path; access(2) only checks permissions.
        if unsafe { libc::access(cpath.as_ptr(), libc::W_OK | libc::X_OK) } != 0 {
            return Err(format!(
                "the agent cannot write PKI requests into {}",
                self.control_dir.display()
            ));
        }
        Ok(())
    }

    fn submit(&self, name: &str, request: &Value) -> std::result::Result<Value, String> {
        if !crate::apply::valid_instance(name) {
            return Err("invalid instance".into());
        }
        self.available(name)?;
        let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_REQUEST_BYTES {
            return Err(format!("PKI request exceeds {MAX_REQUEST_BYTES} bytes"));
        }
        let request_id = request
            .get("request_id")
            .and_then(Value::as_str)
            .filter(|id| crate::apply::valid_key(id))
            .ok_or_else(|| "PKI request has an invalid request_id".to_string())?;
        let request_path = self.control_dir.join(format!("{name}.{REQUEST_SUFFIX}"));
        let result_path = self.control_dir.join(format!("{name}.{RESULT_SUFFIX}"));
        let temp_path = self
            .control_dir
            .join(format!(".{name}.{REQUEST_SUFFIX}.{request_id}.tmp"));
        let _ = fs::remove_file(&result_path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o660)
            .open(&temp_path)
            .map_err(|e| format!("cannot stage the PKI request: {e}"))?;
        if let Err(error) = file
            .set_permissions(fs::Permissions::from_mode(0o660))
            .and_then(|()| file.write_all(&bytes))
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(format!("cannot stage the PKI request: {error}"));
        }
        drop(file);
        // Publish the complete, synced request in one create-if-absent operation. PathExists
        // must never wake the root helper while the Agent is still writing a partial JSON body.
        if let Err(error) = fs::hard_link(&temp_path, &request_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                "another PKI request for this instance is still pending".to_string()
            } else {
                format!("cannot publish the PKI request: {error}")
            });
        }
        let _ = fs::remove_file(&temp_path);
        // The request file itself was synced before publication; directory fsync is best effort
        // on filesystems that do not support syncing directory handles.
        let _ = File::open(&self.control_dir).and_then(|dir| dir.sync_all());

        const WAIT: Duration = Duration::from_secs(35);
        let started = Instant::now();
        while started.elapsed() < WAIT {
            std::thread::sleep(Duration::from_millis(200));
            if fs::symlink_metadata(&result_path).is_err() {
                continue;
            }
            let bytes = read_helper_receipt(&result_path, MAX_CSR_BYTES * 2 + 8192)
                .map_err(|e| format!("cannot read PKI helper receipt: {e:#}"))?;
            let _ = fs::remove_file(&result_path);
            return serde_json::from_slice(&bytes)
                .map_err(|e| format!("the PKI helper receipt is not valid JSON: {e}"));
        }
        let _ = fs::remove_file(&request_path);
        Err(format!(
            "the root PKI helper did not answer within {} s; check baft-tunnel-control-pki@{name}.path",
            WAIT.as_secs()
        ))
    }
}

pub fn request_document(
    request_id: &str,
    operation: PkiOperation,
    node_id: &str,
    server_name: &str,
) -> Value {
    json!({
        "request_version": 1,
        "request_id": request_id,
        "operation": operation,
        "node_id": node_id,
        "server_name": server_name
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU32, Ordering},
    };

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (Temp, PathBuf, PathBuf, PathBuf) {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("baft-pki-helper-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let control = root.join("control");
        let config = root.join("etc");
        let receipts = root.join("receipts");
        fs::create_dir_all(&control).unwrap();
        fs::create_dir_all(&config).unwrap();
        (Temp(root), control, config, receipts)
    }

    fn request(id: &str, operation: PkiOperation, node: &str, server: &str) -> Value {
        json!({
            "request_version": 1,
            "request_id": id,
            "operation": operation,
            "node_id": node,
            "server_name": server
        })
    }

    fn put(control: &Path, req: &Value) {
        fs::write(
            control.join(format!("node1.{REQUEST_SUFFIX}")),
            req.to_string(),
        )
        .unwrap();
    }

    fn test_request(req: &Value) -> PkiRequest {
        serde_json::from_value(req.clone()).unwrap()
    }

    #[test]
    fn system_pki_client_is_bound_to_the_fixed_system_pki_directory() {
        let client = SystemPkiClient::default();
        assert!(client.supports_config_dir(Path::new("/etc/baft-tunnel")));
        assert!(!client.supports_config_dir(Path::new("/tmp/custom-baft")));
    }

    #[test]
    fn system_pki_client_submits_bounded_request_and_reads_root_receipt() {
        let (tmp, control, _config, _receipts) = fixture();
        let units = tmp.0.join("units");
        fs::create_dir_all(&units).unwrap();
        fs::write(units.join("baft-tunnel-control-pki@.service"), b"unit").unwrap();
        fs::write(units.join("baft-tunnel-control-pki@.path"), b"path").unwrap();
        let client = SystemPkiClient {
            control_dir: control.clone(),
            unit_dir: units,
        };
        let worker_control = control.clone();
        let worker = std::thread::spawn(move || {
            let request_path = worker_control.join("node1.pki");
            for _ in 0..100 {
                if request_path.is_file() {
                    let request_meta = fs::metadata(&request_path).unwrap();
                    assert_eq!(request_meta.permissions().mode() & 0o777, 0o660);
                    let request: Value =
                        serde_json::from_slice(&fs::read(&request_path).unwrap()).unwrap();
                    fs::remove_file(&request_path).unwrap();
                    let receipt = json!({
                        "receipt_version": 1,
                        "request_id": request["request_id"],
                        "operation": request["operation"],
                        "result": "retrieved",
                        "private_key_exported": false
                    });
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o640)
                        .open(worker_control.join("node1.pki-result"))
                        .unwrap();
                    file.write_all(&serde_json::to_vec(&receipt).unwrap())
                        .unwrap();
                    file.set_permissions(fs::Permissions::from_mode(0o640))
                        .unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("Agent did not create its bounded PKI request");
        });
        let request = request_document(
            "csr-client-01",
            PkiOperation::ReadCsr,
            "node1",
            "node1.example.test",
        );
        let receipt = client.submit("node1", &request).unwrap();
        worker.join().unwrap();
        assert_eq!(receipt["request_id"], "csr-client-01");
        assert_eq!(receipt["operation"], "read_csr");
        assert!(!control.join("node1.pki").exists());
    }

    #[test]
    fn agent_receipt_reader_rejects_symlinks_and_non_root_permissions() {
        let (tmp, _control, _config, _receipts) = fixture();
        let receipt = tmp.0.join("receipt");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(&receipt)
            .unwrap();
        file.write_all(br#"{"result":"created"}"#).unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o640))
            .unwrap();
        drop(file);
        assert!(read_helper_receipt(&receipt, 1024).is_ok());
        let target = tmp.0.join("receipt-target");
        fs::write(&target, b"{}").unwrap();
        fs::remove_file(&receipt).unwrap();
        std::os::unix::fs::symlink(&target, &receipt).unwrap();
        assert!(read_helper_receipt(&receipt, 1024).is_err());
        fs::remove_file(&receipt).unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&receipt)
            .unwrap();
        file.write_all(b"{}").unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        drop(file);
        assert!(read_helper_receipt(&receipt, 1024).is_err());
    }

    #[test]
    fn creates_p256_key_and_public_csr_without_exporting_private_key() {
        let (tmp, control, config, receipts) = fixture();
        // Production is fixed to /etc/baft-tunnel; tests exercise the internal primitive in a temp dir.
        let req = test_request(&request(
            "csr-create-1",
            PkiOperation::CreateCsr,
            "node1",
            "example.test",
        ));
        let out = create_csr_at(&config, &req, false).unwrap();
        assert_eq!(out["result"], "created");
        assert_eq!(out["node_id"], "node1");
        assert_eq!(out["expected_server_san"], "DNS:example.test");
        assert_eq!(out["private_key_exported"], false);
        assert!(
            out["csr_pem"]
                .as_str()
                .unwrap()
                .contains("BEGIN CERTIFICATE REQUEST")
        );
        assert!(config.join("node.key").is_file());
        assert!(config.join("node.csr").is_file());
        assert_eq!(
            fs::metadata(config.join("node.key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = (tmp, control, receipts);
    }

    #[test]
    fn refuses_overwrite_wrong_identity_bad_dns_and_symlinks() {
        let (_tmp, _control, config, _receipts) = fixture();
        let req = test_request(&request(
            "csr-create-2",
            PkiOperation::CreateCsr,
            "node1",
            "example.test",
        ));
        create_csr_at(&config, &req, false).unwrap();
        assert!(create_csr_at(&config, &req, false).is_err());
        assert!(expected_server_san("*.example.test").is_err());
        assert!(expected_server_san("-bad.example.test").is_err());
        assert!(!valid_node_id("../bad"));
    }

    #[test]
    fn reads_csr_only_and_checks_node_identity() {
        let (_tmp, _control, config, _receipts) = fixture();
        let create = test_request(&request(
            "csr-create-3",
            PkiOperation::CreateCsr,
            "node1",
            "127.0.0.1",
        ));
        create_csr_at(&config, &create, false).unwrap();
        let read = test_request(&request(
            "csr-read-1",
            PkiOperation::ReadCsr,
            "node1",
            "127.0.0.1",
        ));
        let out = read_csr_at(&config, &read, false).unwrap();
        assert_eq!(out["result"], "retrieved");
        assert_eq!(out["expected_server_san"], "IP:127.0.0.1");
        let metadata_path = config.join("node.csr.meta");
        let metadata_backup = config.join("node.csr.meta.backup");
        fs::rename(&metadata_path, &metadata_backup).unwrap();
        std::os::unix::fs::symlink(&metadata_backup, &metadata_path).unwrap();
        assert!(read_csr_at(&config, &read, false).is_err());
        fs::remove_file(&metadata_path).unwrap();
        fs::rename(&metadata_backup, &metadata_path).unwrap();
        let wrong = test_request(&request(
            "csr-read-2",
            PkiOperation::ReadCsr,
            "other",
            "127.0.0.1",
        ));
        assert!(read_csr_at(&config, &wrong, false).is_err());
        let wrong_server = test_request(&request(
            "csr-read-3",
            PkiOperation::ReadCsr,
            "node1",
            "different.example.test",
        ));
        assert!(read_csr_at(&config, &wrong_server, false).is_err());
        let metadata_path = config.join("node.csr.meta");
        let mut metadata: Value =
            serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
        metadata["public_key_sha256"] = json!("00".repeat(32));
        fs::write(&metadata_path, metadata.to_string()).unwrap();
        let matching = test_request(&request(
            "csr-read-4",
            PkiOperation::ReadCsr,
            "node1",
            "127.0.0.1",
        ));
        assert!(read_csr_at(&config, &matching, false).is_err());
    }

    #[test]
    fn refuses_symlinked_and_oversized_requests_without_touching_targets() {
        let (tmp, control, config, receipts) = fixture();
        let target = tmp.0.join("request-target");
        fs::write(
            &target,
            request(
                "csr-create-5",
                PkiOperation::CreateCsr,
                "node1",
                "example.test",
            )
            .to_string(),
        )
        .unwrap();
        std::os::unix::fs::symlink(&target, control.join("node1.pki")).unwrap();
        let refused_link = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(refused_link["result"], "refused");
        assert!(target.exists(), "symlink target must remain untouched");

        fs::write(
            control.join("node1.pki"),
            vec![b' '; MAX_REQUEST_BYTES as usize + 1],
        )
        .unwrap();
        let refused_large = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(refused_large["result"], "refused");
        assert!(!config.join("node.key").exists());
        assert!(!config.join("node.csr").exists());
    }

    #[test]
    fn valid_operation_refusal_keeps_request_id_and_operation_for_agent_correlation() {
        let (_tmp, control, config, receipts) = fixture();
        fs::write(config.join("node.key"), b"pre-existing key").unwrap();
        let req = request(
            "csr-refuse-01",
            PkiOperation::CreateCsr,
            "node1",
            "example.test",
        );
        put(&control, &req);
        let receipt = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(receipt["result"], "refused");
        assert_eq!(receipt["request_id"], "csr-refuse-01");
        assert_eq!(receipt["operation"], "create_csr");
        assert_eq!(receipt["private_key_exported"], false);
        assert!(
            receipt["reason"]
                .as_str()
                .unwrap()
                .contains("already exists")
        );
    }

    #[test]
    fn create_receipt_contains_only_public_csr_and_never_private_key_bytes() {
        let (_tmp, control, config, receipts) = fixture();
        let req = request(
            "csr-create-6",
            PkiOperation::CreateCsr,
            "node1",
            "example.test",
        );
        put(&control, &req);
        let receipt = run_at("node1", &control, &config, &receipts, false).unwrap();
        let encoded = receipt.to_string();
        assert!(encoded.contains("BEGIN CERTIFICATE REQUEST"));
        assert!(!encoded.contains("PRIVATE KEY"));
        assert_eq!(receipt["private_key_exported"], false);
        assert!(config.join("node.key").is_file());
        assert!(config.join("node.csr").is_file());
    }

    #[test]
    fn helper_protocol_is_bounded_consumed_replay_safe_and_separate_from_setup() {
        let (_tmp, control, config, receipts) = fixture();
        let req = request(
            "csr-create-4",
            PkiOperation::CreateCsr,
            "node1",
            "example.test",
        );
        put(&control, &req);
        fs::write(control.join("node1.setup"), b"unrelated setup request").unwrap();
        // Test entry point uses an injected fixed directory while preserving the protocol.
        let out = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(out["result"], "created");
        assert!(!control.join("node1.pki").exists());
        assert!(control.join("node1.pki-result").is_file());
        assert_eq!(
            fs::read(control.join("node1.setup")).unwrap(),
            b"unrelated setup request"
        );
        put(&control, &req);
        let replay = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(replay["replayed"], true);
        let altered = request(
            "csr-create-4",
            PkiOperation::ReadCsr,
            "node1",
            "example.test",
        );
        put(&control, &altered);
        assert_eq!(
            run_at("node1", &control, &config, &receipts, false).unwrap()["result"],
            "refused"
        );
    }
}
