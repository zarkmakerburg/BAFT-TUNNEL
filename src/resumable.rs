//! Resumable stream engine (ADR-0040, step 2). A synchronous, I/O-free state machine plus the
//! frame codec: it turns application bytes into `Data` frames with offsets, applies incoming
//! frames (dedupe, gap detection, cumulative ACK), and re-aligns after a detach with
//! [`Endpoint::attach`]. The tunnel glue (substreams, tasks, grace timers) is a later step; this
//! module is **not wired** and v3 is not selectable, so nothing here is a working resume feature.
#![allow(dead_code)]

use crate::resume_core::{Accept, RecvState, ResumeError, SendWindow, TunnelBudget};

/// Maximum payload of one `Data` frame.
pub const MAX_PAYLOAD: usize = 64 * 1024;
/// `[kind u8][offset u64 BE][len u32 BE]`.
pub const HEADER_LEN: usize = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Data = 1,
    /// Cumulative acknowledgement: the sender of this frame has received every byte below `offset`.
    Ack = 2,
    /// End of the sender's byte stream at `offset`.
    Fin = 3,
    /// Abort: the stream cannot continue.
    Reset = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: Kind,
    pub offset: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    UnknownKind,
    PayloadTooLarge,
    /// Ack, Fin and Reset carry no payload.
    UnexpectedPayload,
    EmptyData,
}

impl Frame {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len());
        out.push(self.kind as u8);
        out.extend_from_slice(&self.offset.to_be_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    /// Decode one frame from the front of `buf`. `Ok(None)` means more bytes are needed; the
    /// declared length is validated from the header alone, so a hostile length never makes the
    /// caller buffer more than [`MAX_PAYLOAD`].
    pub fn decode(buf: &[u8]) -> Result<Option<(Frame, usize)>, CodecError> {
        if buf.len() < HEADER_LEN {
            // Reject an unknown kind as soon as it is seen.
            if let Some(&k) = buf.first() {
                kind_of(k)?;
            }
            return Ok(None);
        }
        let kind = kind_of(buf[0])?;
        let offset = u64::from_be_bytes(buf[1..9].try_into().expect("8 bytes"));
        let len = u32::from_be_bytes(buf[9..13].try_into().expect("4 bytes")) as usize;
        if len > MAX_PAYLOAD {
            return Err(CodecError::PayloadTooLarge);
        }
        match kind {
            Kind::Data if len == 0 => return Err(CodecError::EmptyData),
            Kind::Ack | Kind::Fin | Kind::Reset if len != 0 => {
                return Err(CodecError::UnexpectedPayload);
            }
            _ => {}
        }
        if buf.len() < HEADER_LEN + len {
            return Ok(None);
        }
        Ok(Some((
            Frame {
                kind,
                offset,
                payload: buf[HEADER_LEN..HEADER_LEN + len].to_vec(),
            },
            HEADER_LEN + len,
        )))
    }
}

fn kind_of(byte: u8) -> Result<Kind, CodecError> {
    match byte {
        1 => Ok(Kind::Data),
        2 => Ok(Kind::Ack),
        3 => Ok(Kind::Fin),
        4 => Ok(Kind::Reset),
        _ => Err(CodecError::UnknownKind),
    }
}

/// What applying an incoming frame produced for the application.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivered {
    /// New in-order bytes for the local socket.
    Bytes(Vec<u8>),
    /// Nothing new (duplicate, ACK, or a gap that waits for the resend).
    Nothing,
    /// The peer's stream ended cleanly after exactly the bytes already delivered.
    Eof,
    /// The peer aborted the stream.
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    /// Fin below what was already received, or ACK beyond what was sent.
    Inconsistent,
    /// Data or Fin after the peer's Fin.
    AfterFin,
}

/// One side of one resumable stream.
#[derive(Debug)]
pub struct Endpoint {
    send: SendWindow,
    recv: RecvState,
    /// Offset up to which bytes were put on the *current* attachment's wire.
    sent_upto: u64,
    /// Offset the peer has acknowledged to us for our `Fin`.
    fin_at: Option<u64>,
    fin_sent: bool,
    fin_acked: bool,
    peer_fin: Option<u64>,
    eof_delivered: bool,
    /// An ACK is owed to the peer (new bytes were delivered).
    ack_owed: bool,
}

