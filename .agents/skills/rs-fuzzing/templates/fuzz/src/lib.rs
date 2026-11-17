//! # Design Contract
//! **Role:** A tiny length-prefixed frame codec, written as the fuzzing example: parse is total (never panics), bounded, and round-trips.
//! **Patterns:** Parse, don't validate (rs-type-driven) · Harness function separate from the fuzz target (rs-fuzzing)
//! **Invariants:** `parse` never panics and never allocates more than `MAX_PAYLOAD`; `encode(parse(x)?) == x[..consumed]`.
//! **Extend by:** a new field = new `ParseError` variant + a seed in `fuzz/corpus` + a regression file if it was a crash.
//! **Avoid:** `unwrap`/indexing on input; trusting the length prefix; panics in `Drop`.
//! **Skills:** rs-fuzzing, rs-security-review, rs-error-handling

pub const MAX_PAYLOAD: usize = 1 << 16;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Frame {
    pub kind: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    Truncated,
    TooLarge,
}

/// Parse one frame: `kind:u8, len:u16 (big endian), payload[len]`. Returns the frame and bytes consumed.
///
/// # Errors
/// `Truncated` when the input ends early, `TooLarge` when the declared length exceeds `MAX_PAYLOAD`.
pub fn parse(data: &[u8]) -> Result<(Frame, usize), ParseError> {
    let (&kind, rest) = data.split_first().ok_or(ParseError::Truncated)?;
    let len_bytes: [u8; 2] = rest.get(..2).ok_or(ParseError::Truncated)?.try_into().map_err(|_| ParseError::Truncated)?;
    let len = usize::from(u16::from_be_bytes(len_bytes));
    if len > MAX_PAYLOAD {
        return Err(ParseError::TooLarge);
    }
    let payload = rest.get(2..2 + len).ok_or(ParseError::Truncated)?;      // never trust `len` for indexing
    Ok((Frame { kind, payload: payload.to_vec() }, 3 + len))
}

/// Encode a frame. Payloads longer than `u16::MAX` are rejected.
///
/// # Errors
/// `TooLarge` when the payload does not fit the length prefix or `MAX_PAYLOAD`.
pub fn encode(f: &Frame) -> Result<Vec<u8>, ParseError> {
    let len = u16::try_from(f.payload.len()).map_err(|_| ParseError::TooLarge)?;
    if f.payload.len() > MAX_PAYLOAD { return Err(ParseError::TooLarge); }
    let mut out = Vec::with_capacity(3 + f.payload.len());
    out.push(f.kind);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&f.payload);
    Ok(out)
}

/// The harness: pure, deterministic, no I/O. A *panic or failed assert here is the finding*.
/// Called by the fuzz target, by the corpus-replay test and by property tests.
pub fn check_parse(data: &[u8]) {
    if let Ok((frame, used)) = parse(data) {
        assert!(used <= data.len());
        let again = encode(&frame).expect("a parsed frame must re-encode");
        assert_eq!(&again[..], &data[..used], "round trip changed the bytes");
    }
}

/// Structure-aware harness: generated frames must survive encode → parse.
#[cfg(feature = "arbitrary")]
pub fn check_roundtrip(frame: &Frame) {
    if let Ok(bytes) = encode(frame) {
        let (back, used) = parse(&bytes).expect("encoded frame must parse");
        assert_eq!(&back, frame);
        assert_eq!(used, bytes.len());
    }
}
