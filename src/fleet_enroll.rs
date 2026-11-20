//! ADR-0045 F-7 / ADR-0051 Path A: the hub-side enrollment state machine for a new fleet node.
//!
//! The operator creates an offer (node id + one-time token + nonce, 15 minute TTL). The new
//! server submits a CSR. The hub validates the CSR signature, exact subject and allowed key profile,
//! then computes the canonical public-key fingerprint; client-supplied fingerprints and unkeyed
//! proofs are never trusted. The CSR is not signed until the operator confirms that fingerprint
//! (F-7). A wrong token or node id counts against a small
//! failure budget and then closes the window; a valid token is consumed by the first submit.
//! The Enroller owns only ephemeral protocol state. CSR validation uses the existing OpenSSL-backed
//! Fleet CA gate; certificate signing is injected through the root-helper boundary, so this module
//! never holds a CA private key. Delivery and first-report acknowledgement are separate
//! transitions; signing alone never marks a node Enrolled. Transport wiring comes in later slices.

#![allow(dead_code)] // wired by the Add-server API and the join listener in later slices

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::fleet_ca::valid_node_id;

pub const OFFER_TTL_SECS: u64 = 900;
pub const MAX_FAILURES: u32 = 3;
pub const MAX_OPEN_OFFERS: usize = 16;
const MAX_CSR_BYTES: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    BadNodeId,
    TooManyOffers,
    NodeIdTaken,
    Unknown,
    Expired,
    BadToken,
    BadCsr,
    WrongState,
    FingerprintMismatch,
    BadCertificate,
    Sign,
    WrongNode,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum State {
    Offered,
    Submitted,
    Signing,
    Issued,
    AwaitingReport,
    Enrolled,
    Closed,
}

struct Offer {
    node_id: String,
    token_hash: [u8; 32],
    nonce: String,
    expires: u64,
    failures: u32,
    state: State,
    csr: Option<String>,
    fingerprint: Option<String>,
    certificate: Option<String>,
    delivered_fingerprint: Option<String>,
    submit_digest: Option<[u8; 32]>,
}

/// What the operator hands to the new server (shown once).
pub struct Issued {
    pub id: String,
    pub node_id: String,
    pub token: String,
    pub nonce: String,
    pub expires: u64,
}

/// What the operator sees while waiting for the confirmation.
#[derive(Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: String,
    pub node_id: String,
    pub state: State,
    pub fingerprint: Option<String>,
    pub expires: u64,
}

/// Immutable work handed to the privileged signer after the Enroller lock is released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignRequest {
    pub id: String,
    pub node_id: String,
    pub csr_pem: String,
}

#[derive(Default)]
pub struct Enroller {
    offers: HashMap<String, Offer>,
}

fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    for x in b.iter_mut() {
        *x = rand::random();
    }
    hex::encode(b)
}

