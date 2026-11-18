//! Resume handshake messages (ADR-0040 step 3). Pure encode/decode/validation; no I/O and not
//! yet wired into the tunnel. A `ResumeHello` is only ever evaluated after the new session has
//! passed the normal handshake and authorization (certificate, CRL, pins), so revocation ends
//! resumable streams; this module then proves the *old* session's token belongs to the same
//! identities, tunnel and direction.

#![allow(dead_code)] // wired into the tunnel in step 4 (ADR-0040)

use crate::authorization::ConnectionDirection;
use crate::handshake::resume_binding;
use crate::resume_registry::{Registry, SessionId, TokenError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// At most this many resumable streams are ever announced in one hello.
pub const MAX_RESUME_STREAMS: usize = 256;
/// Hard cap on an encoded message, so a hostile peer cannot make us buffer more.
pub const MAX_WIRE_BYTES: usize = 16 * 1024;
const TOKEN_HEX: usize = 32;
const NONCE_HEX: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamPos {
    pub stream_id: u64,
    /// Bytes of this stream's peer-to-us direction already delivered locally.
    pub recv_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeHello {
    pub old_token: String,
    pub old_client_nonce: String,
    pub old_server_nonce: String,
    pub streams: Vec<StreamPos>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "answer", rename_all = "kebab-case")]
pub enum StreamAnswer {
    /// The stream is alive here; resend from `recv_offset` (what we already delivered).
    Resume { stream_id: u64, recv_offset: u64 },
    /// Unknown, expired, closed, or unrecoverable: the opener must close its local side.
    Gone { stream_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeReply {
    pub answers: Vec<StreamAnswer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    TooLarge,
    Malformed,
    TooManyStreams,
    DuplicateStream,
    Token(TokenError),
}

fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, WireError> {
    if bytes.is_empty() || bytes.len() > MAX_WIRE_BYTES {
        return Err(WireError::TooLarge);
    }
    serde_json::from_slice(bytes).map_err(|_| WireError::Malformed)
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    let bytes = serde_json::to_vec(value).map_err(|_| WireError::Malformed)?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(WireError::TooLarge);
    }
    Ok(bytes)
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

impl ResumeHello {
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let hello: Self = decode(bytes)?;
        hello.check_shape()?;
        Ok(hello)
    }

    fn check_shape(&self) -> Result<(), WireError> {
        if !is_hex(&self.old_token, TOKEN_HEX)
            || !is_hex(&self.old_client_nonce, NONCE_HEX)
            || !is_hex(&self.old_server_nonce, NONCE_HEX)
        {
            return Err(WireError::Malformed);
        }
        if self.streams.len() > MAX_RESUME_STREAMS {
            return Err(WireError::TooManyStreams);
        }
        let mut seen = HashSet::new();
        if !self.streams.iter().all(|s| seen.insert(s.stream_id)) {
            return Err(WireError::DuplicateStream);
        }
        Ok(())
    }

    /// Prove the old session's token belongs to *these* identities, tunnel and direction, that the
    /// old session has really dropped, and consume the token (see `Registry::redeem`). The
    /// acceptor passes its own current, certificate-verified ids, so a token issued for any other
    /// identity pair, tunnel or direction fails with `BindingMismatch`. Streams the opener lists
    /// that the old session carried are `resumable`; listed ones it did not carry are `gone`;
    /// carried but unlisted ones are `orphaned` and the caller must close them.
    pub fn redeem(
        &self,
        registry: &mut Registry,
        new_session: SessionId,
        who: &Identity<'_>,
        now: Duration,
    ) -> Result<Redeemed, WireError> {
        self.check_shape()?;
        let mut token = [0_u8; 16];
        hex::decode_to_slice(&self.old_token, &mut token).map_err(|_| WireError::Malformed)?;
        let binding = resume_binding(
            who.client_id,
            who.server_id,
            who.tunnel_id,
            who.direction,
            &self.old_client_nonce,
            &self.old_server_nonce,
        );
        let owned = registry
            .redeem(&token, &binding, new_session, now)
            .map_err(WireError::Token)?
            .streams;
        let listed: HashSet<u64> = self.streams.iter().map(|s| s.stream_id).collect();
        let owned_set: HashSet<u64> = owned.iter().copied().collect();
        let mut out = Redeemed::default();
        for pos in &self.streams {
            if owned_set.contains(&pos.stream_id) {
                out.resumable.push(pos.clone());
            } else {
                out.gone.push(pos.stream_id);
            }
        }
        for id in owned {
            if !listed.contains(&id) {
                registry.unbind_stream(id);
                out.orphaned.push(id);
            }
        }
        Ok(out)
    }
}

/// The identities of the *new*, already authorized session (from the verified certificate and the
/// configured tunnel, never from peer-supplied text: security review M3).
pub struct Identity<'a> {
    pub client_id: &'a str,
    pub server_id: &'a str,
    pub tunnel_id: &'a str,
    pub direction: ConnectionDirection,
}

/// How an opener's streams map onto the redeemed session.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Redeemed {
    pub resumable: Vec<StreamPos>,
    pub gone: Vec<u64>,
    pub orphaned: Vec<u64>,
}

