//! ADR-0045 F-7 / ADR-0051 Path A: the hub-side enrollment state machine for a new fleet node.
//!
//! The operator creates an offer (node id + one-time token + nonce, 15 minute TTL). The new
//! server submits a CSR with a proof bound to the nonce. The CSR does not become a certificate
//! until the operator confirms the CSR fingerprint, which the new server also prints, so a
//! swapped CSR or bundle is visible (F-7). A wrong token or node id counts against a small
//! failure budget and then closes the window; a valid token is consumed by the first submit.
//! The module is pure: signing and fingerprinting are injected, so it holds no keys and does no
//! I/O. The transport and the root helper wiring come in later slices.

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
    BadProof,
    BadCsr,
    WrongState,
    FingerprintMismatch,
    Sign,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum State {
    Offered,
    Submitted,
    Done,
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

/// Proof the new server computes from the offer nonce and its own CSR fingerprint.
pub fn proof(nonce: &str, fingerprint: &str) -> String {
    hex::encode(Sha256::digest(format!(
        "baft-fleet-enroll-v1|{nonce}|{fingerprint}"
    )))
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

    /// The new server submits its CSR. `fingerprint` is computed by the caller from the CSR.
    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &mut self,
        id: &str,
        node_id: &str,
        token: &str,
        csr_pem: &str,
        fingerprint: &str,
        client_proof: &str,
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
        if !(ct_eq(&hash(token), &o.token_hash) && node_id == o.node_id) {
            o.failures += 1;
            if o.failures >= MAX_FAILURES {
                o.state = State::Closed;
            }
            return Err(Error::BadToken);
        }
        // The token is single use from here: any later failure closes the offer.
        if csr_pem.len() > MAX_CSR_BYTES || fingerprint.len() > 32 {
            o.state = State::Closed;
            return Err(Error::BadCsr);
        }
        if !ct_eq(
            proof(&o.nonce, fingerprint).as_bytes(),
            client_proof.as_bytes(),
        ) {
            o.state = State::Closed;
            return Err(Error::BadProof);
        }
        o.csr = Some(csr_pem.to_string());
        o.fingerprint = Some(fingerprint.to_string());
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
                state: if now > o.expires && o.state != State::Done {
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
    ) -> Result<String, Error> {
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
        let res = sign(&o.node_id, &csr);
        o.state = if res.is_ok() {
            State::Done
        } else {
            State::Closed
        };
        res
    }

    /// The operator cancels, or declines a CSR.
    pub fn deny(&mut self, id: &str) -> Result<(), Error> {
        let o = self.offers.get_mut(id).ok_or(Error::Unknown)?;
        o.state = State::Closed;
        o.csr = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "ab12-cd34-ef56";

    fn submit_ok(e: &mut Enroller, i: &Issued, now: u64) -> Result<(), Error> {
        e.submit(
            &i.id,
            &i.node_id,
            &i.token,
            "CSR",
            FP,
            &proof(&i.nonce, FP),
            now,
        )
    }

    fn ok_sign(_: &str, csr: &str) -> Result<String, Error> {
        Ok(format!("CERT({csr})"))
    }

    #[test]
    fn signs_only_after_fingerprint_confirmation() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 100).unwrap();
        submit_ok(&mut e, &i, 110).unwrap();
        assert_eq!(e.pending(111)[0].fingerprint.as_deref(), Some(FP));
        assert_eq!(
            e.confirm(&i.id, "0000-0000-0000", 120, ok_sign),
            Err(Error::FingerprintMismatch)
        );
        assert_eq!(
            e.confirm(&i.id, "AB12-CD34-EF56", 121, ok_sign).unwrap(),
            "CERT(CSR)"
        );
        assert_eq!(e.confirm(&i.id, FP, 122, ok_sign), Err(Error::WrongState));
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
                e.submit(&j.id, "edge-2", "bad", "CSR", "fp", "x", 1),
                Err(Error::BadToken)
            );
        }
        assert_eq!(submit_ok(&mut e, &j, 2), Err(Error::WrongState));
    }

    #[test]
    fn wrong_node_id_bad_proof_and_expiry_are_refused() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 0).unwrap();
        assert_eq!(
            e.submit(&i.id, "edge-9", &i.token, "CSR", "fp", "x", 1),
            Err(Error::BadToken)
        );
        assert_eq!(
            e.submit(&i.id, "edge-1", &i.token, "CSR", "fp", "wrong", 1),
            Err(Error::BadProof)
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
    fn deny_drops_the_csr_and_signer_failure_closes() {
        let mut e = Enroller::default();
        let i = e.offer("edge-1", 0).unwrap();
        submit_ok(&mut e, &i, 1).unwrap();
        e.deny(&i.id).unwrap();
        assert_eq!(e.confirm(&i.id, FP, 2, ok_sign), Err(Error::WrongState));
        let j = e.offer("edge-1", 3).unwrap();
        submit_ok(&mut e, &j, 4).unwrap();
        assert_eq!(
            e.confirm(&j.id, FP, 5, |_, _| Err(Error::Sign)),
            Err(Error::Sign)
        );
        assert_eq!(
            e.pending(6).iter().find(|p| p.id == j.id).unwrap().state,
            State::Closed
        );
    }
}