impl Endpoint {
    pub fn new(window: usize) -> Self {
        Self {
            send: SendWindow::new(window),
            recv: RecvState::default(),
            sent_upto: 0,
            fin_at: None,
            fin_sent: false,
            fin_acked: false,
            peer_fin: None,
            eof_delivered: false,
            ack_owed: false,
        }
    }

    /// Application bytes to send. Returns how many were accepted; the caller stops reading its
    /// local socket for the rest (backpressure).
    pub fn write(&mut self, data: &[u8], budget: &mut TunnelBudget) -> usize {
        if self.fin_at.is_some() {
            return 0;
        }
        self.send.push(data, budget).1
    }

    /// The application closed its write side: a `Fin` follows the last byte.
    pub fn close_write(&mut self) {
        if self.fin_at.is_none() {
            self.fin_at = Some(self.send.next_offset());
        }
    }

    /// Next frame to transmit on the current attachment, if any: owed ACK first, then new data,
    /// then Fin.
    pub fn next_frame(&mut self) -> Option<Frame> {
        if self.ack_owed {
            self.ack_owed = false;
            return Some(Frame {
                kind: Kind::Ack,
                offset: self.recv.received(),
                payload: vec![],
            });
        }
        let next = self.send.next_offset();
        if self.sent_upto < next {
            let take = ((next - self.sent_upto) as usize).min(MAX_PAYLOAD);
            let start = (self.sent_upto - self.send.acked()) as usize;
            let payload = self.send.peek(start, take);
            let frame = Frame {
                kind: Kind::Data,
                offset: self.sent_upto,
                payload,
            };
            self.sent_upto += take as u64;
            return Some(frame);
        }
        if let Some(at) = self.fin_at {
            if !self.fin_sent && self.sent_upto == at {
                self.fin_sent = true;
                return Some(Frame {
                    kind: Kind::Fin,
                    offset: at,
                    payload: vec![],
                });
            }
        }
        None
    }

    /// Apply a frame from the peer.
    pub fn on_frame(
        &mut self,
        frame: &Frame,
        budget: &mut TunnelBudget,
    ) -> Result<Delivered, ProtocolError> {
        match frame.kind {
            Kind::Data => {
                if let Some(fin) = self.peer_fin {
                    if frame.offset + frame.payload.len() as u64 > fin {
                        return Err(ProtocolError::AfterFin);
                    }
                }
                match self.recv.accept(frame.offset, frame.payload.len()) {
                    Accept::Deliver { skip } => {
                        self.ack_owed = true;
                        Ok(Delivered::Bytes(frame.payload[skip..].to_vec()))
                    }
                    Accept::Duplicate => {
                        // Re-ACK so a sender that missed our ACK frees its window.
                        self.ack_owed = true;
                        Ok(Delivered::Nothing)
                    }
                    // A gap waits for the resend that `attach` triggers; nothing to deliver.
                    Accept::Gap { .. } => Ok(Delivered::Nothing),
                }
            }
            Kind::Ack => {
                self.send
                    .ack(frame.offset, budget)
                    .map_err(|_| ProtocolError::Inconsistent)?;
                if self.fin_at == Some(frame.offset) && self.fin_sent {
                    self.fin_acked = true;
                }
                Ok(Delivered::Nothing)
            }
            Kind::Fin => {
                if frame.offset < self.recv.received() {
                    return Err(ProtocolError::Inconsistent);
                }
                if self.peer_fin.is_some_and(|f| f != frame.offset) {
                    return Err(ProtocolError::Inconsistent);
                }
                self.peer_fin = Some(frame.offset);
                self.try_eof()
            }
            Kind::Reset => Ok(Delivered::Reset),
        }
    }

    /// EOF is delivered once, only when every byte below the peer's Fin was delivered.
    pub fn try_eof(&mut self) -> Result<Delivered, ProtocolError> {
        match self.peer_fin {
            Some(f) if f == self.recv.received() && !self.eof_delivered => {
                self.eof_delivered = true;
                self.ack_owed = true;
                Ok(Delivered::Eof)
            }
            _ => Ok(Delivered::Nothing),
        }
    }

    /// Offset this side has received (reported in `ResumeHello`).
    pub fn received(&self) -> u64 {
        self.recv.received()
    }

    /// A new substream replaces the old one. `peer_received` is the peer's reported receive
    /// offset: bytes below it are acknowledged implicitly and everything after is resent.
    pub fn attach(
        &mut self,
        peer_received: u64,
        budget: &mut TunnelBudget,
    ) -> Result<(), ResumeError> {
        self.send.resume_from(peer_received, budget)?;
        self.sent_upto = peer_received;
        self.fin_sent = false;
        self.ack_owed = true; // tell the peer where we are
        Ok(())
    }

