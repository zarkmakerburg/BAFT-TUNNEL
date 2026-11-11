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
use x509_parser::{
    oid_registry::{OID_EC_P256, OID_KEY_TYPE_EC_PUBLIC_KEY},
    prelude::FromDer,
    x509::SubjectPublicKeyInfo,
};

pub const REQUEST_SUFFIX: &str = "pki";
pub const RESULT_SUFFIX: &str = "pki-result";
pub const RECEIPTS_DIR: &str = "/var/lib/baft-tunnel-control/pki-receipts";
pub const MAX_REQUEST_BYTES: u64 = 512 * 1024;
pub const MAX_CSR_BYTES: u64 = 16 * 1024;
pub const MAX_CERTIFICATE_BYTES: usize = 64 * 1024;
pub const MAX_CA_BYTES: usize = 64 * 1024;
pub const MAX_CRL_BYTES: usize = 256 * 1024;
const OPENSSL: &str = "/usr/bin/openssl";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PkiOperation {
    CreateCsr,
    ReadCsr,
    InstallCertificate,
}

impl PkiOperation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::CreateCsr => "create_csr",
            Self::ReadCsr => "read_csr",
            Self::InstallCertificate => "install_certificate",
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    certificate_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    crl_pem: Option<String>,
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

fn ensure_private_receipt_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "receipt path {} is not a real directory",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)
            .with_context(|| format!("create receipt directory {}", path.display()))?,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect receipt directory {}", path.display()));
        }
    }
    let meta = fs::symlink_metadata(path)?;
    // The fixed production path is below /var/lib, whose parent is root-owned and not writable
    // by the service account. Requiring the helper's effective UID here prevents adoption of a
    // pre-planted directory owned by the unprivileged Agent.
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "receipt path {} changed type",
        path.display()
    );
    ensure!(
        meta.uid() == unsafe { libc::geteuid() },
        "receipt directory {} has an unexpected owner",
        path.display()
    );
    ensure!(
        meta.mode() & 0o022 == 0,
        "receipt directory {} is writable by group or others",
        path.display()
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("secure receipt directory {}", path.display()))?;
    Ok(())
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
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "OpenSSL operation failed: {} {}",
            stderr.trim(),
            stdout.trim()
        );
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

pub(crate) fn is_p256_subject_public_key_info(der: &[u8]) -> bool {
    let Ok((remaining, spki)) = SubjectPublicKeyInfo::from_der(der) else {
        return false;
    };
    if !remaining.is_empty() || spki.algorithm.algorithm != OID_KEY_TYPE_EC_PUBLIC_KEY {
        return false;
    }
    let Some(parameters) = spki.algorithm.parameters.as_ref() else {
        return false;
    };
    // OBJECT IDENTIFIER is universal tag 6. `Any::as_oid` alone is permissive and
    // can reinterpret an explicit-parameter SEQUENCE as an OID; check the tag first.
    if parameters.header.tag().0 != 6
        || parameters.class() != x509_parser::der_parser::asn1_rs::Class::Universal
    {
        return false;
    }
    parameters.as_oid().is_ok_and(|curve| curve == OID_EC_P256) && spki.parsed().is_ok()
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

fn validate_pki_directory_metadata(path: &Path, meta: &fs::Metadata, fixed: bool) -> Result<()> {
    ensure!(meta.is_dir(), "{} is not a directory", path.display());
    ensure!(
        meta.permissions().mode() & 0o022 == 0,
        "{} must not be group/world writable",
        path.display()
    );
    if fixed {
        let (_, service_gid) = crate::setup::service_ids()
            .context("baft-tunnel service group is required for fixed PKI ownership")?;
        ensure!(
            meta.uid() == 0 && meta.gid() == service_gid,
            "{} must be owned by root:baft-tunnel",
            path.display()
        );
        ensure!(
            meta.permissions().mode() & 0o7777 == 0o750,
            "{} must have mode 0750",
            path.display()
        );
    }
    Ok(())
}

fn validate_pki_file_metadata(path: &Path, meta: &fs::Metadata, fixed: bool) -> Result<()> {
    ensure!(meta.is_file(), "{} is not a regular file", path.display());
    ensure!(
        meta.permissions().mode() & 0o022 == 0,
        "{} must not be group/world writable",
        path.display()
    );
    if fixed {
        let (_, service_gid) = crate::setup::service_ids()
            .context("baft-tunnel service group is required for fixed PKI ownership")?;
        ensure!(
            meta.uid() == 0 && meta.gid() == service_gid,
            "{} must be owned by root:baft-tunnel",
            path.display()
        );
        ensure!(
            meta.permissions().mode() & 0o7777 == 0o644,
            "{} must have mode 0644",
            path.display()
        );
    }
    Ok(())
}

/// Resolve the active immutable credential set through the single atomic pki/current pointer.
/// Legacy installations without a pki/ directory continue to use the original flat filenames.
pub(crate) fn active_set_dir(config_dir: &Path) -> Result<Option<PathBuf>> {
    let root = config_dir.join("pki");
    let root_meta = match fs::symlink_metadata(&root) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect PKI version directory"),
    };
    ensure!(
        root_meta.is_dir() && !root_meta.file_type().is_symlink(),
        "PKI version root must be a real directory"
    );
    validate_pki_directory_metadata(
        &root,
        &root_meta,
        config_dir == Path::new("/etc/baft-tunnel"),
    )?;
    let current = root.join("current");
    let current_meta = match fs::symlink_metadata(&current) {
        Ok(meta) => Some(meta),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("inspect active PKI version pointer"),
    };
    let sets = root.join("sets");
    let sets_meta = match fs::symlink_metadata(&sets) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && current_meta.is_none() => {
            // install.sh creates the fixed pki root for a new node. Until the first
            // certificate import, that root legitimately has neither sets/ nor current.
            return Ok(None);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("active PKI version pointer exists without its sets directory")
        }
        Err(error) => return Err(error).context("inspect PKI version sets directory"),
    };
    ensure!(
        sets_meta.is_dir() && !sets_meta.file_type().is_symlink(),
        "PKI version sets must be a real directory"
    );
    validate_pki_directory_metadata(
        &sets,
        &sets_meta,
        config_dir == Path::new("/etc/baft-tunnel"),
    )?;
    let current_meta = match current_meta {
        Some(meta) => meta,
        None => return Ok(None),
    };
    ensure!(
        current_meta.file_type().is_symlink(),
        "active PKI version pointer must be a symlink"
    );
    if config_dir == Path::new("/etc/baft-tunnel") {
        ensure!(
            current_meta.uid() == 0,
            "active PKI version pointer must be owned by root"
        );
    }
    let target = fs::read_link(&current).context("read active PKI version pointer")?;
    let components: Vec<_> = target.components().collect();
    ensure!(
        components.len() == 2
            && components[0].as_os_str() == "sets"
            && components[1]
                .as_os_str()
                .to_str()
                .is_some_and(|id| id.len() == 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
        "active PKI version pointer must target sets/<lowercase-sha256>"
    );
    let active = root.join(&target);
    let active_meta = fs::symlink_metadata(&active).context("inspect active PKI version set")?;
    ensure!(
        active_meta.is_dir() && !active_meta.file_type().is_symlink(),
        "active PKI version must be a real directory"
    );
    validate_pki_directory_metadata(
        &active,
        &active_meta,
        config_dir == Path::new("/etc/baft-tunnel"),
    )?;
    let canonical_sets = sets.canonicalize()?;
    let canonical_active = active.canonicalize()?;
    ensure!(
        canonical_active.parent() == Some(canonical_sets.as_path()),
        "active PKI version escapes the fixed sets directory"
    );
    for name in ["node.crt", "ca.crt", "ca.crl"] {
        let path = active.join(name);
        let meta = fs::symlink_metadata(&path)
            .with_context(|| format!("inspect active PKI material {}", path.display()))?;
        ensure!(
            !meta.file_type().is_symlink(),
            "{} must not be a symlink",
            path.display()
        );
        validate_pki_file_metadata(&path, &meta, config_dir == Path::new("/etc/baft-tunnel"))?;
    }
    let cert = read_regular(
        &active.join("node.crt"),
        MAX_CERTIFICATE_BYTES as u64,
        false,
    )?;
    let ca = read_regular(&active.join("ca.crt"), MAX_CA_BYTES as u64, false)?;
    let crl = read_regular(&active.join("ca.crl"), MAX_CRL_BYTES as u64, false)?;
    ensure!(
        version_set_id(&cert, &ca, &crl) == components[1].as_os_str().to_str().unwrap_or_default(),
        "active PKI set digest does not match its certificate material"
    );
    Ok(Some(active))
}

