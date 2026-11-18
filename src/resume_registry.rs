//! Session-bound registry of resumable streams and their single-use resume tokens
//! (ADR-0040, security review of step 3).
//!
//! * H1: a token's grace period starts when the carrying session *drops*, not when it was issued,
//!   so a long-lived session never holds an already-expired token.
//! * H2: a token cannot be redeemed while its session is alive, and every stream belongs to
//!   exactly one session, so a rotation or a second connection can never put one stream on two
//!   sessions.
//! * M1: the store is bounded; the oldest dropped session is evicted, never a live one.
//! * M2: no `Debug` impl prints a token or binding.

#![allow(dead_code)] // wired into the tunnel in step 4b (ADR-0040)

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::time::Duration;

pub type SessionId = u64;

/// Streams one session may carry (same bound as `ResumeHello`).
pub const MAX_STREAMS_PER_SESSION: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    Unknown,
    Expired,
    /// The token is for different identities, tunnel, direction or handshake.
    BindingMismatch,
    /// The issuing session has not dropped. The token is kept: this is a race, not an attack.
    StillAlive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    /// All session slots are held by live sessions.
    Full,
    UnknownSession,
    SessionDropped,
    /// The stream already belongs to a session.
    StreamTaken,
    TooManyStreams,
}

struct Entry {
    token: [u8; 16],
    binding: [u8; 32],
    dropped_at: Option<Duration>,
    streams: BTreeSet<u64>,
}

pub struct Registry {
    grace: Duration,
    max_sessions: usize,
    next_id: SessionId,
    sessions: HashMap<SessionId, Entry>,
    by_token: HashMap<[u8; 16], SessionId>,
    owner: HashMap<u64, SessionId>,
}

impl fmt::Debug for Registry {
    // Deliberately content-free: tokens and bindings must never reach logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("sessions", &self.sessions.len())
            .field("streams", &self.owner.len())
            .finish_non_exhaustive()
    }
}

/// Result of a successful redeem: the streams the old session carried, now owned by the new one.
#[derive(Debug, PartialEq, Eq)]
pub struct Redeemed {
    pub streams: Vec<u64>,
}