impl ResumeReply {
    /// The opener must check that the reply answers exactly what it asked: one answer per
    /// requested stream, no extras, no repeats (M4). Anything else is a protocol violation.
    pub fn check_matches(&self, requested: &[StreamPos]) -> Result<(), WireError> {
        let want: HashSet<u64> = requested.iter().map(|s| s.stream_id).collect();
        let mut seen = HashSet::new();
        for a in &self.answers {
            let id = match a {
                StreamAnswer::Resume { stream_id, .. } | StreamAnswer::Gone { stream_id } => {
                    *stream_id
                }
            };
            if !want.contains(&id) || !seen.insert(id) {
                return Err(WireError::Malformed);
            }
        }
        if seen.len() != want.len() {
            return Err(WireError::Malformed);
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let reply: Self = decode(bytes)?;
        if reply.answers.len() > MAX_RESUME_STREAMS {
            return Err(WireError::TooManyStreams);
        }
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handshake::{client_handshake_grant, server_handshake_grant};
    use tokio::io::duplex;

    const D: ConnectionDirection = ConnectionDirection::Direct;

    /// Run a real v3 handshake; both sides must agree on token and binding. Returns the grant and
    /// the client's view of the original nonces is not exposed, so tests derive the hello from a
    /// manual binding below where nonces matter.
    async fn handshake() -> (crate::handshake::ResumeGrant, crate::handshake::ResumeGrant) {
        let (mut c, mut s) = duplex(4096);
        let server =
            tokio::spawn(
                async move { server_handshake_grant(&mut s, "abroad", "iran", "t", D).await },
            );
        let client = client_handshake_grant(&mut c, "iran", "abroad", "t", D)
            .await
            .unwrap();
        (client, server.await.unwrap().unwrap())
    }

    #[tokio::test]
    async fn both_sides_derive_the_same_token_and_binding() {
        let (c, s) = handshake().await;
        assert_eq!(c, s);
        let (c2, _) = handshake().await;
        assert_ne!(c.token, c2.token, "tokens are fresh per session");
        assert_ne!(c.binding, c2.binding);
    }

    fn hello(token: &[u8; 16], cn: &str, sn: &str) -> ResumeHello {
        ResumeHello {
            old_token: hex::encode(token),
            old_client_nonce: cn.into(),
            old_server_nonce: sn.into(),
            streams: vec![StreamPos {
                stream_id: 1,
                recv_offset: 10,
            }],
        }
    }

    fn who<'a>(c: &'a str, s: &'a str, t: &'a str, d: ConnectionDirection) -> Identity<'a> {
        Identity {
            client_id: c,
            server_id: s,
            tunnel_id: t,
            direction: d,
        }
    }

    const NOW: Duration = Duration::from_secs(30);

    /// Register an old session that already dropped, plus a live new one; returns the new id.
    fn setup(reg: &mut Registry, cn: &str, sn: &str) -> (SessionId, [u8; 16]) {
        let token = [7_u8; 16];
        let binding = resume_binding("iran", "abroad", "t", D, cn, sn);
        let old = reg.register(token, binding, Duration::ZERO).unwrap();
        reg.bind_stream(old, 1).unwrap();
        reg.bind_stream(old, 3).unwrap();
        reg.drop_session(old, Duration::from_secs(10));
        let new = reg.register([8; 16], [8; 32], NOW).unwrap();
        (new, token)
    }