pub(crate) fn version_set_id(cert: &[u8], ca: &[u8], crl: &[u8]) -> String {
    let mut content = Vec::with_capacity(cert.len() + ca.len() + crl.len() + 3);
    content.extend_from_slice(cert);
    content.push(0);
    content.extend_from_slice(ca);
    content.push(0);
    content.extend_from_slice(crl);
    sha256(&content)
}

fn strict_pem_block_count(bytes: &[u8], label: &str, begin: &str, end: &str) -> Result<()> {
    let text = std::str::from_utf8(bytes).with_context(|| format!("{label} is not UTF-8 PEM"))?;
    let mut rest = text.trim();
    let mut count = 0usize;
    while !rest.is_empty() {
        ensure!(
            rest.starts_with(begin),
            "{label} contains data outside PEM blocks"
        );
        let end_at = rest
            .find(end)
            .context(format!("{label} has an unterminated PEM block"))?;
        let after = end_at + end.len();
        count += 1;
        rest = rest[after..].trim();
    }
    ensure!(count == 1, "{label} must contain exactly one PEM block");
    Ok(())
}

fn crl_number(path: &Path) -> Result<u64> {
    let args = [
        "crl",
        "-in",
        path.to_str().context("non-UTF8 CRL path")?,
        "-noout",
        "-crlnumber",
    ];
    let output = run_openssl(&openssl_args(&args))?;
    let value = String::from_utf8(output.stdout)?;
    let number = value
        .trim()
        .strip_prefix("crlNumber=")
        .context("CRL Number extension is missing or malformed")?;
    if let Some(hex) = number.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).context("CRL Number is outside supported range")
    } else {
        number
            .parse::<u64>()
            .context("CRL Number is outside supported range")
    }
}