    pub fn finished(&self) -> bool {
        self.fin_acked && self.eof_delivered
    }

    pub fn buffered(&self) -> usize {
        self.send.buffered()
    }

    /// Give back the memory this stream holds (closed or grace expired).
    pub fn release(&mut self, budget: &mut TunnelBudget) {
        self.send.release_all(budget);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_round_trips_and_rejects_bad_frames() {
        for f in [
            Frame {
                kind: Kind::Data,
                offset: 7,
                payload: vec![1, 2, 3],
            },
            Frame {
                kind: Kind::Ack,
                offset: 99,
                payload: vec![],
            },
            Frame {
                kind: Kind::Fin,
                offset: 5,
                payload: vec![],
            },
            Frame {
                kind: Kind::Reset,
                offset: 0,
                payload: vec![],
            },
        ] {
            let bytes = f.encode();
            // Truncated input needs more bytes, never a wrong frame.
            for cut in 0..bytes.len() {
                assert_eq!(Frame::decode(&bytes[..cut]).unwrap(), None);
            }
            assert_eq!(Frame::decode(&bytes).unwrap(), Some((f, bytes.len())));
        }
        let mut bad = Frame {
            kind: Kind::Data,
            offset: 0,
            payload: vec![0],
        }
        .encode();
        bad[0] = 9;
        assert_eq!(Frame::decode(&bad), Err(CodecError::UnknownKind));
        assert_eq!(Frame::decode(&[9]), Err(CodecError::UnknownKind));
        let mut big = vec![1u8];
        big.extend_from_slice(&0u64.to_be_bytes());
        big.extend_from_slice(&((MAX_PAYLOAD as u32) + 1).to_be_bytes());
        assert_eq!(Frame::decode(&big), Err(CodecError::PayloadTooLarge));
        let mut ack = Frame {
            kind: Kind::Ack,
            offset: 1,
            payload: vec![],
        }
        .encode();
        ack[12] = 1;
        ack.push(0);
        assert_eq!(Frame::decode(&ack), Err(CodecError::UnexpectedPayload));
        let empty = Frame {
            kind: Kind::Data,
            offset: 0,
            payload: vec![],
        }
        .encode();
        assert_eq!(Frame::decode(&empty), Err(CodecError::EmptyData));
    }

    #[test]
    fn decoder_never_panics_on_garbage() {
        let mut x = 12345u64;
        for _ in 0..20_000 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = (x >> 60) as usize + (x >> 40) as usize % 40;
            let buf: Vec<u8> = (0..len)
                .map(|i| ((x >> (i % 7)) ^ i as u64) as u8)
                .collect();
            let _ = Frame::decode(&buf);
        }
    }