    #[test]
    fn valid_hello_redeems_once_and_replay_is_rejected() {
        let (cn, sn) = ("a".repeat(64), "b".repeat(64));
        let mut reg = Registry::new(Duration::from_secs(60), 4);
        let (new, t) = setup(&mut reg, &cn, &sn);
        let mut h = hello(&t, &cn, &sn);
        h.streams = vec![
            StreamPos {
                stream_id: 1,
                recv_offset: 10,
            },
            StreamPos {
                stream_id: 2,
                recv_offset: 0,
            },
        ];
        let h2 = ResumeHello::decode(&h.encode().unwrap()).unwrap();
        let out = h2
            .redeem(&mut reg, new, &who("iran", "abroad", "t", D), NOW)
            .unwrap();
        assert_eq!(out.resumable.len(), 1);
        assert_eq!(out.gone, vec![2], "listed but never carried");
        assert_eq!(out.orphaned, vec![3], "carried but not listed");
        assert_eq!(reg.owner_of(1), Some(new));
        assert_eq!(reg.owner_of(3), None);
        assert_eq!(
            h2.redeem(&mut reg, new, &who("iran", "abroad", "t", D), NOW),
            Err(WireError::Token(TokenError::Unknown)),
            "a replayed hello must not work"
        );
    }

    #[test]
    fn hello_cannot_redeem_while_the_old_session_is_alive() {
        let (cn, sn) = ("a".repeat(64), "b".repeat(64));
        let mut reg = Registry::new(Duration::from_secs(60), 4);
        let token = [7_u8; 16];
        let old = reg
            .register(
                token,
                resume_binding("iran", "abroad", "t", D, &cn, &sn),
                Duration::ZERO,
            )
            .unwrap();
        reg.bind_stream(old, 1).unwrap();
        let new = reg.register([8; 16], [8; 32], NOW).unwrap();
        let h = hello(&token, &cn, &sn);
        assert_eq!(
            h.redeem(&mut reg, new, &who("iran", "abroad", "t", D), NOW),
            Err(WireError::Token(TokenError::StillAlive))
        );
        assert_eq!(reg.owner_of(1), Some(old));
    }

    #[test]
    fn wrong_identity_tunnel_direction_or_nonce_is_rejected_and_burns_the_token() {
        let (cn, sn) = ("a".repeat(64), "b".repeat(64));
        let wrong: [(&str, &str, &str, ConnectionDirection); 4] = [
            ("mallory", "abroad", "t", D),
            ("iran", "mallory", "t", D),
            ("iran", "abroad", "other", D),
            ("iran", "abroad", "t", ConnectionDirection::Reverse),
        ];
        for (c, s, t, d) in wrong {
            let mut reg = Registry::new(Duration::from_secs(60), 4);
            let (new, tok) = setup(&mut reg, &cn, &sn);
            let h = hello(&tok, &cn, &sn);
            assert_eq!(
                h.redeem(&mut reg, new, &who(c, s, t, d), NOW),
                Err(WireError::Token(TokenError::BindingMismatch)),
                "{c} {s} {t} {d:?}"
            );
            // Consumed: even the right identities cannot use it afterwards.
            assert_eq!(
                h.redeem(&mut reg, new, &who("iran", "abroad", "t", D), NOW),
                Err(WireError::Token(TokenError::Unknown))
            );
        }
        // A guessed nonce changes the binding.
        let mut reg = Registry::new(Duration::from_secs(60), 4);
        let (new, tok) = setup(&mut reg, &cn, &sn);
        let h = hello(&tok, &"c".repeat(64), &sn);
        assert_eq!(
            h.redeem(&mut reg, new, &who("iran", "abroad", "t", D), NOW),
            Err(WireError::Token(TokenError::BindingMismatch))
        );
    }