fn validate_leaf_policy(
    cert_der: &[u8],
    node_id: &str,
    expected_server_san: &str,
) -> Result<(String, String, i64)> {
    use x509_parser::extensions::GeneralName;
    let (remaining, cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|error| anyhow::anyhow!("parse leaf X.509 certificate: {error}"))?;
    ensure!(
        remaining.is_empty(),
        "leaf certificate contains trailing DER data"
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    ensure!(
        cert.validity().not_before.timestamp() <= now
            && cert.validity().not_after.timestamp() > now,
        "leaf certificate is not currently valid"
    );
    let common_names: Vec<_> = cert.subject().iter_common_name().collect();
    ensure!(
        common_names.len() == 1 && common_names[0].as_str().is_ok_and(|name| name == node_id),
        "leaf certificate CN must exactly match the configured node ID"
    );
    let basic = cert
        .basic_constraints()?
        .context("leaf certificate lacks basicConstraints")?;
    ensure!(!basic.value.ca, "leaf certificate must not be a CA");
    let usage = cert
        .key_usage()?
        .context("leaf certificate lacks keyUsage")?;
    ensure!(
        usage.value.flags == 1,
        "leaf certificate keyUsage must be exactly digitalSignature"
    );
    let eku = cert
        .extended_key_usage()?
        .context("leaf certificate lacks extendedKeyUsage")?;
    ensure!(
        eku.value.server_auth
            && eku.value.client_auth
            && !eku.value.any
            && !eku.value.code_signing
            && !eku.value.email_protection
            && !eku.value.time_stamping
            && !eku.value.ocsp_signing
            && eku.value.other.is_empty(),
        "leaf certificate EKU must contain exactly serverAuth and clientAuth"
    );
    let san = cert
        .subject_alternative_name()?
        .context("leaf certificate lacks subjectAltName")?;
    let expected_uri = format!("urn:baft-tunnel:node:{node_id}");
    let mut uri_count = 0usize;
    let mut server_count = 0usize;
    for name in &san.value.general_names {
        match name {
            GeneralName::URI(uri) if *uri == expected_uri => uri_count += 1,
            GeneralName::DNSName(dns) if expected_server_san.strip_prefix("DNS:") == Some(*dns) => {
                server_count += 1;
            }
            GeneralName::IPAddress(bytes) => {
                let expected_ip = expected_server_san.strip_prefix("IP:");
                let actual = match bytes.len() {
                    4 => Some(std::net::IpAddr::from([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ])),
                    16 => {
                        let mut octets = [0u8; 16];
                        octets.copy_from_slice(bytes);
                        Some(std::net::IpAddr::from(octets))
                    }
                    _ => None,
                };
                if expected_ip
                    .is_some_and(|ip| actual.is_some_and(|actual| ip == actual.to_string()))
                {
                    server_count += 1;
                } else {
                    bail!("leaf certificate contains an unexpected IP SAN");
                }
            }
            _ => bail!("leaf certificate contains an unexpected SAN entry"),
        }
    }
    ensure!(
        uri_count == 1,
        "leaf certificate must contain exactly one matching BAFT node URI SAN"
    );
    ensure!(
        server_count == 1,
        "leaf certificate must contain exactly one configured DNS/IP SAN"
    );
    let pin = sha256(cert_der);
    let public_key_fingerprint = sha256(cert.public_key().raw);
    Ok((
        pin,
        public_key_fingerprint,
        cert.validity().not_after.timestamp(),
    ))
}

fn install_certificate_at(
    config_dir: &Path,
    req: &PkiRequest,
    enforce_fixed: bool,
) -> Result<Value> {
    ensure_directory(config_dir, enforce_fixed)?;
    let lock_path = config_dir.join(".pki-install.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&lock_path)
        .context("open PKI installation lock without following symlinks")?;
    let lock_meta = lock.metadata()?;
    ensure!(
        lock_meta.is_file(),
        "PKI installation lock is not a regular file"
    );
    ensure!(
        lock_meta.permissions().mode() & 0o7777 == 0o600,
        "PKI installation lock must have mode 0600"
    );
    if config_dir == Path::new("/etc/baft-tunnel") {
        ensure!(
            lock_meta.uid() == 0 && lock_meta.gid() == 0,
            "PKI installation lock must be owned by root:root"
        );
    }
    // SAFETY: flock only operates on the valid, open lock descriptor.
    ensure!(
        unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } == 0,
        "cannot acquire PKI installation lock"
    );
    let key = config_dir.join("node.key");
    let metadata_path = config_dir.join("node.csr.meta");
    let _ = read_csr_at(config_dir, req, enforce_fixed).context(
        "validate the original CSR and local private-key custody before certificate import",
    )?;
    let metadata_bytes = read_public_file(&metadata_path, 4096)?;
    let metadata: CsrMetadata =
        serde_json::from_slice(&metadata_bytes).context("stored CSR metadata is invalid")?;
    ensure!(
        metadata.metadata_version == 1,
        "unsupported CSR metadata version"
    );
    ensure!(
        metadata.node_id == req.node_id,
        "certificate node ID does not match CSR metadata"
    );
    let expected_san = expected_server_san(&req.server_name)?;
    ensure!(
        metadata.expected_server_san == expected_san,
        "certificate DNS/IP identity does not match the original CSR enrollment"
    );

    let cert_bytes = req
        .certificate_pem
        .as_deref()
        .context("certificate PEM is missing")?
        .as_bytes();
    let ca_bytes = req
        .ca_pem
        .as_deref()
        .context("CA PEM is missing")?
        .as_bytes();
    let crl_bytes = req
        .crl_pem
        .as_deref()
        .context("CRL PEM is missing")?
        .as_bytes();
    ensure!(
        cert_bytes.len() <= MAX_CERTIFICATE_BYTES,
        "leaf certificate exceeds size limit"
    );
    ensure!(
        ca_bytes.len() <= MAX_CA_BYTES,
        "CA certificate exceeds size limit"
    );
    ensure!(crl_bytes.len() <= MAX_CRL_BYTES, "CRL exceeds size limit");
    strict_pem_block_count(
        cert_bytes,
        "leaf certificate",
        "-----BEGIN CERTIFICATE-----",
        "-----END CERTIFICATE-----",
    )?;
    strict_pem_block_count(
        ca_bytes,
        "CA certificate",
        "-----BEGIN CERTIFICATE-----",
        "-----END CERTIFICATE-----",
    )?;
    strict_pem_block_count(
        crl_bytes,
        "CRL",
        "-----BEGIN X509 CRL-----",
        "-----END X509 CRL-----",
    )?;

    let pki_root = config_dir.join("pki");
    if !pki_root.exists() {
        fs::create_dir(&pki_root).context("create fixed PKI version root")?;
        fs::set_permissions(&pki_root, fs::Permissions::from_mode(0o750))?;
        if let Some((_, gid)) = crate::setup::service_ids() {
            chown_file(&pki_root, 0, gid)?;
        }
    }
    let root_meta = fs::symlink_metadata(&pki_root)?;
    ensure!(
        root_meta.is_dir() && !root_meta.file_type().is_symlink(),
        "PKI version root must be a real directory"
    );
    let sets = pki_root.join("sets");
    if !sets.exists() {
        fs::create_dir(&sets).context("create PKI version sets directory")?;
        fs::set_permissions(&sets, fs::Permissions::from_mode(0o750))?;
        if let Some((_, gid)) = crate::setup::service_ids() {
            chown_file(&sets, 0, gid)?;
        }
    }
    let sets_meta = fs::symlink_metadata(&sets)?;
    ensure!(
        sets_meta.is_dir() && !sets_meta.file_type().is_symlink(),
        "PKI version sets must be a real directory"
    );

    let scratch = config_dir.join(format!(".pki-import-{}", req.request_id));
    fs::create_dir(&scratch).context("create private certificate validation directory")?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
    let result = (|| -> Result<Value> {
        let cert_path = scratch.join("node.crt");
        let ca_path = scratch.join("ca.crt");
        let crl_path = scratch.join("ca.crl");
        fs::write(&cert_path, cert_bytes)?;
        fs::write(&ca_path, ca_bytes)?;
        fs::write(&crl_path, crl_bytes)?;
        for path in [&cert_path, &ca_path, &crl_path] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            File::open(path)?.sync_all()?;
        }

        // The CSR key and returned leaf must have identical P-256 SubjectPublicKeyInfo.
        let key_fingerprint = public_key_fingerprint_from_key(&key)?;
        ensure!(
            key_fingerprint == metadata.public_key_sha256,
            "local private key no longer matches the original CSR metadata"
        );
        let der_path = scratch.join("node.der");
        let der_args = [
            "x509",
            "-in",
            cert_path.to_str().context("non-UTF8 certificate path")?,
            "-outform",
            "DER",
            "-out",
            der_path.to_str().context("non-UTF8 certificate DER path")?,
        ];
        run_openssl(&openssl_args(&der_args))?;
        let cert_der = read_regular(&der_path, MAX_CERTIFICATE_BYTES as u64, false)?;
        let (fingerprint, cert_key_fingerprint, not_after) =
            validate_leaf_policy(&cert_der, &req.node_id, &expected_san)?;
        ensure!(
            cert_key_fingerprint == key_fingerprint,
            "leaf certificate public key does not match the local private key"
        );

        // Reuse the tunnel WebPKI validators for both serverAuth and clientAuth. This
        // validates the trust chain, current validity, CRL signature and revocation status.
        crate::tls::verify_local_certificate(&ca_path, &crl_path, &cert_path, &expected_san)
            .context(
                "certificate chain, EKU, CRL signature, validity or revocation check failed",
            )?;
        let crl_remaining = crate::tls::crl_seconds_remaining(&crl_path)?;
        ensure!(
            crl_remaining >= 24 * 3600,
            "CRL has under 24 hours remaining or is expired"
        );

        let ca_der_path = scratch.join("ca.der");
        let ca_der_args = [
            "x509",
            "-in",
            ca_path.to_str().context("non-UTF8 CA path")?,
            "-outform",
            "DER",
            "-out",
            ca_der_path.to_str().context("non-UTF8 CA DER path")?,
        ];
        run_openssl(&openssl_args(&ca_der_args))?;
        let ca_der = read_regular(&ca_der_path, MAX_CA_BYTES as u64, false)?;
        let (remaining, ca_cert) = x509_parser::parse_x509_certificate(&ca_der)
            .map_err(|error| anyhow::anyhow!("parse CA certificate: {error}"))?;
        ensure!(
            remaining.is_empty(),
            "uploaded CA certificate contains trailing DER data"
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        ensure!(
            ca_cert.validity().not_before.timestamp() <= now
                && ca_cert.validity().not_after.timestamp() > now,
            "uploaded CA certificate is not currently valid"
        );
        let ca_basic = ca_cert
            .basic_constraints()?
            .context("CA certificate lacks basicConstraints")?;
        ensure!(ca_basic.value.ca, "uploaded CA certificate is not a CA");
        let ca_usage = ca_cert
            .key_usage()?
            .context("CA certificate lacks keyUsage")?;
        ensure!(
            ca_usage.value.key_cert_sign() && ca_usage.value.crl_sign(),
            "CA keyUsage must include keyCertSign and cRLSign"
        );

        let set_id = version_set_id(cert_bytes, ca_bytes, crl_bytes);
        let old_set = active_set_dir(config_dir)?;
        let already_active = old_set
            .as_ref()
            .is_some_and(|path| path.file_name().is_some_and(|name| name == set_id.as_str()));
        if let Some(old_set) = old_set.as_ref().filter(|_| !already_active) {
            let old_ca = scratch.join("old-ca.crt");
            fs::copy(old_set.join("ca.crt"), &old_ca)?;
            let old_ca_fp = run_openssl(&openssl_args(&[
                "x509",
                "-in",
                old_ca.to_str().context("non-UTF8 old CA path")?,
                "-noout",
                "-fingerprint",
                "-sha256",
            ]))?;
            let new_ca_fp = run_openssl(&openssl_args(&[
                "x509",
                "-in",
                ca_path.to_str().context("non-UTF8 CA path")?,
                "-noout",
                "-fingerprint",
                "-sha256",
            ]))?;
            ensure!(
                old_ca_fp.stdout == new_ca_fp.stdout,
                "automatic CA replacement/rollover is not supported by this enrollment operation"
            );
            let old_crl = scratch.join("old-ca.crl");
            fs::copy(old_set.join("ca.crl"), &old_crl)?;
            ensure!(
                crl_number(&crl_path)? > crl_number(&old_crl)?,
                "candidate CRL number must be greater than the currently active CRL number"
            );
        }

        let final_set = sets.join(&set_id);
        if final_set.exists() {
            let final_meta = fs::symlink_metadata(&final_set)?;
            ensure!(
                final_meta.is_dir() && !final_meta.file_type().is_symlink(),
                "existing PKI set path is not a real directory"
            );
            validate_pki_directory_metadata(
                &final_set,
                &final_meta,
                config_dir == Path::new("/etc/baft-tunnel"),
            )?;
            for name in ["node.crt", "ca.crt", "ca.crl"] {
                let path = final_set.join(name);
                let meta = fs::symlink_metadata(&path)
                    .with_context(|| format!("inspect existing PKI material {}", path.display()))?;
                ensure!(
                    !meta.file_type().is_symlink(),
                    "{} must not be a symlink",
                    path.display()
                );
                validate_pki_file_metadata(
                    &path,
                    &meta,
                    config_dir == Path::new("/etc/baft-tunnel"),
                )?;
            }
            ensure!(
                fs::read(final_set.join("node.crt"))? == cert_bytes
                    && fs::read(final_set.join("ca.crt"))? == ca_bytes
                    && fs::read(final_set.join("ca.crl"))? == crl_bytes,
                "existing content-addressed PKI set does not match its digest"
            );
        } else {
            let staging = sets.join(format!(".staging-{}", req.request_id));
            fs::create_dir(&staging).context("create versioned PKI staging directory")?;
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o750))?;
            let stage_result = (|| -> Result<()> {
                for (name, bytes) in [
                    ("node.crt", cert_bytes),
                    ("ca.crt", ca_bytes),
                    ("ca.crl", crl_bytes),
                ] {
                    let path = staging.join(name);
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o644)
                        .open(&path)?;
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    drop(file);
                    if let Some((_, gid)) = crate::setup::service_ids() {
                        chown_file(&path, 0, gid)?;
                    }
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
                }
                if let Some((_, gid)) = crate::setup::service_ids() {
                    chown_file(&staging, 0, gid)?;
                }
                File::open(&staging)?.sync_all()?;
                fs::rename(&staging, &final_set)?;
                File::open(&sets)?.sync_all()?;
                Ok(())
            })();
            if stage_result.is_err() {
                let _ = fs::remove_dir_all(&staging);
            }
            stage_result?;
        }
        // Recheck the published content-addressed directory and each public file before the
        // active pointer can refer to them. This also covers interrupted earlier staging runs.
        let final_meta =
            fs::symlink_metadata(&final_set).context("inspect completed PKI certificate set")?;
        ensure!(
            final_meta.is_dir() && !final_meta.file_type().is_symlink(),
            "completed PKI certificate set must be a real directory"
        );
        validate_pki_directory_metadata(
            &final_set,
            &final_meta,
            config_dir == Path::new("/etc/baft-tunnel"),
        )?;
        for name in ["node.crt", "ca.crt", "ca.crl"] {
            let path = final_set.join(name);
            let meta = fs::symlink_metadata(&path)
                .with_context(|| format!("inspect completed PKI material {}", path.display()))?;
            ensure!(
                !meta.file_type().is_symlink(),
                "{} must not be a symlink",
                path.display()
            );
            validate_pki_file_metadata(&path, &meta, config_dir == Path::new("/etc/baft-tunnel"))?;
        }

        // One symlink rename switches all three public files together. The previous active
        // pointer is retained and restored if the directory sync after publication fails.
        if already_active {
            return Ok(json!({
                "result": "installed",
                "node_id": req.node_id,
                "certificate_sha256": fingerprint,
                "certificate_not_after": not_after,
                "ca_count": 1,
                "crl_seconds_remaining": crl_remaining,
                "active_set": set_id,
                "private_key_exported": false
            }));
        }
        let current = pki_root.join("current");
        let previous = if fs::symlink_metadata(&current).is_ok() {
            Some(fs::read_link(&current).context("read previous active PKI pointer")?)
        } else {
            None
        };
        let temporary = pki_root.join(format!(".current-{}", req.request_id));
        let _ = fs::remove_file(&temporary);
        std::os::unix::fs::symlink(format!("sets/{set_id}"), &temporary)?;
        fs::rename(&temporary, &current)
            .context("atomically publish active PKI version pointer")?;
        if let Err(error) = File::open(&pki_root).and_then(|dir| dir.sync_all()) {
            if let Some(old) = previous {
                let rollback = pki_root.join(format!(".rollback-{}", req.request_id));
                let _ = fs::remove_file(&rollback);
                if std::os::unix::fs::symlink(old, &rollback).is_ok() {
                    let _ = fs::rename(&rollback, &current);
                }
            } else {
                let _ = fs::remove_file(&current);
            }
            let _ = File::open(&pki_root).and_then(|dir| dir.sync_all());
            return Err(error).context("sync active PKI pointer; previous active set restored");
        }
        Ok(json!({
            "result": "installed",
            "node_id": req.node_id,
            "certificate_sha256": fingerprint,
            "certificate_not_after": not_after,
            "ca_count": 1,
            "crl_seconds_remaining": crl_remaining,
            "active_set": set_id,
            "private_key_exported": false
        }))
    })();
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn validate_fixed_directory_metadata(
    uid: u32,
    gid: u32,
    mode: u32,
    service_gid: u32,
) -> Result<()> {
    ensure!(uid == 0, "fixed PKI directory must be owned by root");
    ensure!(
        gid == service_gid,
        "fixed PKI directory must belong to the baft-tunnel service group"
    );
    ensure!(
        mode & 0o777 == 0o750,
        "fixed PKI directory must have mode 0750"
    );
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
        let (_, service_gid) =
            crate::setup::service_ids().context("baft-tunnel service account is unavailable")?;
        validate_fixed_directory_metadata(meta.uid(), meta.gid(), meta.mode(), service_gid)?;
    }
    Ok(())
}

