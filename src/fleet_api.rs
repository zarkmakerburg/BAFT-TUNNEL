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
use std::sync::Mutex;

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
        State::Signing => "signing",
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
        Error::BadCertificate => (400, "issued certificate is invalid"),
        Error::Sign => (503, "signing is not available on this host"),
        Error::BadToken | Error::BadCsr => (400, "invalid enrollment request"),
        Error::WrongNode => (409, "first report identity does not match the offered node"),
    };
    (code, json!({"error": msg}))
}

pub fn offer(
    e: &mut Enroller,
    body: &[u8],
    now: u64,
    join: Option<&crate::fleet_run::JoinOfferContext>,
) -> (u16, Value) {
    let Some(join) = join else {
        return (
            503,
            json!({"error": "Fleet Join listener is not configured"}),
        );
    };
    let Ok(req) = serde_json::from_slice::<OfferReq>(body) else {
        return (400, json!({"error": "invalid request"}));
    };
    match e.offer(&req.node_id, now) {
        Ok(i) => {
            let offer = crate::fleet_join::JoinOffer {
                v: 1,
                host: join.host.clone(),
                port: join.port,
                hub_name: join.hub_name.clone(),
                hub_id: join.hub_id.clone(),
                hub_pin: join.hub_pin.clone(),
                offer_id: i.id.clone(),
                node_id: i.node_id.clone(),
                token: i.token,
                nonce: i.nonce,
                expires: i.expires,
            };
            match offer.encode() {
                Ok(code) => (
                    200,
                    json!({
                        "id": i.id,
                        "node_id": i.node_id,
                        "join_offer": code,
                        "expires": i.expires,
                        "note": "shown once; give it to the new server only"
                    }),
                ),
                Err(_) => {
                    let _ = e.deny(&i.id);
                    (500, json!({"error": "could not encode Fleet Join offer"}))
                }
            }
        }
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

/// Confirm without holding the shared Enroller mutex across the privileged helper call.
pub fn confirm_shared(
    e: &Mutex<Enroller>,
    body: &[u8],
    now: u64,
    sign: Option<&Signer>,
) -> (u16, Value) {
    let Ok(req) = serde_json::from_slice::<ConfirmReq>(body) else {
        return (400, json!({"error": "invalid request"}));
    };
    let Some(sign) = sign else {
        return fail(Error::Sign);
    };
    let request = {
        let mut guard = e.lock().unwrap_or_else(|poison| poison.into_inner());
        match guard.begin_confirm(&req.id, &req.fingerprint, now) {
            Ok(value) => value,
            Err(error) => return fail(error),
        }
    };
    let signed = sign(&request.node_id, &request.csr_pem);
    let mut guard = e.lock().unwrap_or_else(|poison| poison.into_inner());
    match guard.finish_confirm(&request.id, now, signed) {
        Ok(()) => (202, json!({"state": "issued-unconfirmed"})),
        Err(error) => fail(error),
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

    fn join_context() -> crate::fleet_run::JoinOfferContext {
        crate::fleet_run::JoinOfferContext {
            host: "hub.example.test".into(),
            port: 18444,
            hub_id: "hub-1".into(),
            hub_name: "hub-1.fleet.baft.invalid".into(),
            hub_pin: "a".repeat(64),
        }
    }

    fn csr(node_id: &str, challenge: &str) -> String {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("fleet-api-csr-{:x}", rand::random::<u64>()));
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
        fs::write(&config_path, format!("[ req ]\nprompt = no\ndistinguished_name = dn\nattributes = req_attributes\n[ dn ]\nCN = {node_id}\n[ req_attributes ]\nchallengePassword = {challenge}\n")).unwrap();
        let request = crate::pair_ca::run(
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
        String::from_utf8(request).unwrap()
    }

    #[test]
    fn offer_list_confirm_flow_never_leaks_secrets_in_the_list() {
        let mut e = Enroller::default();
        let (c, v) = offer(
            &mut e,
            br#"{"node_id":"edge-1"}"#,
            10,
            Some(&join_context()),
        );
        assert_eq!(c, 200);
        let id = v["id"].as_str().unwrap().to_string();
        let encoded = v["join_offer"].as_str().unwrap();
        let decoded = crate::fleet_join::JoinOffer::decode(encoded, 10).unwrap();
        let (token, nonce) = (decoded.token, decoded.nonce);
        assert_eq!(decoded.host, "hub.example.test");
        assert_eq!(decoded.port, 18444);
        assert_eq!(decoded.hub_id, "hub-1");
        let listed = list(&mut e, 11).1.to_string();
        assert!(!listed.contains(&token) && !listed.contains(&nonce));
        let request = csr("edge-1", &nonce);
        let fp = crate::fleet_ca::validate_node_csr("edge-1", &request).unwrap();
        e.submit(&id, "edge-1", &token, &nonce, &request, 12)
            .unwrap();
        assert_eq!(
            list(&mut e, 13).1["offers"][0]["state"],
            "needs-confirmation"
        );
        let body = format!(r#"{{"id":"{id}","fingerprint":"{fp}"}}"#);
        // No signer: refused and nothing consumed.
        assert_eq!(confirm(&mut e, body.as_bytes(), 14, None).0, 503);
        let signer = |_: &str, csr: &str| Ok(format!("CERT({csr})"));
        let expected_certificate = format!("CERT({request})");
        let (c, v) = confirm(&mut e, body.as_bytes(), 15, Some(&signer));
        assert_eq!(c, 202);
        assert_eq!(v["state"], "issued-unconfirmed");
        assert!(v.get("certificate").is_none());
        assert_eq!(
            e.certificate_for_delivery(&id),
            Ok(expected_certificate.as_str())
        );
    }

    #[test]
    fn fleet_confirm_releases_enroller_lock_during_helper_call() {
        use std::sync::Mutex;
        let enroller = Mutex::new(Enroller::default());
        let (id, token, nonce) = {
            let mut guard = enroller.lock().unwrap();
            let (status, value) = offer(
                &mut guard,
                br#"{"node_id":"edge-1"}"#,
                10,
                Some(&join_context()),
            );
            assert_eq!(status, 200);
            let encoded = value["join_offer"].as_str().unwrap();
            let decoded = crate::fleet_join::JoinOffer::decode(encoded, 10).unwrap();
            (
                value["id"].as_str().unwrap().to_owned(),
                decoded.token,
                decoded.nonce,
            )
        };
        let request = csr("edge-1", &nonce);
        let fingerprint = crate::fleet_ca::validate_node_csr("edge-1", &request).unwrap();
        enroller
            .lock()
            .unwrap()
            .submit(&id, "edge-1", &token, &nonce, &request, 11)
            .unwrap();
        let body = format!(r#"{{"id":"{id}","fingerprint":"{fingerprint}"}}"#);
        let signer = |_: &str, _: &str| {
            assert!(
                enroller.try_lock().is_ok(),
                "Enroller mutex must be released while the helper signs"
            );
            Ok("CERT".to_owned())
        };
        assert_eq!(
            confirm_shared(&enroller, body.as_bytes(), 12, Some(&signer)).0,
            202
        );
    }

    #[test]
    fn fleet_offer_unavailable_without_join_does_not_create_offer() {
        let mut e = Enroller::default();
        assert_eq!(offer(&mut e, br#"{"node_id":"edge-1"}"#, 10, None).0, 503);
        assert!(list(&mut e, 11).1["offers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn fleet_offer_encoding_failure_closes_created_offer() {
        let mut e = Enroller::default();
        let mut context = join_context();
        context.hub_pin = "not-a-pin".into();
        assert_eq!(
            offer(&mut e, br#"{"node_id":"edge-1"}"#, 10, Some(&context)).0,
            500
        );
        let listed = list(&mut e, 11).1;
        assert_eq!(listed["offers"].as_array().unwrap().len(), 1);
        assert_eq!(listed["offers"][0]["state"], "closed");
    }

    #[test]
    fn bad_input_maps_to_clean_errors() {
        let mut e = Enroller::default();
        assert_eq!(offer(&mut e, b"{", 0, Some(&join_context())).0, 400);
        assert_eq!(
            offer(&mut e, br#"{"node_id":"X y"}"#, 0, Some(&join_context())).0,
            400
        );
        assert_eq!(
            offer(
                &mut e,
                br#"{"node_id":"a","x":1}"#,
                0,
                Some(&join_context())
            )
            .0,
            400
        );
        offer(&mut e, br#"{"node_id":"a"}"#, 0, Some(&join_context()));
        assert_eq!(
            offer(&mut e, br#"{"node_id":"a"}"#, 1, Some(&join_context())).0,
            409
        );
        assert_eq!(deny(&mut e, br#"{"id":"nope"}"#).0, 404);
    }
}