    /// Two endpoints exchange bytes both ways over a link that loses and duplicates frames and is
    /// cut at random; after each cut both sides re-attach with their receive offsets. Both
    /// directions must arrive byte-exact, in order, with bounded buffers and a clean Fin/Eof.
    #[test]
    fn random_cuts_both_directions_byte_exact_with_fin() {
        for seed in 0..40u64 {
            let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let mut next = move || {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng >> 11
            };
            let total = 30_000usize;
            let src_a: Vec<u8> = (0..total).map(|i| (i as u64 * 7 + seed) as u8).collect();
            let src_b: Vec<u8> = (0..total / 2)
                .map(|i| (i as u64 * 13 + seed * 3) as u8)
                .collect();
            let (mut ba, mut bb) = (TunnelBudget::new(1 << 20), TunnelBudget::new(1 << 20));
            let (mut a, mut b) = (Endpoint::new(4096), Endpoint::new(4096));
            let (mut out_a, mut out_b) = (Vec::new(), Vec::new()); // what each side delivered
            let (mut fed_a, mut fed_b) = (0usize, 0usize);
            let (mut eof_a, mut eof_b) = (false, false);
            let mut wire_ab: Vec<Frame> = vec![]; // in flight A->B on the current attachment
            let mut wire_ba: Vec<Frame> = vec![];
            for step in 0..400_000 {
                assert!(step < 399_999, "no progress, seed {seed}");
                // Application writes.
                if fed_a < src_a.len() {
                    let end = (fed_a + (next() % 700 + 1) as usize).min(src_a.len());
                    fed_a += a.write(&src_a[fed_a..end], &mut ba);
                    if fed_a == src_a.len() {
                        a.close_write();
                    }
                }
                if fed_b < src_b.len() {
                    let end = (fed_b + (next() % 500 + 1) as usize).min(src_b.len());
                    fed_b += b.write(&src_b[fed_b..end], &mut bb);
                    if fed_b == src_b.len() {
                        b.close_write();
                    }
                }
                // Each side emits a frame; the link may lose or duplicate it.
                for _ in 0..2 {
                    if let Some(f) = a.next_frame() {
                        match next() % 6 {
                            0 => {}
                            1 => {
                                wire_ab.push(f.clone());
                                wire_ab.push(f);
                            }
                            _ => wire_ab.push(f),
                        }
                    }
                    if let Some(f) = b.next_frame() {
                        match next() % 6 {
                            0 => {}
                            1 => {
                                wire_ba.push(f.clone());
                                wire_ba.push(f);
                            }
                            _ => wire_ba.push(f),
                        }
                    }
                }
                // Deliver in order (a TCP substream keeps order); the cut drops what is in flight.
                if next() % 25 == 0 {
                    wire_ab.clear();
                    wire_ba.clear();
                    let (ra, rb) = (a.received(), b.received());
                    a.attach(rb, &mut ba).expect("attach a");
                    b.attach(ra, &mut bb).expect("attach b");
                } else {
                    for f in wire_ab.drain(..) {
                        match b.on_frame(&f, &mut bb).expect("b frame") {
                            Delivered::Bytes(x) => out_b.extend_from_slice(&x),
                            Delivered::Eof => eof_b = true,
                            _ => {}
                        }
                        if let Delivered::Eof = b.try_eof().unwrap() {
                            eof_b = true;
                        }
                    }
                    for f in wire_ba.drain(..) {
                        match a.on_frame(&f, &mut ba).expect("a frame") {
                            Delivered::Bytes(x) => out_a.extend_from_slice(&x),
                            Delivered::Eof => eof_a = true,
                            _ => {}
                        }
                        if let Delivered::Eof = a.try_eof().unwrap() {
                            eof_a = true;
                        }
                    }
                }
                assert!(a.buffered() <= 4096 && b.buffered() <= 4096);
                if a.finished() && b.finished() {
                    break;
                }
            }
            assert_eq!(out_b, src_a, "A->B seed {seed}");
            assert_eq!(out_a, src_b, "B->A seed {seed}");
            assert!(eof_a && eof_b, "both sides saw EOF, seed {seed}");
            a.release(&mut ba);
            b.release(&mut bb);
            assert_eq!(ba.used() + bb.used(), 0);
        }
    }

    #[test]
    fn protocol_violations_are_errors_not_panics() {
        let mut budget = TunnelBudget::new(1024);
        let mut e = Endpoint::new(64);
        // ACK beyond anything sent.
        let bad_ack = Frame {
            kind: Kind::Ack,
            offset: 10,
            payload: vec![],
        };
        assert_eq!(
            e.on_frame(&bad_ack, &mut budget),
            Err(ProtocolError::Inconsistent)
        );
        // Data, then a Fin below what was received.
        let d = Frame {
            kind: Kind::Data,
            offset: 0,
            payload: vec![1; 8],
        };
        assert_eq!(
            e.on_frame(&d, &mut budget),
            Ok(Delivered::Bytes(vec![1; 8]))
        );
        let fin_low = Frame {
            kind: Kind::Fin,
            offset: 4,
            payload: vec![],
        };
        assert_eq!(
            e.on_frame(&fin_low, &mut budget),
            Err(ProtocolError::Inconsistent)
        );
        // A Fin ahead of the data waits; the missing bytes then complete it.
        let fin = Frame {
            kind: Kind::Fin,
            offset: 16,
            payload: vec![],
        };
        assert_eq!(e.on_frame(&fin, &mut budget), Ok(Delivered::Nothing));
        let d2 = Frame {
            kind: Kind::Data,
            offset: 8,
            payload: vec![2; 9],
        };
        assert_eq!(e.on_frame(&d2, &mut budget), Err(ProtocolError::AfterFin));
        // A conflicting second Fin offset is inconsistent.
        let fin2 = Frame {
            kind: Kind::Fin,
            offset: 20,
            payload: vec![],
        };
        assert_eq!(
            e.on_frame(&fin2, &mut budget),
            Err(ProtocolError::Inconsistent)
        );
    }
}
