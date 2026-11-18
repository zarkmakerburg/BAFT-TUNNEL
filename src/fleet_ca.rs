//! ADR-0051 D1 / M5: the Fleet CA that signs `fleet-node` reporting certificates.
//!
//! The per-tunnel CA (ADR-0050) signs exactly two fixed node ids, so it cannot enroll a third
//! panel-added server. This separate CA exists for that one purpose and is bounded by M5:
//! its key lives only in a root-only directory (the root helper owns it), it is name
//! constrained to `*.fleet.baft.invalid`, signs only the `fleet-node` profile (the single BAFT
//! EKU from `fleet::EKU_FLEET_NODE` plus `clientAuth`, F-4), leaves live at most 30 days, and a
//! CRL is kept. It never signs a tunnel certificate and a tunnel CA never signs a fleet one.
//! Uses `/usr/bin/openssl` through `pair_ca`, so no crypto dependency is added.

#![allow(dead_code)] // wired by the root helper in the enrollment slice

use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use crate::fleet::EKU_FLEET_NODE;
use crate::pair_ca::{check_csr, run, write_private};

/// M5: at most 30 days.
pub const LEAF_DAYS: u32 = 30;
pub const CRL_DAYS: u32 = 30;
const DOMAIN: &str = "fleet.baft.invalid";

/// Fleet node ids become a DNS label inside the certificate name, so they are lowercase
/// letters, digits and hyphens only.
pub fn valid_node_id(id: &str) -> bool {
    (1..=63).contains(&id.len())
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The URI name a fleet-node certificate carries; the hub takes the node id from here (F-5).
pub fn node_uri(node_id: &str) -> String {
    format!("baft-fleet://{node_id}.{DOMAIN}")
}

/// Create the Fleet CA in an empty or absent `dir` (mode 0700). Returns the CA certificate PEM.
pub fn create(dir: &Path, name: &str) -> Result<String> {
    ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" ._-".contains(&b)),
        "invalid CA name"
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
    let cnf = format!(
        "[ ca ]\ndefault_ca = CA_default\n[ CA_default ]\ndatabase = \"{d}/index.txt\"\nnew_certs_dir = \"{d}/newcerts\"\ncertificate = \"{d}/ca.crt\"\nprivate_key = \"{d}/ca.key\"\nserial = \"{d}/serial\"\ncrlnumber = \"{d}/crlnumber\"\ndefault_md = sha384\ndefault_days = {LEAF_DAYS}\ndefault_crl_days = {CRL_DAYS}\npolicy = policy_node\nunique_subject = no\ncopy_extensions = none\ncrl_extensions = crl_ext\n[ policy_node ]\ncommonName = supplied\n[ crl_ext ]\nauthorityKeyIdentifier = keyid:always\n"
    );
    write_private(&dir.join("openssl.cnf"), cnf.as_bytes(), 0o600)?;
    let key = dir.join("ca.key");
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
    let ca_crt = dir.join("ca.crt");
    run(
        &[
            "req",
            "-new",
            "-x509",
            "-key",
            &key.to_string_lossy(),
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
            &format!("nameConstraints=critical,permitted;URI:.{DOMAIN}"),
        ],
        None,
    )?;
    crate::pair_ca::crl(dir, CRL_DAYS)?;
    Ok(fs::read_to_string(ca_crt)?)
}