fn hash(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn live(o: &Offer, now: u64) -> bool {
    matches!(o.state, State::Offered | State::Submitted) && now <= o.expires
}

fn leaf_fingerprint(certificate_pem: &str) -> Result<String, Error> {
    let (tail, certificate) = x509_parser::pem::parse_x509_pem(certificate_pem.as_bytes())
        .map_err(|_| Error::BadCertificate)?;
    if certificate.label != "CERTIFICATE" || !tail.iter().all(u8::is_ascii_whitespace) {
        return Err(Error::BadCertificate);
    }
    let (rest, _) = x509_parser::parse_x509_certificate(&certificate.contents)
        .map_err(|_| Error::BadCertificate)?;
    if !rest.is_empty() {
        return Err(Error::BadCertificate);
    }
    Ok(hex::encode(Sha256::digest(&certificate.contents)))
}

impl Enroller {
    fn sweep(&mut self, now: u64) {
        self.offers.retain(|_, o| now <= o.expires + OFFER_TTL_SECS);
    }

    /// Open an offer. One live offer per node id.
    pub fn offer(&mut self, node_id: &str, now: u64) -> Result<Issued, Error> {
        self.sweep(now);
        if !valid_node_id(node_id) {
            return Err(Error::BadNodeId);
        }
        if self.offers.values().filter(|o| live(o, now)).count() >= MAX_OPEN_OFFERS {
            return Err(Error::TooManyOffers);
        }
        if self
            .offers
            .values()
            .any(|o| o.node_id == node_id && live(o, now))
        {
            return Err(Error::NodeIdTaken);
        }
        let (id, token, nonce) = (rand_hex(8), rand_hex(16), rand_hex(16));
        let expires = now + OFFER_TTL_SECS;
        self.offers.insert(
            id.clone(),
            Offer {
                node_id: node_id.to_string(),
                token_hash: hash(&token),
                nonce: nonce.clone(),
                expires,
                failures: 0,
                state: State::Offered,
                csr: None,
                fingerprint: None,
                certificate: None,
                delivered_fingerprint: None,
                submit_digest: None,
            },
        );
        Ok(Issued {
            id,
            node_id: node_id.to_string(),
            token,
            nonce,
            expires,
        })
    }

    /// Submit a CSR after validating its signature, subject and key profile. The hub computes the
    /// canonical SHA-256 fingerprint of the CSR public key; no client-asserted fingerprint or
    /// unkeyed proof is accepted. A valid token is consumed even if CSR validation fails.
    pub fn submit(
        &mut self,
        id: &str,
        node_id: &str,
        token: &str,
        nonce: &str,
        csr_pem: &str,
        now: u64,
    ) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        if matches!(o.state, State::Submitted | State::Issued) {
            let digest: [u8; 32] = Sha256::digest(csr_pem.as_bytes()).into();
            return if o.expires >= now
                && ct_eq(&hash(token), &o.token_hash)
                && ct_eq(nonce.as_bytes(), o.nonce.as_bytes())
                && node_id == o.node_id
                && o.submit_digest
                    .is_some_and(|stored| ct_eq(&stored, &digest))
            {
                Ok(())
            } else {
                Err(Error::WrongState)
            };
        }
        if o.state != State::Offered {
            return Err(Error::WrongState);
        }
        if now > o.expires {
            o.state = State::Closed;
            return Err(Error::Expired);
        }
        if !(ct_eq(&hash(token), &o.token_hash)
            && ct_eq(nonce.as_bytes(), o.nonce.as_bytes())
            && node_id == o.node_id)
        {
            o.failures += 1;
            if o.failures >= MAX_FAILURES {
                o.state = State::Closed;
            }
            return Err(Error::BadToken);
        }
        // The token is single use from here: any later failure closes the offer.
        if csr_pem.len() > MAX_CSR_BYTES {
            o.state = State::Closed;
            return Err(Error::BadCsr);
        }
        let fingerprint =
            match crate::fleet_ca::validate_node_csr_with_challenge(node_id, csr_pem, nonce) {
                Ok(value) => value,
                Err(_) => {
                    o.state = State::Closed;
                    return Err(Error::BadCsr);
                }
            };
        o.csr = Some(csr_pem.to_string());
        o.submit_digest = Some(Sha256::digest(csr_pem.as_bytes()).into());
        o.fingerprint = Some(fingerprint);
        o.state = State::Submitted;
        Ok(())
    }

    /// Offers the operator can see (no token, no nonce, no CSR).
    /// Returns the current state and expiry for a known offer without exposing its secrets.
    pub fn state_and_expiry(&self, id: &str) -> Result<(State, u64), Error> {
        let offer = self.offers.get(id).ok_or(Error::Unknown)?;
        Ok((offer.state, offer.expires))
    }

    pub fn pending(&mut self, now: u64) -> Vec<Pending> {
        self.sweep(now);
        let mut v: Vec<Pending> = self
            .offers
            .iter()
            .map(|(id, o)| Pending {
                id: id.clone(),
                node_id: o.node_id.clone(),
                state: if now > o.expires
                    && matches!(
                        o.state,
                        State::Offered | State::Submitted | State::AwaitingReport
                    ) {
                    State::Closed
                } else {
                    o.state
                },
                fingerprint: o.fingerprint.clone(),
                expires: o.expires,
            })
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Atomically reserves a submitted offer for signing. The caller must release its mutex
    /// before invoking the privileged helper.
    pub fn begin_confirm(
        &mut self,
        id: &str,
        fingerprint: &str,
        now: u64,
    ) -> Result<SignRequest, Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        if o.state != State::Submitted {
            return Err(Error::WrongState);
        }
        if now > o.expires {
            o.state = State::Closed;
            return Err(Error::Expired);
        }
        let want = o.fingerprint.as_deref().unwrap_or("");
        let got = fingerprint.trim().to_ascii_lowercase();
        if !ct_eq(want.as_bytes(), got.as_bytes()) {
            return Err(Error::FingerprintMismatch);
        }
        let request = SignRequest {
            id: id.to_owned(),
            node_id: o.node_id.clone(),
            csr_pem: o.csr.clone().ok_or(Error::WrongState)?,
        };
        o.state = State::Signing;
        Ok(request)
    }

    /// Commits a signer result only if the reservation is still live. Deny/expiry wins over a
    /// late helper result, so an in-flight signature can never resurrect a closed offer.
    pub fn finish_confirm(
        &mut self,
        id: &str,
        now: u64,
        result: Result<String, Error>,
    ) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        if o.state != State::Signing {
            return Err(Error::WrongState);
        }
        if now > o.expires {
            o.state = State::Closed;
            o.csr = None;
            return Err(Error::Expired);
        }
        match result {
            Ok(certificate) => {
                o.certificate = Some(certificate);
                o.csr = None;
                o.state = State::Issued;
                Ok(())
            }
            Err(error) => {
                o.state = State::Closed;
                o.csr = None;
                o.certificate = None;
                Err(error)
            }
        }
    }

    /// Compatibility helper for callers that already own an exclusive Enroller borrow. Runtime
    /// API paths use begin/finish around the helper without holding the mutex.
    pub fn confirm(
        &mut self,
        id: &str,
        fingerprint: &str,
        now: u64,
        sign: impl FnOnce(&str, &str) -> Result<String, Error>,
    ) -> Result<(), Error> {
        let request = self.begin_confirm(id, fingerprint, now)?;
        let result = sign(&request.node_id, &request.csr_pem);
        self.finish_confirm(id, now, result)
    }

    /// The Join TLS session retrieves the public certificate; the operator API never receives it.
    pub fn certificate_for_delivery(&self, id: &str) -> Result<&str, Error> {
        let o = self.offers.get(id).ok_or(Error::Unknown)?;
        if o.state != State::Issued {
            return Err(Error::WrongState);
        }
        o.certificate.as_deref().ok_or(Error::WrongState)
    }

    /// Called only after the Join transport ACK confirms local persistence of the delivered bundle.
    /// The fingerprint is derived here from the exact issued leaf, never supplied by the caller.
    pub fn mark_delivered(&mut self, id: &str, now: u64) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        if o.state != State::Issued {
            return Err(Error::WrongState);
        }
        if now > o.expires {
            o.state = State::Closed;
            o.certificate = None;
            o.delivered_fingerprint = None;
            return Err(Error::Expired);
        }
        let certificate = o.certificate.as_deref().ok_or(Error::WrongState)?;
        let fingerprint = leaf_fingerprint(certificate)?;
        o.delivered_fingerprint = Some(fingerprint);
        o.state = State::AwaitingReport;
        Ok(())
    }

    /// Enrolled is reached only after the Hub accepts a mandatory-mTLS report from the exact
    /// certificate delivered for an offer. Both values are derived from the authenticated leaf.
    pub fn acknowledge_first_report(
        &mut self,
        node_id: &str,
        certificate_fingerprint: &str,
        now: u64,
    ) -> bool {
        if certificate_fingerprint.len() != 64 || hex::decode(certificate_fingerprint).is_err() {
            return false;
        }
        let got = certificate_fingerprint.to_ascii_lowercase();
        let Some(o) = self.offers.values_mut().find(|o| {
            o.state == State::AwaitingReport
                && o.node_id == node_id
                && o.delivered_fingerprint
                    .as_deref()
                    .is_some_and(|want| ct_eq(want.as_bytes(), got.as_bytes()))
        }) else {
            return false;
        };
        if now > o.expires {
            o.state = State::Closed;
            o.certificate = None;
            o.delivered_fingerprint = None;
            return false;
        }
        o.state = State::Enrolled;
        o.certificate = None;
        o.delivered_fingerprint = None;
        true
    }

    /// The operator cancels, or declines a CSR.
    pub fn deny(&mut self, id: &str) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        o.state = State::Closed;
        o.csr = None;
        o.certificate = None;
        o.delivered_fingerprint = None;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{Enroller, State};

    pub(crate) fn certificate_pem() -> String {
        let key = rcgen::KeyPair::generate().unwrap();
        rcgen::CertificateParams::new(Vec::new())
            .unwrap()
            .self_signed(&key)
            .unwrap()
            .pem()
    }

    pub(crate) fn certificate_fingerprint(pem: &str) -> String {
        use sha2::Digest;
        let (tail, certificate) = x509_parser::pem::parse_x509_pem(pem.as_bytes()).unwrap();
        assert!(tail.iter().all(u8::is_ascii_whitespace));
        hex::encode(sha2::Sha256::digest(&certificate.contents))
    }

    /// Prepare an offer with a valid public certificate at the pre-delivery boundary.
    pub(crate) fn issued_offer(enroller: &mut Enroller, node_id: &str, now: u64) -> String {
        let issued = enroller.offer(node_id, now).unwrap();
        let offer = enroller.offers.get_mut(&issued.id).unwrap();
        offer.state = State::Issued;
        offer.certificate = Some(certificate_pem());
        issued.id
    }

    /// Prepare a real Enroller instance at the post-persistence-ACK boundary without invoking
    /// OpenSSL CSR/signing fixtures; those boundaries have their own tests.
    pub(crate) fn awaiting_report(
        enroller: &mut Enroller,
        node_id: &str,
        now: u64,
    ) -> (String, String) {
        let issued = enroller.offer(node_id, now).unwrap();
        let offer = enroller.offers.get_mut(&issued.id).unwrap();
        offer.state = State::Issued;
        offer.certificate = Some(certificate_pem());
        let fingerprint = certificate_fingerprint(offer.certificate.as_deref().unwrap());
        enroller.mark_delivered(&issued.id, now + 1).unwrap();
        (issued.id, fingerprint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csr(node_id: &str, challenge: Option<&str>) -> String {
        use std::fs;
        let dir =
            std::env::temp_dir().join(format!("fleet-enroll-csr-{:x}", rand::random::<u64>()));
        fs::create_dir_all(&dir).unwrap();
        let key = crate::pair_ca::run(
            &[
                "genpkey",
                "-algorithm",
                "EC",
                "-pkeyopt",
                "ec_paramgen_curve:prime256v1",
            ],
            None,
        )
        .unwrap();
        let key_path = dir.join("node.key");
        crate::pair_ca::write_private(&key_path, &key, 0o600).unwrap();
        let config_path = dir.join("request.cnf");
        let mut config = String::from("[ req ]\nprompt = no\ndistinguished_name = dn\n");
        if challenge.is_some() {
            config.push_str("attributes = req_attributes\n");
        }
        config.push_str(&format!("[ dn ]\nCN = {node_id}\n"));
        if let Some(challenge) = challenge {
            config.push_str(&format!(
                "[ req_attributes ]\nchallengePassword = {challenge}\n"
            ));
        }
        fs::write(&config_path, config).unwrap();
        let csr = crate::pair_ca::run(
            &[
                "req",
                "-new",
                "-key",
                &key_path.to_string_lossy(),
                "-config",
                &config_path.to_string_lossy(),
            ],
            None,
        )
        .unwrap();
        let _ = fs::remove_dir_all(dir);
        String::from_utf8(csr).unwrap()
    }

    fn submit_ok(e: &mut Enroller, i: &Issued, now: u64) -> Result<(), Error> {
        let csr = csr(&i.node_id, Some(&i.nonce));
        e.submit(&i.id, &i.node_id, &i.token, &i.nonce, &csr, now)
    }

    fn ok_sign(_: &str, _: &str) -> Result<String, Error> {
        Ok("CERT".to_owned())
    }

    #[test]
    fn signs_only_after_fingerprint_confirmation() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 100).unwrap();
        submit_ok(&mut e, &i, 110).unwrap();
        let fp = e.pending(111)[0].fingerprint.clone().unwrap();
        assert_eq!(fp.len(), 64);
        assert_eq!(
            e.confirm(&i.id, "0000-0000-0000", 120, ok_sign),
            Err(Error::FingerprintMismatch)
        );
        let uppercase_fp = fp.to_ascii_uppercase();
        assert_eq!(e.confirm(&i.id, &uppercase_fp, 121, ok_sign), Ok(()));
        assert_eq!(e.pending(121)[0].state, State::Issued);
        assert_eq!(e.certificate_for_delivery(&i.id), Ok("CERT"));
        assert_eq!(e.confirm(&i.id, &fp, 122, ok_sign), Err(Error::WrongState));
    }

    #[test]
    fn submit_uses_fingerprint_from_cryptographically_validated_csr() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 0).unwrap();
        let request = csr("edge-1", Some(&i.nonce));
        let expected = crate::fleet_ca::validate_node_csr("edge-1", &request).unwrap();
        e.submit(&i.id, &i.node_id, &i.token, &i.nonce, &request, 1)
            .unwrap();
        assert_eq!(
            e.pending(2)[0].fingerprint.as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn fleet_csr_requires_exact_offer_nonce_challenge_password() {
        let mut e = Enroller::default();
        let missing_offer = e.offer("edge-1", 100).unwrap();
        let missing = csr("edge-1", None);
        let missing_result = e.submit(
            &missing_offer.id,
            &missing_offer.node_id,
            &missing_offer.token,
            &missing_offer.nonce,
            &missing,
            101,
        );

        let wrong_offer = e.offer("edge-2", 100).unwrap();
        let wrong = csr("edge-2", Some("ffffffffffffffffffffffffffffffff"));
        let wrong_result = e.submit(
            &wrong_offer.id,
            &wrong_offer.node_id,
            &wrong_offer.token,
            &wrong_offer.nonce,
            &wrong,
            101,
        );

        let valid_offer = e.offer("edge-3", 100).unwrap();
        let valid = csr("edge-3", Some(&valid_offer.nonce));
        let valid_result = e.submit(
            &valid_offer.id,
            &valid_offer.node_id,
            &valid_offer.token,
            &valid_offer.nonce,
            &valid,
            101,
        );

        assert_eq!(
            missing_result,
            Err(Error::BadCsr),
            "missing challengePassword must be rejected"
        );
        assert_eq!(
            wrong_result,
            Err(Error::BadCsr),
            "wrong challengePassword must be rejected"
        );
        assert_eq!(
            valid_result,
            Ok(()),
            "the exact offer nonce must be accepted"
        );
    }

    #[test]
    fn finish_confirm_rejects_a_late_result_after_deny_or_expiry() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-1", 100).unwrap();
        let request = csr("edge-1", Some(&issued.nonce));
        e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &request,
            101,
        )
        .unwrap();
        let fingerprint = e
            .pending(101)
            .iter()
            .find(|p| p.id == issued.id)
            .unwrap()
            .fingerprint
            .clone()
            .unwrap();
        e.begin_confirm(&issued.id, &fingerprint, 102).unwrap();
        e.deny(&issued.id).unwrap();
        assert_eq!(
            e.finish_confirm(&issued.id, 103, Ok("CERT".into())),
            Err(Error::WrongState)
        );

        let second = e.offer("edge-2", 200).unwrap();
        let csr = csr("edge-2", Some(&second.nonce));
        e.submit(
            &second.id,
            &second.node_id,
            &second.token,
            &second.nonce,
            &csr,
            201,
        )
        .unwrap();
        let fingerprint = e
            .pending(201)
            .iter()
            .find(|p| p.id == second.id)
            .unwrap()
            .fingerprint
            .clone()
            .unwrap();
        e.begin_confirm(&second.id, &fingerprint, 202).unwrap();
        assert_eq!(
            e.finish_confirm(&second.id, second.expires + 1, Ok("CERT".into())),
            Err(Error::Expired)
        );
    }

    #[test]
    fn enroller_submit_allows_only_exact_idempotent_retry_after_first_submit() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-1", 100).unwrap();
        let original = csr("edge-1", Some(&issued.nonce));
        e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &original,
            101,
        )
        .unwrap();
        let exact_retry = e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &original,
            102,
        );
        let changed_csr = csr("edge-1", Some(&issued.nonce));
        let changed_retry = e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &changed_csr,
            103,
        );
        assert_eq!(
            exact_retry,
            Ok(()),
            "the exact original request must be idempotent"
        );
        assert_eq!(
            changed_retry,
            Err(Error::WrongState),
            "a changed CSR must be rejected"
        );
        assert_eq!(
            e.pending(103)
                .iter()
                .find(|p| p.id == issued.id)
                .unwrap()
                .state,
            State::Submitted
        );
    }

    #[test]
    fn enroller_submit_allows_exact_retry_after_operator_issuance() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-1", 100).unwrap();
        let original = csr("edge-1", Some(&issued.nonce));
        e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &original,
            101,
        )
        .unwrap();
        let fingerprint = e.pending(102)[0].fingerprint.clone().unwrap();
        e.confirm(&issued.id, &fingerprint, 103, ok_sign).unwrap();
        assert_eq!(
            e.submit(
                &issued.id,
                &issued.node_id,
                &issued.token,
                &issued.nonce,
                &original,
                104
            ),
            Ok(()),
            "the exact CSR retry remains idempotent after operator issuance"
        );
        let changed = csr("edge-1", Some(&issued.nonce));
        assert_eq!(
            e.submit(
                &issued.id,
                &issued.node_id,
                &issued.token,
                &issued.nonce,
                &changed,
                105
            ),
            Err(Error::WrongState)
        );
    }

    #[test]
    fn invalid_csr_validation_consumes_the_one_time_offer_and_closes_it() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 0).unwrap();
        assert_eq!(
            e.submit(&i.id, &i.node_id, &i.token, &i.nonce, "bad-csr", 1),
            Err(Error::BadCsr)
        );
        assert_eq!(e.pending(2)[0].state, State::Closed);
    }

    #[test]
    fn token_is_single_use_and_wrong_token_closes_after_three() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 0).unwrap();
        submit_ok(&mut e, &i, 1).unwrap();
        assert_eq!(submit_ok(&mut e, &i, 2), Err(Error::WrongState));
        let j = e.offer("edge-2", 0).unwrap();
        for _ in 0..3 {
            assert_eq!(
                e.submit(&j.id, "edge-2", "bad", &j.nonce, "CSR", 1),
                Err(Error::BadToken)
            );
        }
        assert_eq!(submit_ok(&mut e, &j, 2), Err(Error::WrongState));
    }

    #[test]
    fn wrong_node_id_invalid_csr_and_expiry_are_refused() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 0).unwrap();
        assert_eq!(
            e.submit(&i.id, "edge-1", &i.token, "wrong-nonce", "CSR", 1),
            Err(Error::BadToken)
        );
        assert_eq!(
            e.submit(&i.id, "edge-9", &i.token, &i.nonce, "CSR", 1),
            Err(Error::BadToken)
        );
        assert_eq!(
            e.submit(&i.id, "edge-1", &i.token, &i.nonce, "not a CSR", 1),
            Err(Error::BadCsr)
        );
        let k = e.offer("edge-3", 0).unwrap();
        assert_eq!(
            submit_ok(&mut e, &k, OFFER_TTL_SECS + 1),
            Err(Error::Expired)
        );
    }

    #[test]
    fn invalid_ids_duplicates_and_limits() {
        let mut e = Enroller::default();
        assert!(matches!(e.offer("Bad_ID", 0), Err(Error::BadNodeId)));
        e.offer("edge-1", 0).unwrap();
        assert!(matches!(e.offer("edge-1", 1), Err(Error::NodeIdTaken)));
        for n in 0..MAX_OPEN_OFFERS - 1 {
            e.offer(&format!("n{n}"), 1).unwrap();
        }
        assert!(matches!(e.offer("extra", 2), Err(Error::TooManyOffers)));
    }

    #[test]
    fn signing_is_not_enrollment_until_delivery_and_first_report_ack() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 100).unwrap();
        submit_ok(&mut e, &i, 110).unwrap();

        let fp = e.pending(111)[0].fingerprint.clone().unwrap();
        let certificate = test_support::certificate_pem();
        e.confirm(&i.id, &fp, 120, |_, _| Ok(certificate.clone()))
            .unwrap();
        assert_eq!(e.pending(121)[0].state, State::Issued);
        assert_eq!(e.certificate_for_delivery(&i.id).unwrap(), certificate);
        assert_eq!(e.pending(121)[0].state, State::Issued);
        let delivered = test_support::certificate_fingerprint(&certificate);
        assert!(!e.acknowledge_first_report("edge-1", &delivered, 121));

        e.mark_delivered(&i.id, 122).unwrap();
        assert_eq!(e.certificate_for_delivery(&i.id), Err(Error::WrongState));
        assert_eq!(e.pending(122)[0].state, State::AwaitingReport);
        assert!(!e.acknowledge_first_report("edge-2", &delivered, 122));
        assert_eq!(e.pending(122)[0].state, State::AwaitingReport);
        assert!(e.acknowledge_first_report("edge-1", &delivered, 123));
        assert_eq!(e.pending(123)[0].state, State::Enrolled);
        assert_eq!(e.certificate_for_delivery(&i.id), Err(Error::WrongState));
    }

    #[test]
    fn exact_join_retry_after_ack_redelivers_the_same_certificate() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-retry", 100).unwrap();
        let request_csr = csr(&issued.node_id, Some(&issued.nonce));
        e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &request_csr,
            101,
        )
        .unwrap();
        let fingerprint = e.pending(102)[0].fingerprint.clone().unwrap();
        let certificate = test_support::certificate_pem();
        e.confirm(
            &issued.id,
            &fingerprint,
            103,
            |_, _| Ok(certificate.clone()),
        )
        .unwrap();
        e.mark_delivered(&issued.id, 104).unwrap();

        assert_eq!(
            e.submit(
                &issued.id,
                &issued.node_id,
                &issued.token,
                &issued.nonce,
                &request_csr,
                105,
            ),
            Ok(())
        );
        assert_eq!(
            e.certificate_for_delivery(&issued.id),
            Ok(certificate.as_str())
        );
        assert_eq!(e.mark_delivered(&issued.id, 105), Ok(()));
        assert_eq!(
            e.state_and_expiry(&issued.id).unwrap().0,
            State::AwaitingReport
        );
    }

    #[test]
    fn awaiting_report_retry_rejects_a_changed_csr_or_credential() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-retry-guard", 100).unwrap();
        let request_csr = csr(&issued.node_id, Some(&issued.nonce));
        e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &request_csr,
            101,
        )
        .unwrap();
        let fingerprint = e.pending(102)[0].fingerprint.clone().unwrap();
        let certificate = test_support::certificate_pem();
        e.confirm(
            &issued.id,
            &fingerprint,
            103,
            |_, _| Ok(certificate.clone()),
        )
        .unwrap();
        e.mark_delivered(&issued.id, 104).unwrap();

        let changed_csr = csr(&issued.node_id, Some(&issued.nonce));
        assert_eq!(
            e.submit(
                &issued.id,
                &issued.node_id,
                &issued.token,
                &issued.nonce,
                &changed_csr,
                105,
            ),
            Err(Error::WrongState)
        );
        assert_eq!(
            e.submit(
                &issued.id,
                &issued.node_id,
                &"00".repeat(16),
                &issued.nonce,
                &request_csr,
                105,
            ),
            Err(Error::WrongState)
        );
    }

    #[test]
    fn expired_awaiting_report_offer_cannot_be_retried_for_delivery() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-retry-expired", 100).unwrap();
        let request_csr = csr(&issued.node_id, Some(&issued.nonce));
        e.submit(
            &issued.id,
            &issued.node_id,
            &issued.token,
            &issued.nonce,
            &request_csr,
            101,
        )
        .unwrap();
        let fingerprint = e.pending(102)[0].fingerprint.clone().unwrap();
        let certificate = test_support::certificate_pem();
        e.confirm(
            &issued.id,
            &fingerprint,
            103,
            |_, _| Ok(certificate.clone()),
        )
        .unwrap();
        e.mark_delivered(&issued.id, 104).unwrap();

        assert_eq!(
            e.submit(
                &issued.id,
                &issued.node_id,
                &issued.token,
                &issued.nonce,
                &request_csr,
                issued.expires + 1,
            ),
            Err(Error::Expired)
        );
        assert_eq!(e.state_and_expiry(&issued.id).unwrap().0, State::Closed);
    }

    #[test]
    fn first_report_must_match_node_and_exact_delivered_certificate() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 100).unwrap();
        // Isolate the state transition from OpenSSL CSR fixtures; submit/confirm have separate tests.
        let offer = e.offers.get_mut(&i.id).unwrap();
        offer.state = State::Issued;
        let certificate = test_support::certificate_pem();
        offer.certificate = Some(certificate.clone());

        let delivered = test_support::certificate_fingerprint(&certificate);
        e.mark_delivered(&i.id, 101).unwrap();
        assert_eq!(e.pending(121)[0].state, State::AwaitingReport);
        assert!(!e.acknowledge_first_report("edge-1", &"b".repeat(64), 122));
        assert!(!e.acknowledge_first_report("edge-2", &delivered, 122));
        assert_eq!(e.pending(122)[0].state, State::AwaitingReport);
        assert!(e.acknowledge_first_report("edge-1", &delivered, 123));
        assert_eq!(e.pending(123)[0].state, State::Enrolled);
        assert!(!e.acknowledge_first_report("edge-1", &delivered, 123));
    }

    #[test]
    fn expired_offer_cannot_be_marked_delivered() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-1", 100).unwrap();
        let offer = e.offers.get_mut(&issued.id).unwrap();
        offer.state = State::Issued;
        offer.certificate = Some(test_support::certificate_pem());

        assert_eq!(e.mark_delivered(&issued.id, 1001), Err(Error::Expired));
        assert_eq!(e.pending(1001)[0].state, State::Closed);
        assert!(e.offers[&issued.id].certificate.is_none());
    }

    #[test]
    fn expired_awaiting_report_offer_cannot_become_enrolled() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 100).unwrap();
        let offer = e.offers.get_mut(&i.id).unwrap();
        offer.state = State::Issued;
        let certificate = test_support::certificate_pem();
        offer.certificate = Some(certificate.clone());
        let delivered = test_support::certificate_fingerprint(&certificate);
        e.mark_delivered(&i.id, 101).unwrap();

        assert!(!e.acknowledge_first_report("edge-1", &delivered, 1001));
        assert_eq!(e.pending(1001)[0].state, State::Closed);
    }

    #[test]
    fn mark_delivered_derives_fingerprint_from_the_issued_certificate() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-1", 100).unwrap();
        let offer = e.offers.get_mut(&issued.id).unwrap();
        offer.state = State::Issued;
        offer.certificate = Some(test_support::certificate_pem());
        let expected = test_support::certificate_fingerprint(offer.certificate.as_deref().unwrap());

        e.mark_delivered(&issued.id, 101).unwrap();
        assert_eq!(
            e.offers[&issued.id].delivered_fingerprint.as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn mark_delivered_rejects_an_invalid_issued_certificate() {
        let mut e = Enroller::default();
        let issued = e.offer("edge-1", 100).unwrap();
        let offer = e.offers.get_mut(&issued.id).unwrap();
        offer.state = State::Issued;
        offer.certificate = Some("not a certificate".to_owned());
        assert_eq!(
            e.mark_delivered(&issued.id, 101),
            Err(Error::BadCertificate)
        );
        assert_eq!(e.pending(101)[0].state, State::Issued);
    }

    #[test]
    fn denying_awaiting_report_offer_clears_delivered_certificate_binding() {
        let mut e = Enroller::default();
        let (id, _fingerprint) = test_support::awaiting_report(&mut e, "edge-1", 100);
        e.deny(&id).unwrap();
        assert!(e.offers[&id].delivered_fingerprint.is_none());
    }

    #[test]
    fn deny_drops_the_csr_and_signer_failure_closes() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 0).unwrap();
        submit_ok(&mut e, &i, 1).unwrap();
        let i_fp = e.pending(1)[0].fingerprint.clone().unwrap();
        e.deny(&i.id).unwrap();
        assert_eq!(e.confirm(&i.id, &i_fp, 2, ok_sign), Err(Error::WrongState));
        let j = e.offer("edge-1", 3).unwrap();
        submit_ok(&mut e, &j, 4).unwrap();
        let j_fp = e
            .pending(4)
            .into_iter()
            .find(|pending| pending.id == j.id)
            .and_then(|pending| pending.fingerprint)
            .unwrap();
        assert_eq!(
            e.confirm(&j.id, &j_fp, 5, |_, _| Err(Error::Sign)),
            Err(Error::Sign)
        );
        assert_eq!(
            e.pending(6).iter().find(|p| p.id == j.id).unwrap().state,
            State::Closed
        );
    }
}
