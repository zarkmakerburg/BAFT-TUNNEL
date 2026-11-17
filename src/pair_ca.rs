//! Per-tunnel pair CA for automatic pairing (ADR-0050, stage 2).
//!
//! A small CA created for exactly one tunnel and exactly two nodes. The CA key lives only in a
//! root-only directory on the listener host; it is never exported. Signing is allowed only for
//! the two node ids fixed at creation, with the DNS/IP name fixed for each, and only for a valid
//! EC P-256 CSR whose CN equals the node id (CSR extensions are never copied, as in ADR-0012).
//! The CA certificate carries X.509 name constraints for those two DNS/IP names.
//! Uses `/usr/bin/openssl`, like `pki_helper`, so no new crypto dependency is added.

#![allow(dead_code)] // wired into the helper and agent in ADR-0050 stage 4

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

const OPENSSL: &str = "/usr/bin/openssl";
const NODE_URI_PREFIX: &str = "urn:baft-tunnel:node:";
const MAX_CSR_BYTES: usize = 4096;
/// Leaf validity; automatic renewal is a later stage (ADR-0050 plan step 6).
pub const LEAF_DAYS: u32 = 365;
/// CRL validity for the (initially empty) pair CRL; renewed automatically by the same helper.
pub const CRL_DAYS: u32 = 90;

/// One allowed node: its id and the DNS name or IP its certificate carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub node_id: String,
    /// `DNS:name` or `IP:addr`.
    pub san: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Members {
    version: u8,
    members: [Member; 2],
}

fn valid_node_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

fn validate_san(san: &str) -> Result<()> {
    if let Some(ip) = san.strip_prefix("IP:") {
        ip.parse::<std::net::IpAddr>().context("invalid IP SAN")?;
    } else if let Some(dns) = san.strip_prefix("DNS:") {
        ensure!(
            !dns.is_empty()
                && dns.len() <= 253
                && !dns.starts_with(['.', '-'])
                && dns
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.'),
            "invalid DNS SAN"
        );
    } else {
        bail!("SAN must start with DNS: or IP:");
    }
    Ok(())
}

fn constraint(san: &str) -> String {
    match san.strip_prefix("IP:") {
        Some(ip) if ip.contains(':') => {
            format!("permitted;IP:{ip}/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff")
        }
        Some(ip) => format!("permitted;IP:{ip}/255.255.255.255"),
        None => format!("permitted;{san}"),
    }
}

pub(crate) fn run(args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut child = Command::new(OPENSSL)
        .args(args)
        .env_clear()
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start openssl")?;
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .context("openssl stdin")?
            .write_all(input)?;
    }
    let out = child.wait_with_output()?;
    ensure!(
        out.status.success(),
        "openssl {} failed",
        args.first().unwrap_or(&"")
    );
    Ok(out.stdout)
}

pub(crate) fn write_private(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    f.write_all(bytes)?;
    Ok(())
}