    #[test]
    fn expiry_runs_from_the_drop_and_unknown_tokens_are_rejected() {
        let (cn, sn) = ("a".repeat(64), "b".repeat(64));
        let mut reg = Registry::new(Duration::from_secs(60), 4);
        let (new, tok) = setup(&mut reg, &cn, &sn); // dropped at t=10
        let late = Duration::from_secs(71); // 61 s after the drop at 10
        assert_eq!(
            hello(&tok, &cn, &sn).redeem(&mut reg, new, &who("iran", "abroad", "t", D), late),
            Err(WireError::Token(TokenError::Expired))
        );
        assert_eq!(
            hello(&[9; 16], &cn, &sn).redeem(&mut reg, new, &who("iran", "abroad", "t", D), NOW),
            Err(WireError::Token(TokenError::Unknown))
        );
    }

    #[test]
    fn reply_must_answer_exactly_what_was_asked() {
        let asked = vec![
            StreamPos {
                stream_id: 1,
                recv_offset: 0,
            },
            StreamPos {
                stream_id: 2,
                recv_offset: 0,
            },
        ];
        let ok = ResumeReply {
            answers: vec![
                StreamAnswer::Gone { stream_id: 2 },
                StreamAnswer::Resume {
                    stream_id: 1,
                    recv_offset: 4,
                },
            ],
        };
        assert!(ok.check_matches(&asked).is_ok());
        let extra = ResumeReply {
            answers: vec![
                StreamAnswer::Gone { stream_id: 1 },
                StreamAnswer::Gone { stream_id: 2 },
                StreamAnswer::Gone { stream_id: 3 },
            ],
        };
        let missing = ResumeReply {
            answers: vec![StreamAnswer::Gone { stream_id: 1 }],
        };
        let repeat = ResumeReply {
            answers: vec![
                StreamAnswer::Gone { stream_id: 1 },
                StreamAnswer::Gone { stream_id: 1 },
            ],
        };
        for bad in [extra, missing, repeat] {
            assert_eq!(bad.check_matches(&asked), Err(WireError::Malformed));
        }
    }

    #[test]
    fn decoding_is_bounded_strict_and_never_panics() {
        let (cn, sn) = ("a".repeat(64), "b".repeat(64));
        let good = hello(&[1; 16], &cn, &sn);
        let mut many = good.clone();
        many.streams = (0..=MAX_RESUME_STREAMS as u64)
            .map(|stream_id| StreamPos {
                stream_id,
                recv_offset: 0,
            })
            .collect();
        assert_eq!(
            ResumeHello::decode(&serde_json::to_vec(&many).unwrap()),
            Err(WireError::TooManyStreams)
        );
        let mut dup = good.clone();
        dup.streams.push(dup.streams[0].clone());
        assert_eq!(
            ResumeHello::decode(&serde_json::to_vec(&dup).unwrap()),
            Err(WireError::DuplicateStream)
        );
        let mut bad_tok = good.clone();
        bad_tok.old_token = "zz".into();
        assert_eq!(
            ResumeHello::decode(&serde_json::to_vec(&bad_tok).unwrap()),
            Err(WireError::Malformed)
        );
        assert_eq!(
            ResumeHello::decode(&vec![b'x'; MAX_WIRE_BYTES + 1]),
            Err(WireError::TooLarge)
        );
        assert_eq!(ResumeHello::decode(b""), Err(WireError::TooLarge));
        assert_eq!(
            ResumeHello::decode(br#"{"old_token":"","extra":1}"#),
            Err(WireError::Malformed)
        );
        // Mutation sweep: flipping any byte of a valid message must not panic.
        let enc = good.encode().unwrap();
        for i in 0..enc.len() {
            let mut m = enc.clone();
            m[i] ^= 0xff;
            let _ = ResumeHello::decode(&m);
        }
    }

    #[test]
    fn reply_round_trips_and_is_bounded() {
        let r = ResumeReply {
            answers: vec![
                StreamAnswer::Resume {
                    stream_id: 1,
                    recv_offset: 5,
                },
                StreamAnswer::Gone { stream_id: 2 },
            ],
        };
        assert_eq!(ResumeReply::decode(&r.encode().unwrap()).unwrap(), r);
        let big = ResumeReply {
            answers: (0..=MAX_RESUME_STREAMS as u64)
                .map(|stream_id| StreamAnswer::Gone { stream_id })
                .collect(),
        };
        assert_eq!(
            ResumeReply::decode(&serde_json::to_vec(&big).unwrap()),
            Err(WireError::TooManyStreams)
        );
    }
}