fn create_csr_at(config_dir: &Path, req: &PkiRequest, enforce_fixed: bool) -> Result<Value> {
    ensure_directory(config_dir, enforce_fixed)?;
    ensure!(
        active_set_dir(config_dir)?.is_none(),
        "an active certificate set already exists; CSR generation never replaces credentials"
    );
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
        // SEC1 PEM is intentionally used here: rustls-ring accepts EC SEC1 keys directly,
        // while OpenSSL's default PKCS#8 output omits the public-key field ring needs.
        let gen_args = [
            "ecparam",
            "-name",
            "prime256v1",
            "-genkey",
            "-noout",
            "-param_enc",
            "named_curve",
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
    let file_name = path.file_name().context("file path has no name")?;
    let tmp = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
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
    match req.operation {
        PkiOperation::InstallCertificate => {
            let cert = req
                .certificate_pem
                .as_deref()
                .context("certificate PEM is missing")?;
            let ca = req.ca_pem.as_deref().context("CA PEM is missing")?;
            let crl = req.crl_pem.as_deref().context("CRL PEM is missing")?;
            ensure!(
                !cert.is_empty() && cert.len() <= MAX_CERTIFICATE_BYTES,
                "certificate PEM is empty or oversized"
            );
            ensure!(
                !ca.is_empty() && ca.len() <= MAX_CA_BYTES,
                "CA PEM is empty or oversized"
            );
            ensure!(
                !crl.is_empty() && crl.len() <= MAX_CRL_BYTES,
                "CRL PEM is empty or oversized"
            );
        }
        _ => ensure!(
            req.certificate_pem.is_none() && req.ca_pem.is_none() && req.crl_pem.is_none(),
            "certificate material is not accepted for CSR operations"
        ),
    }
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
    let (_, receipt) = match bytes {
        Err(error) => (None, refused(None, None, format!("{error:#}"))),
        Ok(bytes) => match parse_request(&bytes) {
            Err(error) => (None, refused(None, None, format!("{error:#}"))),
            Ok(req) => {
                let request_id = req.request_id.clone();
                let digest = sha256(&bytes);
                let instance_dir = receipts.join(name);
                let receipt_dirs = ensure_private_receipt_dir(receipts)
                    .and_then(|()| ensure_private_receipt_dir(&instance_dir));
                if let Err(error) = receipt_dirs {
                    (
                        Some(request_id),
                        refused(
                            Some(&req.request_id),
                            Some(req.operation),
                            format!("cannot prepare the root-owned receipt directory: {error:#}"),
                        ),
                    )
                } else {
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
                            PkiOperation::CreateCsr => {
                                create_csr_at(config_dir, &req, enforce_fixed)
                            }
                            PkiOperation::ReadCsr => read_csr_at(config_dir, &req, enforce_fixed),
                            PkiOperation::InstallCertificate => {
                                install_certificate_at(config_dir, &req, enforce_fixed)
                            }
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
            }
        },
    };
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

        const WAIT: Duration = Duration::from_secs(65);
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
        "server_name": server_name,
        "certificate_pem": null,
        "ca_pem": null,
        "crl_pem": null
    })
}

