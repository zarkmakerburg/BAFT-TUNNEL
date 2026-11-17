//! Resume handshake messages (ADR-0040 step 3). Pure encode/decode/validation; no I/O and not
//! yet wired into the tunnel. A `ResumeHello` is only ever evaluated after the new session has
//! passed the normal handshake and authorization (certificate, CRL, pins), so revocation ends
//! resumable streams; this module then proves the *old* session's token belongs to the same
//! identities, tunnel and direction.

#![allow(dead_code)] // wired into the tunnel in step 4 (ADR-0040)

use crate::authorization::ConnectionDirection;
use crate::handshake::resume_binding;
use crate::resume_core::{TokenBook, TokenError};
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

    /// Prove the old session's token belongs to *these* identities, tunnel and direction and
    /// consume it. The acceptor passes its own current ids, so a token issued to any other
    /// identity pair, tunnel or direction fails with `BindingMismatch`. The token is consumed
    /// on every outcome but `Unknown` (see `TokenBook`), so a hello cannot be replayed.
    pub fn redeem(
        &self,
        book: &mut TokenBook,
        client_id: &str,
        server_id: &str,
        tunnel_id: &str,
        direction: ConnectionDirection,
        now: Duration,
    ) -> Result<&[StreamPos], WireError> {
        self.check_shape()?;
        let mut token = [0_u8; 16];
        hex::decode_to_slice(&self.old_token, &mut token).map_err(|_| WireError::Malformed)?;
        let binding = resume_binding(
            client_id,
            server_id,
            tunnel_id,
            direction,
            &self.old_client_nonce,
            &self.old_server_nonce,
        );
        book.redeem(&token, &binding, now)
            .map_err(WireError::Token)?;
        Ok(&self.streams)
    }
}

impl ResumeReply {
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

    fn issue(book: &mut TokenBook, cn: &str, sn: &str, now: Duration) -> [u8; 16] {
        let token = [7_u8; 16];
        let binding = resume_binding("iran", "abroad", "t", D, cn, sn);
        book.issue(token, binding, now);
        token
    }

    #[test]
    fn valid_hello_redeems_once_and_replay_is_rejected() {
        let (cn, sn) = ("a".repeat(64), "b".repeat(64));
        let mut book = TokenBook::new(Duration::from_secs(60));
        let t = issue(&mut book, &cn, &sn, Duration::ZERO);
        let h = hello(&t, &cn, &sn);
        let enc = h.encode().unwrap();
        let h2 = ResumeHello::decode(&enc).unwrap();
        assert_eq!(
            h2.redeem(&mut book, "iran", "abroad", "t", D, Duration::from_secs(5))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            h2.redeem(&mut book, "iran", "abroad", "t", D, Duration::from_secs(6)),
            Err(WireError::Token(TokenError::Unknown)),
            "a replayed hello must not work"
        );
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
            let mut book = TokenBook::new(Duration::from_secs(60));
            let tok = issue(&mut book, &cn, &sn, Duration::ZERO);
            let h = hello(&tok, &cn, &sn);
            assert_eq!(
                h.redeem(&mut book, c, s, t, d, Duration::from_secs(1)),
                Err(WireError::Token(TokenError::BindingMismatch)),
                "{c} {s} {t} {d:?}"
            );
            // Consumed: even the right identities cannot use it afterwards.
            assert_eq!(
                h.redeem(&mut book, "iran", "abroad", "t", D, Duration::from_secs(2)),
                Err(WireError::Token(TokenError::Unknown))
            );
        }
        // A guessed nonce changes the binding.
        let mut book = TokenBook::new(Duration::from_secs(60));
        let tok = issue(&mut book, &cn, &sn, Duration::ZERO);
        let h = hello(&tok, &"c".repeat(64), &sn);
        assert_eq!(
            h.redeem(&mut book, "iran", "abroad", "t", D, Duration::from_secs(1)),
            Err(WireError::Token(TokenError::BindingMismatch))
        );
    }

    #[test]
    fn expired_and_unknown_tokens_are_rejected() {
        let (cn, sn) = ("a".repeat(64), "b".repeat(64));
        let mut book = TokenBook::new(Duration::from_secs(60));
        let tok = issue(&mut book, &cn, &sn, Duration::ZERO);
        assert_eq!(
            hello(&tok, &cn, &sn).redeem(
                &mut book,
                "iran",
                "abroad",
                "t",
                D,
                Duration::from_secs(61)
            ),
            Err(WireError::Token(TokenError::Expired))
        );
        assert_eq!(
            hello(&[9; 16], &cn, &sn).redeem(
                &mut book,
                "iran",
                "abroad",
                "t",
                D,
                Duration::from_secs(1)
            ),
            Err(WireError::Token(TokenError::Unknown))
        );
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
