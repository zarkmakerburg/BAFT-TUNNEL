//! Stream resume core (ADR-0040). Pure logic: no I/O, no clock, no randomness; callers supply a
//! monotonic `now` and random token bytes. It decides offsets, acknowledgements, replay windows
//! and token validity. It is **not wired** into the tunnel and protocol v3 does not exist yet, so
//! nothing here may be presented as a working resume feature.
//!
//! Scope per ADR-0040: TCP-like byte streams, in-memory only (no restart-resume).
#![allow(dead_code)]

use std::collections::VecDeque;
use std::time::Duration;

pub const DEFAULT_STREAM_WINDOW: usize = 1024 * 1024;
pub const MAX_STREAM_WINDOW: usize = 16 * 1024 * 1024;
pub const DEFAULT_TUNNEL_BUDGET: usize = 64 * 1024 * 1024;
pub const DEFAULT_RESUME_GRACE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeError {
    /// ACK beyond what was ever sent, or behind what is already acknowledged.
    BadAck,
    /// The peer asks for bytes that are no longer buffered: this stream cannot resume (`Gone`).
    Unrecoverable,
    /// The peer claims to have received more than was sent.
    PeerAhead,
}

/// Per-tunnel memory budget shared by all stream send windows.
#[derive(Debug)]
pub struct TunnelBudget {
    cap: usize,
    used: usize,
}

impl TunnelBudget {
    pub fn new(cap: usize) -> Self {
        Self { cap, used: 0 }
    }
    /// Reserve up to `want` bytes; returns how many were granted (possibly 0). Never over the cap.
    fn reserve(&mut self, want: usize) -> usize {
        let grant = want.min(self.cap - self.used);
        self.used += grant;
        grant
    }
    fn release(&mut self, n: usize) {
        self.used = self.used.saturating_sub(n);
    }
    pub fn used(&self) -> usize {
        self.used
    }
}

/// Sender side of one stream direction. Keeps unacknowledged bytes so they can be resent.
#[derive(Debug)]
pub struct SendWindow {
    cap: usize,
    /// Offset of the first buffered byte (= cumulative ACK).
    base: u64,
    buf: VecDeque<u8>,
}

impl SendWindow {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.clamp(1, MAX_STREAM_WINDOW),
            base: 0,
            buf: VecDeque::new(),
        }
    }

    /// Copy `len` buffered bytes starting `start` bytes after the acknowledged offset.
    pub fn peek(&self, start: usize, len: usize) -> Vec<u8> {
        self.buf.iter().skip(start).take(len).copied().collect()
    }

    /// Offset the next new byte will get.
    pub fn next_offset(&self) -> u64 {
        self.base + self.buf.len() as u64
    }
    pub fn acked(&self) -> u64 {
        self.base
    }
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Accept as many bytes as the stream window and tunnel budget allow. The caller must stop
    /// reading its local socket for the rest (backpressure); nothing is dropped silently.
    /// Returns the number of bytes accepted, and the offset of the first one.
    pub fn push(&mut self, data: &[u8], budget: &mut TunnelBudget) -> (u64, usize) {
        let start = self.next_offset();
        let room = self.cap - self.buf.len();
        let granted = budget.reserve(data.len().min(room));
        self.buf.extend(&data[..granted]);
        (start, granted)
    }

    /// Cumulative ACK: the peer has everything below `offset`. Frees buffer and budget.
    pub fn ack(&mut self, offset: u64, budget: &mut TunnelBudget) -> Result<(), ResumeError> {
        if offset < self.base || offset > self.next_offset() {
            return Err(ResumeError::BadAck);
        }
        let n = (offset - self.base) as usize;
        self.buf.drain(..n);
        self.base = offset;
        budget.release(n);
        Ok(())
    }

    /// On resume the peer reports how much it received. Everything before that is acknowledged
    /// implicitly; the rest is returned for resending. A peer behind our ACK (`Unrecoverable`)
    /// or ahead of what we sent (`PeerAhead`) cannot resume.
    pub fn resume_from(
        &mut self,
        peer_received: u64,
        budget: &mut TunnelBudget,
    ) -> Result<Vec<u8>, ResumeError> {
        if peer_received < self.base {
            return Err(ResumeError::Unrecoverable);
        }
        if peer_received > self.next_offset() {
            return Err(ResumeError::PeerAhead);
        }
        self.ack(peer_received, budget)?;
        Ok(self.buf.iter().copied().collect())
    }

    /// Give back everything this window still holds (stream closed or grace expired).
    pub fn release_all(&mut self, budget: &mut TunnelBudget) {
        budget.release(self.buf.len());
        self.buf.clear();
    }
}

/// What the receiver does with an incoming frame `(offset, len)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    /// Deliver `data[skip..]`; `skip` bytes were already received (partial overlap).
    Deliver { skip: usize },
    /// Entirely old data: drop.
    Duplicate,
    /// A gap before this frame: ask the peer to resend from `expected`.
    Gap { expected: u64 },
}

