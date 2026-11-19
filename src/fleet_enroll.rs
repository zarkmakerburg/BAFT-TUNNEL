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
    Sign,
    WrongNode,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum State {
    Offered,
    Submitted,
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
        let fingerprint = match crate::fleet_ca::validate_node_csr(node_id, csr_pem) {
            Ok(value) => value,
            Err(_) => {
                o.state = State::Closed;
                return Err(Error::BadCsr);
            }
        };
        o.csr = Some(csr_pem.to_string());
        o.fingerprint = Some(fingerprint);
        o.state = State::Submitted;
        Ok(())
    }

    /// Offers the operator can see (no token, no nonce, no CSR).
    pub fn pending(&mut self, now: u64) -> Vec<Pending> {
        self.sweep(now);
        let mut v: Vec<Pending> = self
            .offers
            .iter()
            .map(|(id, o)| Pending {
                id: id.clone(),
                node_id: o.node_id.clone(),
                state: if now > o.expires && matches!(o.state, State::Offered | State::Submitted) {
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

    /// The operator confirms the fingerprint it compared out of band; `sign(node_id, csr)` then
    /// issues the certificate. A mismatch is an operator typo and may be retried until expiry.
    pub fn confirm(
        &mut self,
        id: &str,
        fingerprint: &str,
        now: u64,
        sign: impl FnOnce(&str, &str) -> Result<String, Error>,
    ) -> Result<(), Error> {
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
        let csr = o.csr.take().unwrap_or_default();
        match sign(&o.node_id, &csr) {
            Ok(certificate) => {
                o.certificate = Some(certificate);
                o.state = State::Issued;
                Ok(())
            }
            Err(error) => {
                o.state = State::Closed;
                o.certificate = None;
                Err(error)
            }
        }
    }

    /// The Join TLS session retrieves the public certificate; the operator API never receives it.
    pub fn certificate_for_delivery(&self, id: &str) -> Result<&str, Error> {
        let o = self.offers.get(id).ok_or(Error::Unknown)?;
        if o.state != State::Issued {
            return Err(Error::WrongState);
        }
        o.certificate.as_deref().ok_or(Error::WrongState)
    }

    /// Called only after the certificate and public trust bundle were written to the pinned Join session.
    pub fn mark_delivered(&mut self, id: &str) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        if o.state != State::Issued || o.certificate.is_none() {
            return Err(Error::WrongState);
        }
        o.state = State::AwaitingReport;
        Ok(())
    }

    /// Enrolled is reached only after the first mandatory-mTLS report has been accepted and the
    /// Join session can return its acknowledgement to the same member identity.
    pub fn acknowledge_first_report(&mut self, id: &str, node_id: &str) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        if o.state != State::AwaitingReport {
            return Err(Error::WrongState);
        }
        if o.node_id != node_id {
            return Err(Error::WrongNode);
        }
        o.state = State::Enrolled;
        o.certificate = None;
        Ok(())
    }

    /// The operator cancels, or declines a CSR.
    pub fn deny(&mut self, id: &str) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        o.state = State::Closed;
        o.csr = None;
        o.certificate = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csr(node_id: &str) -> String {
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
        let csr = crate::pair_ca::run(
            &[
                "req",
                "-new",
                "-key",
                &key_path.to_string_lossy(),
                "-subj",
                &format!("/CN={node_id}"),
            ],
            None,
        )
        .unwrap();
        let _ = fs::remove_dir_all(dir);
        String::from_utf8(csr).unwrap()
    }

    fn submit_ok(e: &mut Enroller, i: &Issued, now: u64) -> Result<(), Error> {
        let csr = csr(&i.node_id);
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
        let request = csr("edge-1");
        let expected = crate::fleet_ca::validate_node_csr("edge-1", &request).unwrap();
        e.submit(&i.id, &i.node_id, &i.token, &i.nonce, &request, 1)
            .unwrap();
        assert_eq!(
            e.pending(2)[0].fingerprint.as_deref(),
            Some(expected.as_str())
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
        e.confirm(&i.id, &fp, 120, ok_sign).unwrap();
        assert_eq!(e.pending(121)[0].state, State::Issued);
        assert_eq!(e.certificate_for_delivery(&i.id).unwrap(), "CERT");
        assert_eq!(e.pending(121)[0].state, State::Issued);
        assert_eq!(
            e.acknowledge_first_report(&i.id, "edge-1"),
            Err(Error::WrongState)
        );

        e.mark_delivered(&i.id).unwrap();
        assert_eq!(e.certificate_for_delivery(&i.id), Err(Error::WrongState));
        assert_eq!(e.pending(122)[0].state, State::AwaitingReport);
        assert_eq!(
            e.acknowledge_first_report(&i.id, "edge-2"),
            Err(Error::WrongNode)
        );
        assert_eq!(e.pending(122)[0].state, State::AwaitingReport);
        e.acknowledge_first_report(&i.id, "edge-1").unwrap();
        assert_eq!(e.pending(123)[0].state, State::Enrolled);
        assert_eq!(e.certificate_for_delivery(&i.id), Err(Error::WrongState));
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