/// Create the CA in an empty or absent `dir` (mode 0700). Returns the CA certificate PEM.
pub fn create(dir: &Path, name: &str, members: &[Member; 2]) -> Result<String> {
    ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" ._-".contains(&b)),
        "invalid CA name"
    );
    for m in members {
        ensure!(valid_node_id(&m.node_id), "invalid node id");
        validate_san(&m.san)?;
    }
    ensure!(
        members[0].node_id != members[1].node_id,
        "node ids must differ"
    );
    if dir.exists() {
        ensure!(
            fs::read_dir(dir)?.next().is_none(),
            "refusing to initialize a non-empty directory"
        );
    } else {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(dir.join("newcerts"))?;
    let d = dir.display();
    write_private(&dir.join("index.txt"), b"", 0o600)?;
    write_private(&dir.join("serial"), b"1000\n", 0o600)?;
    write_private(&dir.join("crlnumber"), b"1000\n", 0o600)?;
    write_private(
        &dir.join("members.json"),
        &serde_json::to_vec_pretty(&Members {
            version: 1,
            members: members.clone(),
        })?,
        0o600,
    )?;
    let cnf = format!(
        "[ ca ]\ndefault_ca = CA_default\n[ CA_default ]\ndatabase = \"{d}/index.txt\"\nnew_certs_dir = \"{d}/newcerts\"\ncertificate = \"{d}/ca.crt\"\nprivate_key = \"{d}/ca.key\"\nserial = \"{d}/serial\"\ncrlnumber = \"{d}/crlnumber\"\ndefault_md = sha384\ndefault_days = {LEAF_DAYS}\ndefault_crl_days = {CRL_DAYS}\npolicy = policy_node\nunique_subject = no\ncopy_extensions = none\ncrl_extensions = crl_ext\n[ policy_node ]\ncommonName = supplied\n[ crl_ext ]\nauthorityKeyIdentifier = keyid:always\n"
    );
    write_private(&dir.join("openssl.cnf"), cnf.as_bytes(), 0o600)?;
    let key = dir.join("ca.key");
    let key_s = key.to_string_lossy().into_owned();
    // The key is generated into a 0600 file in a 0700 directory and is never printed.
    let pem = run(
        &[
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:P-384",
        ],
        None,
    )?;
    write_private(&key, &pem, 0o600)?;
    let nc = format!(
        "nameConstraints=critical,{},{}",
        constraint(&members[0].san),
        constraint(&members[1].san)
    );
    let ca_crt = dir.join("ca.crt");
    run(
        &[
            "req",
            "-new",
            "-x509",
            "-key",
            &key_s,
            "-out",
            &ca_crt.to_string_lossy(),
            "-days",
            "1825",
            "-sha384",
            "-subj",
            &format!("/CN={name}"),
            "-addext",
            "basicConstraints=critical,CA:TRUE,pathlen:0",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-addext",
            &nc,
        ],
        None,
    )?;
    crl(dir, CRL_DAYS)?;
    Ok(fs::read_to_string(ca_crt)?)
}

/// Issue a CRL (the CA database is empty unless something was revoked) valid for `days`.
pub fn crl(dir: &Path, days: u32) -> Result<String> {
    ensure!((2..=365).contains(&days), "CRL days out of range");
    let cnf = dir.join("openssl.cnf");
    let out = run(
        &[
            "ca",
            "-gencrl",
            "-batch",
            "-crldays",
            &days.to_string(),
            "-config",
            &cnf.to_string_lossy(),
        ],
        None,
    )?;
    let pem = String::from_utf8(out).context("CRL is not UTF-8")?;
    fs::write(dir.join("ca.crl"), &pem)?;
    Ok(pem)
}