pub fn certificate_request_document(
    request_id: &str,
    node_id: &str,
    server_name: &str,
    certificate_pem: &str,
    ca_pem: &str,
    crl_pem: &str,
) -> Value {
    json!({
        "request_version": 1,
        "request_id": request_id,
        "operation": PkiOperation::InstallCertificate,
        "node_id": node_id,
        "server_name": server_name,
        "certificate_pem": certificate_pem,
        "ca_pem": ca_pem,
        "crl_pem": crl_pem
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
    fn parses_complete_named_curve_spki_and_rejects_oid_decoys_and_trailing_der() {
        let (tmp, _, _, _) = fixture();
        let key = tmp.0.join("p256.pem");
        let output = Command::new(OPENSSL)
            .args([
                "ecparam",
                "-name",
                "prime256v1",
                "-genkey",
                "-noout",
                "-param_enc",
                "named_curve",
                "-out",
            ])
            .arg(&key)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let public = Command::new(OPENSSL)
            .args(["pkey", "-in"])
            .arg(&key)
            .args(["-pubout", "-outform", "DER"])
            .output()
            .unwrap();
        assert!(public.status.success());
        assert!(is_p256_subject_public_key_info(&public.stdout));

        let p384_key = tmp.0.join("p384.pem");
        let p384 = Command::new(OPENSSL)
            .args([
                "ecparam",
                "-name",
                "secp384r1",
                "-genkey",
                "-noout",
                "-param_enc",
                "named_curve",
                "-out",
            ])
            .arg(&p384_key)
            .output()
            .unwrap();
        assert!(p384.status.success());
        let p384_public = Command::new(OPENSSL)
            .args(["pkey", "-in"])
            .arg(&p384_key)
            .args(["-pubout", "-outform", "DER"])
            .output()
            .unwrap();
        assert!(p384_public.status.success());
        assert!(!is_p256_subject_public_key_info(&p384_public.stdout));

        // Explicit EC parameters are rejected; CSR/key generation below explicitly requests
        // named-curve encoding so supported OpenSSL/LibreSSL variants emit the same SPKI form.
        let explicit_key = tmp.0.join("p256-explicit.pem");
        let explicit = Command::new(OPENSSL)
            .args([
                "ecparam",
                "-name",
                "prime256v1",
                "-genkey",
                "-noout",
                "-param_enc",
                "explicit",
                "-out",
            ])
            .arg(&explicit_key)
            .output()
            .unwrap();
        assert!(explicit.status.success());
        let explicit_public = Command::new(OPENSSL)
            .args(["pkey", "-in"])
            .arg(&explicit_key)
            .args(["-pubout", "-outform", "DER"])
            .output()
            .unwrap();
        assert!(explicit_public.status.success());
        assert!(!is_p256_subject_public_key_info(&explicit_public.stdout));

        let mut trailing = public.stdout.clone();
        trailing.push(0);
        assert!(!is_p256_subject_public_key_info(&trailing));
        let oid_decoy = [
            0x30, 0x0a, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07,
        ];
        assert!(!is_p256_subject_public_key_info(&oid_decoy));
        assert!(!is_p256_subject_public_key_info(&[0x30, 0x01, 0x00]));
    }

    #[test]
    fn installs_only_a_valid_offline_certificate_set_and_switches_one_pointer() {
        let (tmp, control, config, receipts) = fixture();
        let create = request(
            "csr-install-01",
            PkiOperation::CreateCsr,
            "node1",
            "localhost",
        );
        let created = create_csr_at(&config, &test_request(&create), false).unwrap();
        assert_eq!(created["result"], "created");

        let ca = crate::tls::tests::make_ca(&tmp.0, "install-ca");
        let ca_dir = ca.ca_pem.parent().unwrap();
        let ext = tmp.0.join("node-ext.cnf");
        fs::write(
            &ext,
            "[node_ext]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=URI:urn:baft-tunnel:node:node1,DNS:localhost\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n",
        )
        .unwrap();
        let cert = tmp.0.join("issued-node.crt");
        let status = Command::new("openssl")
            .args(["ca", "-batch", "-notext", "-config"])
            .arg(ca_dir.join("openssl.cnf"))
            .arg("-extfile")
            .arg(&ext)
            .args(["-extensions", "node_ext", "-days", "90", "-in"])
            .arg(config.join("node.csr"))
            .arg("-out")
            .arg(&cert)
            .status()
            .unwrap();
        assert!(status.success(), "offline CA must issue the matching CSR");
        let crl = tmp.0.join("fresh.crl");
        let status = Command::new("openssl")
            .args(["ca", "-gencrl", "-batch", "-config"])
            .arg(ca_dir.join("openssl.cnf"))
            .arg("-out")
            .arg(&crl)
            .status()
            .unwrap();
        assert!(status.success(), "offline CA must issue a fresh CRL");

        let cert_pem = fs::read_to_string(&cert).unwrap();
        let ca_pem = fs::read_to_string(&ca.ca_pem).unwrap();
        let crl_pem = fs::read_to_string(&crl).unwrap();
        let request = certificate_request_document(
            "cert-install-01",
            "node1",
            "localhost",
            &cert_pem,
            &ca_pem,
            &crl_pem,
        );
        put(&control, &request);
        let receipt = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(receipt["result"], "installed", "{receipt:?}");
        assert_eq!(receipt["operation"], "install_certificate");
        assert_eq!(receipt["private_key_exported"], false);
        assert_eq!(receipt["service_restarted"], Value::Null);
        let active = active_set_dir(&config).unwrap().unwrap();
        assert_eq!(
            active.file_name().unwrap(),
            receipt["active_set"].as_str().unwrap()
        );
        let runtime_check = crate::tls::server_config(
            &active.join("ca.crt"),
            &active.join("ca.crl"),
            &active.join("node.crt"),
            &config.join("node.key"),
        );
        assert!(
            runtime_check.is_ok(),
            "runtime TLS config: {:?}",
            runtime_check.err()
        );
        assert!(control.join("node1.pki-result").is_file());

        // A fresh request ID with identical bytes is idempotent and does not advance/rewrite
        // the active credential pointer or demand a higher CRL number.
        let retry = certificate_request_document(
            "cert-install-retry-01",
            "node1",
            "localhost",
            &cert_pem,
            &ca_pem,
            &crl_pem,
        );
        put(&control, &retry);
        let replay = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(replay["result"], "installed");
        assert_eq!(replay["active_set"], receipt["active_set"]);
        assert_eq!(active_set_dir(&config).unwrap(), Some(active.clone()));

        // A newer CRL from the same CA can atomically replace the complete active set.
        let ca_dir = ca.ca_pem.parent().unwrap();
        let newer_crl = tmp.0.join("newer.crl");
        let status = Command::new("openssl")
            .args(["ca", "-gencrl", "-batch", "-config"])
            .arg(ca_dir.join("openssl.cnf"))
            .arg("-out")
            .arg(&newer_crl)
            .status()
            .unwrap();
        assert!(status.success());
        let newer_crl_pem = fs::read_to_string(&newer_crl).unwrap();
        let newer_request = certificate_request_document(
            "cert-install-newer-crl",
            "node1",
            "localhost",
            &cert_pem,
            &ca_pem,
            &newer_crl_pem,
        );
        put(&control, &newer_request);
        let newer = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(newer["result"], "installed", "{newer:?}");
        assert_ne!(newer["active_set"], receipt["active_set"]);
        let new_active = active_set_dir(&config).unwrap().unwrap();

        // Replaying the older CRL cannot roll back the active revocation state.
        let stale_request = certificate_request_document(
            "cert-install-stale-crl",
            "node1",
            "localhost",
            &cert_pem,
            &ca_pem,
            &crl_pem,
        );
        put(&control, &stale_request);
        let stale = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(stale["result"], "refused");
        assert_eq!(active_set_dir(&config).unwrap(), Some(new_active));
    }

    #[test]
    fn rejects_wrong_certificate_identity_without_changing_active_pointer() {
        let (tmp, control, config, receipts) = fixture();
        let create = request(
            "csr-install-02",
            PkiOperation::CreateCsr,
            "node1",
            "localhost",
        );
        create_csr_at(&config, &test_request(&create), false).unwrap();
        let ca = crate::tls::tests::make_ca(&tmp.0, "wrong-ca");
        let ca_dir = ca.ca_pem.parent().unwrap();
        let ext = tmp.0.join("wrong-ext.cnf");
        fs::write(
            &ext,
            "[node_ext]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=URI:urn:baft-tunnel:node:other-node,DNS:localhost\n",
        ).unwrap();
        let cert = tmp.0.join("wrong-node.crt");
        let status = Command::new("openssl")
            .args(["ca", "-batch", "-notext", "-config"])
            .arg(ca_dir.join("openssl.cnf"))
            .arg("-extfile")
            .arg(&ext)
            .args(["-extensions", "node_ext", "-days", "90", "-in"])
            .arg(config.join("node.csr"))
            .arg("-out")
            .arg(&cert)
            .status()
            .unwrap();
        assert!(status.success());
        let request = certificate_request_document(
            "cert-install-02",
            "node1",
            "localhost",
            &fs::read_to_string(cert).unwrap(),
            &fs::read_to_string(ca.ca_pem).unwrap(),
            &fs::read_to_string(ca.crl_pem).unwrap(),
        );
        put(&control, &request);
        let receipt = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(receipt["result"], "refused");
        assert!(active_set_dir(&config).unwrap().is_none());
    }

    #[test]
    fn installer_created_pki_root_without_sets_is_a_pending_enrollment_state() {
        let (_tmp, _control, config, _receipts) = fixture();
        let root = config.join("pki");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            active_set_dir(&config).unwrap().is_none(),
            "a pristine installer-created PKI root without sets/current must mean no active set"
        );

        std::os::unix::fs::symlink(format!("sets/{}", "a".repeat(64)), root.join("current"))
            .unwrap();
        assert!(
            active_set_dir(&config).is_err(),
            "a current pointer without its sets directory must remain a hard error"
        );
    }

    #[test]
    fn active_pki_set_requires_a_fixed_digest_pointer_and_regular_material() {
        let (tmp, _control, config, _receipts) = fixture();
        assert!(active_set_dir(&config).unwrap().is_none());
        let root = config.join("pki");
        let sets = root.join("sets");
        let cert = b"public test certificate";
        let ca = b"public test CA";
        let crl = b"public test CRL";
        let id = version_set_id(cert, ca, crl);
        let active = sets.join(&id);
        fs::create_dir_all(&active).unwrap();
        fs::write(active.join("node.crt"), cert).unwrap();
        fs::write(active.join("ca.crt"), ca).unwrap();
        fs::write(active.join("ca.crl"), crl).unwrap();
        std::os::unix::fs::symlink(format!("sets/{id}"), root.join("current")).unwrap();
        assert_eq!(active_set_dir(&config).unwrap(), Some(active.clone()));

        // The active set must remain immutable to the service group and other local users.
        fs::set_permissions(active.join("node.crt"), fs::Permissions::from_mode(0o664)).unwrap();
        assert!(
            active_set_dir(&config).is_err(),
            "group-writable certificate must be refused"
        );
        fs::set_permissions(active.join("node.crt"), fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&active, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(
            active_set_dir(&config).is_err(),
            "group-writable active set must be refused"
        );
        fs::set_permissions(&active, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&sets, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(
            active_set_dir(&config).is_err(),
            "group-writable sets directory must be refused"
        );
        fs::set_permissions(&sets, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(
            active_set_dir(&config).is_err(),
            "group-writable PKI root must be refused"
        );
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        fs::remove_file(root.join("current")).unwrap();
        std::os::unix::fs::symlink("../outside", root.join("current")).unwrap();
        assert!(active_set_dir(&config).is_err());
        fs::remove_file(root.join("current")).unwrap();
        std::os::unix::fs::symlink(format!("sets/{}", "A".repeat(64)), root.join("current"))
            .unwrap();
        assert!(active_set_dir(&config).is_err());

        fs::remove_file(root.join("current")).unwrap();
        fs::remove_file(active.join("node.crt")).unwrap();
        std::os::unix::fs::symlink(active.join("ca.crt"), active.join("node.crt")).unwrap();
        std::os::unix::fs::symlink(format!("sets/{id}"), root.join("current")).unwrap();
        assert!(active_set_dir(&config).is_err());
        drop(tmp);
    }

    #[test]
    fn refuses_symlinked_receipt_root_without_changing_target_permissions() {
        let (tmp, control, config, receipts) = fixture();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&config, &receipts).unwrap();
        put(
            &control,
            &request(
                "csr-symlink-receipt",
                PkiOperation::CreateCsr,
                "node1",
                "localhost",
            ),
        );
        let receipt = run_at("node1", &control, &config, &receipts, false).unwrap();
        assert_eq!(receipt["result"], "refused", "{receipt}");
        assert!(
            receipt["reason"]
                .as_str()
                .unwrap()
                .contains("receipt directory")
        );
        assert!(control.join("node1.pki-result").exists());
        assert_eq!(
            fs::metadata(&config).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!config.join("node.key").exists());
        assert!(!config.join("node.csr").exists());
        drop(tmp);
    }

    #[test]
    fn receipt_parent_failure_returns_refused_result_without_waiting_for_timeout() {
        let (_tmp, control, config, receipts) = fixture();
        put(
            &control,
            &request(
                "csr-parent-missing",
                PkiOperation::CreateCsr,
                "node1",
                "localhost",
            ),
        );
        let missing_parent = receipts.join("missing").join("pki-receipts");
        let receipt = run_at("node1", &control, &config, &missing_parent, false).unwrap();
        assert_eq!(receipt["result"], "refused", "{receipt}");
        assert!(
            receipt["reason"]
                .as_str()
                .unwrap()
                .contains("cannot prepare the root-owned receipt directory"),
            "{receipt}"
        );
        let result: Value =
            serde_json::from_slice(&fs::read(control.join("node1.pki-result")).unwrap()).unwrap();
        assert_eq!(result["result"], "refused");
        assert_eq!(result["request_id"], "csr-parent-missing");
        assert!(!config.join("node.key").exists());
        assert!(!config.join("node.csr").exists());
    }

    #[test]
    fn refuses_group_writable_receipt_root() {
        let (tmp, _control, _config, receipts) = fixture();
        fs::create_dir(&receipts).unwrap();
        fs::set_permissions(&receipts, fs::Permissions::from_mode(0o770)).unwrap();
        assert!(ensure_private_receipt_dir(&receipts).is_err());
        assert_eq!(
            fs::metadata(&receipts).unwrap().permissions().mode() & 0o777,
            0o770
        );
        drop(tmp);
    }

    #[test]
    fn fixed_pki_directory_requires_root_service_group_and_0750() {
        assert!(validate_fixed_directory_metadata(0, 42, 0o40750, 42).is_ok());
        assert!(validate_fixed_directory_metadata(1000, 42, 0o40750, 42).is_err());
        assert!(validate_fixed_directory_metadata(0, 1000, 0o40750, 42).is_err());
        assert!(validate_fixed_directory_metadata(0, 42, 0o40770, 42).is_err());
        assert!(validate_fixed_directory_metadata(0, 42, 0o40755, 42).is_err());
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
                    if unsafe { libc::geteuid() } == 0 {
                        if let Some((_, gid)) = crate::setup::service_ids() {
                            use std::os::fd::AsRawFd;
                            assert_eq!(unsafe { libc::fchown(file.as_raw_fd(), 0, gid) }, 0);
                        }
                    }
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
        let receipt = client.submit("node1", &request);
        worker.join().unwrap();
        if unsafe { libc::geteuid() } == 0 && crate::setup::service_ids().is_none() {
            assert!(
                receipt.is_err(),
                "root must reject receipts without the service identity"
            );
        } else {
            let receipt = receipt.unwrap();
            assert_eq!(receipt["request_id"], "csr-client-01");
            assert_eq!(receipt["operation"], "read_csr");
        }
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
        if unsafe { libc::geteuid() } == 0 {
            if let Some((_, gid)) = crate::setup::service_ids() {
                use std::os::fd::AsRawFd;
                assert_eq!(unsafe { libc::fchown(file.as_raw_fd(), 0, gid) }, 0);
            }
        }
        drop(file);
        if unsafe { libc::geteuid() } == 0 && crate::setup::service_ids().is_none() {
            assert!(read_helper_receipt(&receipt, 1024).is_err());
        } else {
            assert!(read_helper_receipt(&receipt, 1024).is_ok());
        }
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
