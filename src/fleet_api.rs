//! Operator API for Fleet enrollment (ADR-0051 Path A), pure over `fleet_enroll::Enroller`.
//!
//! `POST /v1/fleet/enroll/offer {"node_id"}` opens an offer and returns the one-time secrets
//! once; `GET /v1/fleet/enroll` lists offers (never a token, nonce or CSR); `POST
//! /v1/fleet/enroll/confirm {"id","fingerprint"}` signs only after the operator types the CSR
//! fingerprint; `POST /v1/fleet/enroll/deny {"id"}` cancels. Signing is injected: the key lives
//! with the root helper (M5), so without a signer `confirm` answers 503 and signs nothing.

#![allow(dead_code)] // routed from agent.rs once the Add-server slice lands

use serde::Deserialize;
use serde_json::{Value, json};

use crate::fleet_enroll::{Enroller, Error, State};

pub type Signer<'a> = dyn Fn(&str, &str) -> Result<String, Error> + 'a;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferReq {
    node_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmReq {
    id: String,
    fingerprint: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DenyReq {
    id: String,
}

fn state_name(s: State) -> &'static str {
    match s {
        State::Offered => "waiting-for-node",
        State::Submitted => "needs-confirmation",
        State::Issued => "issued-unconfirmed",
        State::AwaitingReport => "awaiting-first-report",
        State::Enrolled => "enrolled",
        State::Closed => "closed",
    }
}

fn fail(e: Error) -> (u16, Value) {
    let (code, msg) = match e {
        Error::BadNodeId => (
            400,
            "node id must be 1-63 lowercase letters, digits or hyphens",
        ),
        Error::TooManyOffers => (429, "too many open offers"),
        Error::NodeIdTaken => (409, "an offer for this node id is already open"),
        Error::Unknown => (404, "unknown offer"),
        Error::Expired => (410, "offer expired"),
        Error::WrongState => (409, "offer is not in a state that allows this"),
        Error::FingerprintMismatch => (422, "fingerprint does not match the one the node reported"),
        Error::Sign => (503, "signing is not available on this host"),
        Error::BadToken | Error::BadCsr => (400, "invalid enrollment request"),
        Error::WrongNode => (409, "first report identity does not match the offered node"),
    };
    (code, json!({"error": msg}))
}

pub fn offer(e: &mut Enroller, body: &[u8], now: u64) -> (u16, Value) {
    let Ok(req) = serde_json::from_slice::<OfferReq>(body) else {
        return (400, json!({"error": "invalid request"}));
    };
    match e.offer(&req.node_id, now) {
        Ok(i) => (
            200,
            json!({
                "id": i.id, "node_id": i.node_id, "token": i.token, "nonce": i.nonce,
                "expires": i.expires,
                "note": "shown once; give it to the new server only"
            }),
        ),
        Err(err) => fail(err),
    }
}

pub fn list(e: &mut Enroller, now: u64) -> (u16, Value) {
    let offers: Vec<Value> = e
        .pending(now)
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id, "node_id": p.node_id, "state": state_name(p.state),
                "fingerprint": p.fingerprint, "expires": p.expires
            })
        })
        .collect();
    (200, json!({ "offers": offers }))
}

/// `sign` is `None` when no signer is configured; the offer then stays untouched.
pub fn confirm(e: &mut Enroller, body: &[u8], now: u64, sign: Option<&Signer>) -> (u16, Value) {
    let Ok(req) = serde_json::from_slice::<ConfirmReq>(body) else {
        return (400, json!({"error": "invalid request"}));
    };
    let Some(sign) = sign else {
        return fail(Error::Sign);
    };
    match e.confirm(&req.id, &req.fingerprint, now, sign) {
        Ok(()) => (202, json!({"state": "issued-unconfirmed"})),
        Err(err) => fail(err),
    }
}

pub fn deny(e: &mut Enroller, body: &[u8]) -> (u16, Value) {
    let Ok(req) = serde_json::from_slice::<DenyReq>(body) else {
        return (400, json!({"error": "invalid request"}));
    };
    match e.deny(&req.id) {
        Ok(()) => (200, json!({"state": "closed"})),
        Err(err) => fail(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_list_confirm_flow_never_leaks_secrets_in_the_list() {
        let mut e = Enroller::default();
        let (c, v) = offer(&mut e, br#"{"node_id":"edge-1"}"#, 10);
        assert_eq!(c, 200);
        let (id, token, nonce) = (
            v["id"].as_str().unwrap().to_string(),
            v["token"].as_str().unwrap().to_string(),
            v["nonce"].as_str().unwrap().to_string(),
        );
        let listed = list(&mut e, 11).1.to_string();
        assert!(!listed.contains(&token) && !listed.contains(&nonce));
        let fp = "ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34ab12cd34";
        e.submit(&id, "edge-1", &token, "CSR", 12, |csr, node| {
            assert_eq!(csr, "CSR");
            assert_eq!(node, "edge-1");
            Ok(fp.to_owned())
        })
        .unwrap();
        assert_eq!(
            list(&mut e, 13).1["offers"][0]["state"],
            "needs-confirmation"
        );
        let body = format!(r#"{{"id":"{id}","fingerprint":"{fp}"}}"#);
        // No signer: refused and nothing consumed.
        assert_eq!(confirm(&mut e, body.as_bytes(), 14, None).0, 503);
        let signer = |_: &str, csr: &str| Ok(format!("CERT({csr})"));
        let (c, v) = confirm(&mut e, body.as_bytes(), 15, Some(&signer));
        assert_eq!(c, 202);
        assert_eq!(v["state"], "issued-unconfirmed");
        assert!(v.get("certificate").is_none());
        assert_eq!(e.certificate_for_delivery(&id), Ok("CERT(CSR)"));
    }

    #[test]
    fn bad_input_maps_to_clean_errors() {
        let mut e = Enroller::default();
        assert_eq!(offer(&mut e, b"{", 0).0, 400);
        assert_eq!(offer(&mut e, br#"{"node_id":"X y"}"#, 0).0, 400);
        assert_eq!(offer(&mut e, br#"{"node_id":"a","x":1}"#, 0).0, 400);
        offer(&mut e, br#"{"node_id":"a"}"#, 0);
        assert_eq!(offer(&mut e, br#"{"node_id":"a"}"#, 1).0, 409);
        assert_eq!(deny(&mut e, br#"{"id":"nope"}"#).0, 404);
    }
}