/// Sign a `fleet-node` certificate for `node_id`. Same CSR gate as the tunnel CA (one PEM
/// request, CN equals the node id, EC P-256); the certificate carries only clientAuth plus the
/// fleet-node EKU, a URI name for the id, and lives `LEAF_DAYS`.
pub fn sign(dir: &Path, node_id: &str, csr_pem: &str) -> Result<String> {
    ensure!(valid_node_id(node_id), "invalid fleet node id");
    check_csr(csr_pem, node_id)?;
    let tmp = dir.join(format!("sign-{}.tmp", rand::random::<u64>()));
    let ext = format!(
        "[node_ext]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth,{EKU_FLEET_NODE}\nsubjectAltName=URI:{}\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n",
        node_uri(node_id)
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
    String::from_utf8(out?).context("certificate is not UTF-8")
}

/// Revoke a previously issued certificate (PEM) and refresh the CRL.
pub fn revoke(dir: &Path, cert_pem: &str) -> Result<String> {
    ensure!(
        cert_pem.matches("-----BEGIN CERTIFICATE-----").count() == 1 && cert_pem.len() <= 4096,
        "expected exactly one certificate"
    );
    let p = dir.join(format!("revoke-{}.pem", rand::random::<u64>()));
    write_private(&p, cert_pem.as_bytes(), 0o600)?;
    let cnf = dir.join("openssl.cnf");
    let r = run(
        &[
            "ca",
            "-batch",
            "-config",
            &cnf.to_string_lossy(),
            "-revoke",
            &p.to_string_lossy(),
        ],
        None,
    );
    let _ = fs::remove_file(&p);
    r?;
    crate::pair_ca::crl(dir, CRL_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fleet-ca-{tag}-{:x}", rand::random::<u64>()))
    }

    fn csr(cn: &str, curve: &str) -> String {
        let key = String::from_utf8(
            run(
                &[
                    "genpkey",
                    "-algorithm",
                    "EC",
                    "-pkeyopt",
                    &format!("ec_paramgen_curve:{curve}"),
                ],
                None,
            )
            .unwrap(),
        )
        .unwrap();
        let d = tmp("k");
        fs::create_dir_all(&d).unwrap();
        let kp = d.join("k.pem");
        fs::write(&kp, key).unwrap();
        let out = run(
            &[
                "req",
                "-new",
                "-key",
                &kp.to_string_lossy(),
                "-subj",
                &format!("/CN={cn}"),
            ],
            None,
        )
        .unwrap();
        let _ = fs::remove_dir_all(d);
        String::from_utf8(out).unwrap()
    }

    fn text(pem: &str) -> String {
        String::from_utf8(run(&["x509", "-noout", "-text"], Some(pem.as_bytes())).unwrap()).unwrap()
    }

    #[test]
    fn node_ids() {
        assert!(valid_node_id("iran-1"));
        for bad in ["", "-a", "a-", "A", "a_b", "a.b", "a/b", &"a".repeat(64)] {
            assert!(!valid_node_id(bad), "{bad}");
        }
    }

    #[test]
    fn issues_fleet_node_profile_only_and_verifies() {
        let dir = tmp("ca");
        let ca = create(&dir, "BAFT Fleet CA").unwrap();
        let ca_text = text(&ca);
        assert!(ca_text.contains("Name Constraints"));
        assert!(ca_text.contains("fleet.baft.invalid"));
        let cert = sign(&dir, "iran-1", &csr("iran-1", "prime256v1")).unwrap();
        let t = text(&cert);
        assert!(t.contains("TLS Web Client Authentication"), "{t}");
        assert!(
            t.contains(EKU_FLEET_NODE) || t.contains("1.3.6.1.4.1.59999.1.2"),
            "{t}"
        );
        assert!(!t.contains("TLS Web Server Authentication"));
        assert!(t.contains("baft-fleet://iran-1.fleet.baft.invalid"));
        assert!(t.contains("CA:FALSE"));
        // other BAFT profiles' EKUs must be absent (F-4)
        for other in [
            "1.3.6.1.4.1.59999.1.1",
            "1.3.6.1.4.1.59999.1.3",
            "1.3.6.1.4.1.59999.1.4",
        ] {
            assert!(!t.contains(other), "{other}");
        }
        // lifetime at most 30 days
        let (cp, ap) = (dir.join("leaf.pem"), dir.join("ca.crt"));
        fs::write(&cp, &cert).unwrap();
        assert!(
            run(
                &[
                    "verify",
                    "-CAfile",
                    &ap.to_string_lossy(),
                    &cp.to_string_lossy()
                ],
                None
            )
            .is_ok()
        );
        let ok_30 = run(
            &[
                "x509",
                "-noout",
                "-checkend",
                &(30 * 86400 + 60).to_string(),
            ],
            Some(cert.as_bytes()),
        );
        assert!(ok_30.is_err(), "leaf must not outlive 30 days");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_bad_requests_and_revokes() {
        let dir = tmp("ca2");
        create(&dir, "BAFT Fleet CA").unwrap();
        assert!(sign(&dir, "iran-1", &csr("other", "prime256v1")).is_err());
        assert!(sign(&dir, "iran-1", &csr("iran-1", "secp384r1")).is_err());
        assert!(sign(&dir, "../x", &csr("iran-1", "prime256v1")).is_err());
        assert!(sign(&dir, "Iran", &csr("iran-1", "prime256v1")).is_err());
        let cert = sign(&dir, "n2", &csr("n2", "prime256v1")).unwrap();
        let crl = revoke(&dir, &cert).unwrap();
        let ctext =
            String::from_utf8(run(&["crl", "-noout", "-text"], Some(crl.as_bytes())).unwrap())
                .unwrap();
        assert!(ctext.contains("Serial Number"), "{ctext}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_non_empty_directory() {
        let dir = tmp("ca3");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("x"), b"1").unwrap();
        assert!(create(&dir, "x").is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
