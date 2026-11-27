#![no_main]
use libfuzzer_sys::fuzz_target;

// Gate (c): the carrier's application datagram frame and the bounded control-plane hello are the
// two entry points a remote peer reaches before any session state exists. Both must refuse every
// malformed shape without panicking, allocating unboundedly or reading past the frame.
fuzz_target!(|data: &[u8]| {
    let _ = baft_tunnel_fuzz::udp_carrier::decode_datagram(data);
    let _ = baft_tunnel_fuzz::udp_carrier::decode_hello(data);
});