/// Receiver side of one stream direction.
#[derive(Debug, Default)]
pub struct RecvState {
    received: u64,
}

impl RecvState {
    pub fn received(&self) -> u64 {
        self.received
    }

    pub fn accept(&mut self, offset: u64, len: usize) -> Accept {
        let end = offset + len as u64;
        if offset > self.received {
            return Accept::Gap {
                expected: self.received,
            };
        }
        if end <= self.received {
            return Accept::Duplicate;
        }
        let skip = (self.received - offset) as usize;
        self.received = end;
        Accept::Deliver { skip }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    #[test]
    fn push_is_bounded_by_window_and_tunnel_budget() {
        let mut b = TunnelBudget::new(10);
        let mut a = SendWindow::new(8);
        let mut c = SendWindow::new(8);
        assert_eq!(a.push(&[1; 20], &mut b), (0, 8));
        // Stream window full: nothing more, nothing dropped silently (caller sees 0).
        assert_eq!(a.push(&[1; 5], &mut b).1, 0);
        // Tunnel budget (10) leaves only 2 for the second stream.
        assert_eq!(c.push(&[2; 8], &mut b).1, 2);
        assert_eq!(b.used(), 10);
        a.ack(8, &mut b).unwrap();
        assert_eq!(b.used(), 2);
        assert_eq!(c.push(&[2; 8], &mut b).1, 6); // stream window 8 minus 2 held
    }

    #[test]
    fn ack_and_resume_reject_impossible_offsets() {
        let mut b = TunnelBudget::new(100);
        let mut w = SendWindow::new(50);
        w.push(&[7; 30], &mut b);
        assert_eq!(w.ack(31, &mut b), Err(ResumeError::BadAck));
        w.ack(10, &mut b).unwrap();
        assert_eq!(w.ack(5, &mut b), Err(ResumeError::BadAck));
        assert_eq!(w.resume_from(5, &mut b), Err(ResumeError::Unrecoverable));
        assert_eq!(w.resume_from(31, &mut b), Err(ResumeError::PeerAhead));
        assert_eq!(w.resume_from(25, &mut b).unwrap().len(), 5);
        assert_eq!(w.acked(), 25);
    }

    #[test]
    fn receiver_drops_duplicates_trims_overlap_and_reports_gaps() {
        let mut r = RecvState::default();
        assert_eq!(r.accept(0, 10), Accept::Deliver { skip: 0 });
        assert_eq!(r.accept(0, 10), Accept::Duplicate);
        assert_eq!(r.accept(5, 10), Accept::Deliver { skip: 5 });
        assert_eq!(r.received(), 15);
        assert_eq!(r.accept(20, 5), Accept::Gap { expected: 15 });
        assert_eq!(r.received(), 15);
    }

    /// Random session cuts: frames are lost or duplicated in flight, the peers resume from the
    /// receiver's offset. The received stream must equal the sent stream exactly.
    #[test]
    fn random_cuts_neither_lose_duplicate_nor_reorder() {
        for seed in 0..50u64 {
            let mut rng = Lcg(seed + 1);
            let total = 20_000usize;
            let source: Vec<u8> = (0..total).map(|i| (i * 31 + seed as usize) as u8).collect();
            let mut budget = TunnelBudget::new(4096);
            let mut tx = SendWindow::new(2048);
            let mut rx = RecvState::default();
            let mut out: Vec<u8> = Vec::new();
            let mut fed = 0usize;
            let mut guard = 0;
            while out.len() < total {
                guard += 1;
                assert!(guard < 200_000, "no progress, seed {seed}");
                // Application writes what the window accepts.
                if fed < total {
                    let want = (rng.next() % 300 + 1) as usize;
                    let end = (fed + want).min(total);
                    let (_, n) = tx.push(&source[fed..end], &mut budget);
                    fed += n;
                }
                // Network: send the buffered tail as one frame, maybe duplicated or lost.
                let from = tx.acked();
                let frame: Vec<u8> = tx.buf.iter().copied().collect();
                let copies = rng.next() % 3; // 0 = lost, 1 = normal, 2 = duplicated
                for _ in 0..copies {
                    match rx.accept(from, frame.len()) {
                        Accept::Deliver { skip } => out.extend_from_slice(&frame[skip..]),
                        Accept::Duplicate | Accept::Gap { .. } => {}
                    }
                }
                // ACK sometimes; sometimes the session is cut and both sides resume.
                match rng.next() % 4 {
                    0 => tx.ack(rx.received(), &mut budget).unwrap(),
                    1 => {
                        let resend = tx.resume_from(rx.received(), &mut budget).unwrap();
                        // Resent bytes start at the receiver's offset.
                        match rx.accept(rx.received(), resend.len()) {
                            Accept::Deliver { skip } => out.extend_from_slice(&resend[skip..]),
                            Accept::Duplicate => assert!(resend.is_empty()),
                            Accept::Gap { .. } => panic!("gap on resume"),
                        }
                    }
                    _ => {}
                }
            }
            assert_eq!(out, source, "seed {seed}");
        }
    }
}