/// Sign `csr_pem` for `node_id`. Refuses unknown ids, wrong CN, non-P-256 keys, bad CSR signature.
pub fn sign(dir: &Path, node_id: &str, csr_pem: &str) -> Result<String> {
    ensure!(csr_pem.len() <= MAX_CSR_BYTES, "CSR too large");
    ensure!(
        csr_pem
            .matches("-----BEGIN CERTIFICATE REQUEST-----")
            .count()
            == 1
            && csr_pem
                .trim_end()
                .ends_with("-----END CERTIFICATE REQUEST-----")
            && csr_pem
                .trim_start()
                .starts_with("-----BEGIN CERTIFICATE REQUEST-----"),
        "CSR must be exactly one PEM request"
    );
    let members: Members = serde_json::from_slice(&fs::read(dir.join("members.json"))?)?;
    let member = members
        .members
        .iter()
        .find(|m| m.node_id == node_id)
        .context("node id is not part of this pairing")?;
    let verify = run(&["req", "-verify", "-noout"], Some(csr_pem.as_bytes()));
    ensure!(verify.is_ok(), "CSR signature is invalid");
    let subject = String::from_utf8(run(
        &["req", "-noout", "-subject", "-nameopt", "RFC2253"],
        Some(csr_pem.as_bytes()),
    )?)?;
    ensure!(
        subject.trim() == format!("subject=CN={node_id}"),
        "CSR subject CN must equal the node id"
    );
    let pubkey = run(&["req", "-pubkey", "-noout"], Some(csr_pem.as_bytes()))?;
    let info = String::from_utf8(run(&["pkey", "-pubin", "-text", "-noout"], Some(&pubkey))?)?;
    ensure!(
        info.contains("prime256v1"),
        "CSR public key must be EC P-256"
    );
    let tmp = dir.join(format!("sign-{}.tmp", rand::random::<u64>()));
    (|| -> Result<String> {
        let ext = format!(
            "[node_ext]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=URI:{NODE_URI_PREFIX}{node_id},{}\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n",
            member.san
        );
        let ext_path: PathBuf = tmp.with_extension("ext");
        let csr_path: PathBuf = tmp.with_extension("csr");
        write_private(&ext_path, ext.as_bytes(), 0o600)?;
        write_private(&csr_path, csr_pem.as_bytes(), 0o600)?;
        let cnf = dir.join("openssl.cnf");
        let out = run(
            &[
                "ca",
                "-batch",
                "-notext",
                "-config",
                &cnf.to_string_lossy(),
                "-extfile",
                &ext_path.to_string_lossy(),
                "-extensions",
                "node_ext",
                "-days",
                &LEAF_DAYS.to_string(),
                "-in",
                &csr_path.to_string_lossy(),
                "-out",
                "/dev/stdout",
            ],
            None,
        );
        let _ = fs::remove_file(&ext_path);
        let _ = fs::remove_file(&csr_path);
        Ok(String::from_utf8(out?)?)
    })()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pair-ca-{tag}-{:x}", rand::random::<u64>()))
    }

    fn csr(dir: &Path, node: &str, cn: &str) -> (PathBuf, String) {
        let key = dir.join(format!("{node}.key"));
        let out = run(
            &[
                "req",
                "-new",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
                "-nodes",
                "-keyout",
                &key.to_string_lossy(),
                "-subj",
                &format!("/CN={cn}"),
            ],
            None,
        )
        .unwrap();
        (key, String::from_utf8(out).unwrap())
    }

    fn members() -> [Member; 2] {
        [
            Member {
                node_id: "node-a".into(),
                san: "IP:127.0.0.1".into(),
            },
            Member {
                node_id: "node-b".into(),
                san: "DNS:node-b.pair.test".into(),
            },
        ]
    }

    #[test]
    fn issued_certificates_pass_the_runtime_policy() {
        let dir = tmpdir("ok");
        let work = tmpdir("work");
        fs::create_dir_all(&work).unwrap();
        let ca = create(&dir.join("ca"), "Pair CA t", &members()).unwrap();
        for (id, san) in [
            ("node-a", "IP:127.0.0.1"),
            ("node-b", "DNS:node-b.pair.test"),
        ] {
            let (key, req) = csr(&work, id, id);
            let leaf = sign(&dir.join("ca"), id, &req).unwrap();
            fs::write(work.join(format!("{id}.crt")), &leaf).unwrap();
            fs::write(work.join("ca.crt"), &ca).unwrap();
            fs::copy(dir.join("ca/ca.crl"), work.join("ca.crl")).unwrap();
            crate::tls::verify_local_certificate(
                &work.join("ca.crt"),
                &work.join("ca.crl"),
                &work.join(format!("{id}.crt")),
                san,
            )
            .unwrap_or_else(|e| panic!("{id}: {e:#}"));
            let certs = crate::tls::certificates(&work.join(format!("{id}.crt"))).unwrap();
            crate::pki_helper::validate_leaf_policy(
                certs[0].as_ref(),
                id,
                san,
                crate::pki_helper::IdentityProfile::BaftTunnel,
            )
            .unwrap();
            assert!(key.exists());
        }
        assert_eq!(
            fs::metadata(dir.join("ca/ca.key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&work);
    }

    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn refuses_unknown_node_wrong_cn_and_reinit() {
        let dir = tmpdir("deny");
        let work = tmpdir("w2");
        fs::create_dir_all(&work).unwrap();
        create(&dir.join("ca"), "Pair CA t", &members()).unwrap();
        let (_k, req) = csr(&work, "x", "intruder");
        assert!(sign(&dir.join("ca"), "intruder", &req).is_err());
        let (_k2, req2) = csr(&work, "y", "node-b");
        assert!(
            sign(&dir.join("ca"), "node-a", &req2).is_err(),
            "CN must match node id"
        );
        assert!(create(&dir.join("ca"), "again", &members()).is_err());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn rejects_bad_inputs() {
        let dir = tmpdir("bad");
        let mut m = members();
        m[1].san = "DNS:bad name".into();
        assert!(create(&dir, "x", &m).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