impl Registry {
    pub fn new(grace: Duration, max_sessions: usize) -> Self {
        Self {
            grace,
            max_sessions: max_sessions.max(1),
            next_id: 1,
            sessions: HashMap::new(),
            by_token: HashMap::new(),
            owner: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn stream_count(&self) -> usize {
        self.owner.len()
    }

    /// Register a freshly authenticated session and its token. Returns its id.
    pub fn register(
        &mut self,
        token: [u8; 16],
        binding: [u8; 32],
        now: Duration,
    ) -> Result<SessionId, RegistryError> {
        self.sweep(now);
        if self.sessions.len() >= self.max_sessions {
            let oldest = self
                .sessions
                .iter()
                .filter_map(|(id, e)| e.dropped_at.map(|at| (at, *id)))
                .min()
                .map(|(_, id)| id);
            match oldest {
                Some(id) => {
                    self.remove(id);
                }
                None => return Err(RegistryError::Full),
            }
        }
        // A colliding token (never expected from a 128-bit random value) is refused outright.
        if self.by_token.contains_key(&token) {
            return Err(RegistryError::Full);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.by_token.insert(token, id);
        self.sessions.insert(
            id,
            Entry {
                token,
                binding,
                dropped_at: None,
                streams: BTreeSet::new(),
            },
        );
        Ok(id)
    }

    /// Bind a stream to a live session. A stream is owned by at most one session.
    pub fn bind_stream(&mut self, session: SessionId, stream_id: u64) -> Result<(), RegistryError> {
        let entry = self
            .sessions
            .get_mut(&session)
            .ok_or(RegistryError::UnknownSession)?;
        if entry.dropped_at.is_some() {
            return Err(RegistryError::SessionDropped);
        }
        if self.owner.contains_key(&stream_id) {
            return Err(RegistryError::StreamTaken);
        }
        if entry.streams.len() >= MAX_STREAMS_PER_SESSION {
            return Err(RegistryError::TooManyStreams);
        }
        entry.streams.insert(stream_id);
        self.owner.insert(stream_id, session);
        Ok(())
    }

    /// A stream ended normally (Fin both ways or Reset): it can never resume.
    pub fn unbind_stream(&mut self, stream_id: u64) {
        if let Some(session) = self.owner.remove(&stream_id) {
            if let Some(e) = self.sessions.get_mut(&session) {
                e.streams.remove(&stream_id);
            }
        }
    }

    pub fn owner_of(&self, stream_id: u64) -> Option<SessionId> {
        self.owner.get(&stream_id).copied()
    }

    /// The carrying connection dropped: the grace period starts now (H1).
    pub fn drop_session(&mut self, session: SessionId, now: Duration) {
        if let Some(e) = self.sessions.get_mut(&session) {
            e.dropped_at.get_or_insert(now);
        }
    }

    /// Redeem a token for a new, live session. The old session must have dropped (H2). On success
    /// its streams move to `new_session` atomically and the old entry and token are gone. A
    /// mismatch or expiry burns the token; `StillAlive` and `Unknown` do not.
    pub fn redeem(
        &mut self,
        token: &[u8; 16],
        binding: &[u8; 32],
        new_session: SessionId,
        now: Duration,
    ) -> Result<Redeemed, TokenError> {
        let Some(&old) = self.by_token.get(token) else {
            return Err(TokenError::Unknown);
        };
        if old == new_session {
            return Err(TokenError::StillAlive);
        }
        let entry = self.sessions.get(&old).ok_or(TokenError::Unknown)?;
        let Some(dropped_at) = entry.dropped_at else {
            return Err(TokenError::StillAlive);
        };
        if self
            .sessions
            .get(&new_session)
            .is_none_or(|e| e.dropped_at.is_some())
        {
            return Err(TokenError::Unknown);
        }
        if now.saturating_sub(dropped_at) > self.grace {
            self.remove(old);
            return Err(TokenError::Expired);
        }
        if &entry.binding != binding {
            self.remove(old);
            return Err(TokenError::BindingMismatch);
        }
        let old_entry = self.remove_keep_streams(old);
        let streams: Vec<u64> = old_entry.streams.iter().copied().collect();
        let new = self.sessions.get_mut(&new_session).expect("checked above");
        for s in &streams {
            new.streams.insert(*s);
            self.owner.insert(*s, new_session);
        }
        Ok(Redeemed { streams })
    }

    /// Drop sessions whose grace ran out. Returns the streams the caller must now close.
    pub fn sweep(&mut self, now: Duration) -> Vec<u64> {
        let grace = self.grace;
        let expired: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, e)| {
                e.dropped_at
                    .is_some_and(|at| now.saturating_sub(at) > grace)
            })
            .map(|(id, _)| *id)
            .collect();
        let mut closed = Vec::new();
        for id in expired {
            closed.extend(self.remove(id));
        }
        closed
    }

    fn remove_keep_streams(&mut self, id: SessionId) -> Entry {
        let e = self.sessions.remove(&id).expect("session exists");
        self.by_token.remove(&e.token);
        e
    }

    /// Remove a session and release its streams; returns them so the caller can close them.
    fn remove(&mut self, id: SessionId) -> Vec<u64> {
        let e = self.remove_keep_streams(id);
        for s in &e.streams {
            self.owner.remove(s);
        }
        e.streams.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Duration = Duration::from_secs(60);
    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn grace_starts_at_drop_not_at_issue() {
        let mut r = Registry::new(G, 4);
        let old = r.register([1; 16], [9; 32], secs(0)).unwrap();
        r.bind_stream(old, 7).unwrap();
        // The session lives for an hour, then drops: the token is still good (H1).
        r.drop_session(old, secs(3600));
        let new = r.register([2; 16], [8; 32], secs(3601)).unwrap();
        assert_eq!(
            r.redeem(&[1; 16], &[9; 32], new, secs(3630)),
            Ok(Redeemed { streams: vec![7] })
        );
        assert_eq!(r.owner_of(7), Some(new));
        // ... but only for the grace after the drop.
        let mut r = Registry::new(G, 4);
        let old = r.register([1; 16], [9; 32], secs(0)).unwrap();
        r.drop_session(old, secs(3600));
        let new = r.register([2; 16], [8; 32], secs(3601)).unwrap();
        assert_eq!(
            r.redeem(&[1; 16], &[9; 32], new, secs(3661)),
            Err(TokenError::Expired)
        );
        assert_eq!(
            r.redeem(&[1; 16], &[9; 32], new, secs(3662)),
            Err(TokenError::Unknown)
        );
    }

    #[test]
    fn a_live_session_cannot_be_hijacked_and_the_token_survives_the_race() {
        let mut r = Registry::new(G, 4);
        let old = r.register([1; 16], [9; 32], secs(0)).unwrap();
        r.bind_stream(old, 7).unwrap();
        let new = r.register([2; 16], [8; 32], secs(1)).unwrap();
        assert_eq!(
            r.redeem(&[1; 16], &[9; 32], new, secs(2)),
            Err(TokenError::StillAlive)
        );
        // Still owned by the old session; nothing moved.
        assert_eq!(r.owner_of(7), Some(old));
        // Once the old session drops, the same token works.
        r.drop_session(old, secs(3));
        assert!(r.redeem(&[1; 16], &[9; 32], new, secs(4)).is_ok());
        // A session cannot redeem its own token.
        let mut r = Registry::new(G, 4);
        let s = r.register([1; 16], [9; 32], secs(0)).unwrap();
        r.drop_session(s, secs(1));
        assert_eq!(
            r.redeem(&[1; 16], &[9; 32], s, secs(2)),
            Err(TokenError::StillAlive)
        );
    }

    #[test]
    fn streams_belong_to_exactly_one_session() {
        let mut r = Registry::new(G, 4);
        let a = r.register([1; 16], [9; 32], secs(0)).unwrap();
        let b = r.register([2; 16], [8; 32], secs(0)).unwrap();
        r.bind_stream(a, 1).unwrap();
        assert_eq!(r.bind_stream(b, 1), Err(RegistryError::StreamTaken));
        assert_eq!(r.bind_stream(99, 5), Err(RegistryError::UnknownSession));
        r.drop_session(a, secs(1));
        assert_eq!(r.bind_stream(a, 2), Err(RegistryError::SessionDropped));
        // Ended streams are released and can be reused by id.
        r.unbind_stream(1);
        r.bind_stream(b, 1).unwrap();
        for i in 10..(10 + MAX_STREAMS_PER_SESSION as u64 - 1) {
            r.bind_stream(b, i).unwrap();
        }
        assert_eq!(r.bind_stream(b, 9999), Err(RegistryError::TooManyStreams));
    }

    #[test]
    fn wrong_binding_burns_the_token_and_releases_streams() {
        let mut r = Registry::new(G, 4);
        let old = r.register([1; 16], [9; 32], secs(0)).unwrap();
        r.bind_stream(old, 7).unwrap();
        r.drop_session(old, secs(1));
        let new = r.register([2; 16], [8; 32], secs(2)).unwrap();
        assert_eq!(
            r.redeem(&[1; 16], &[0; 32], new, secs(3)),
            Err(TokenError::BindingMismatch)
        );
        assert_eq!(
            r.redeem(&[1; 16], &[9; 32], new, secs(4)),
            Err(TokenError::Unknown)
        );
        assert_eq!(r.owner_of(7), None);
        assert_eq!(
            r.redeem(&[5; 16], &[9; 32], new, secs(4)),
            Err(TokenError::Unknown)
        );
    }

    #[test]
    fn store_is_bounded_and_evicts_the_oldest_dropped_never_a_live_one() {
        let mut r = Registry::new(G, 3);
        let a = r.register([1; 16], [1; 32], secs(0)).unwrap();
        let b = r.register([2; 16], [2; 32], secs(0)).unwrap();
        let c = r.register([3; 16], [3; 32], secs(0)).unwrap();
        // All live: a fourth session is refused, not squeezed in.
        assert_eq!(
            r.register([4; 16], [4; 32], secs(1)),
            Err(RegistryError::Full)
        );
        r.bind_stream(b, 20).unwrap();
        r.drop_session(b, secs(5));
        r.drop_session(a, secs(2));
        // The oldest dropped (a) goes first; b and c survive.
        let d = r.register([4; 16], [4; 32], secs(6)).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r.owner_of(20), Some(b));
        assert_ne!(d, c);
        // Expired sessions are swept and their streams reported for closing.
        assert_eq!(r.sweep(secs(70)), vec![20]);
        assert_eq!(r.stream_count(), 0);
    }

    #[test]
    fn debug_output_never_contains_token_or_binding_bytes() {
        let mut r = Registry::new(G, 4);
        let s = r.register([0xAB; 16], [0xCD; 32], secs(0)).unwrap();
        r.bind_stream(s, 1).unwrap();
        let shown = format!("{r:?}");
        assert!(!shown.contains("171") && !shown.to_lowercase().contains("ab, ab"));
        assert!(!shown.contains("205"), "{shown}");
        assert!(shown.contains("sessions"));
    }
}
