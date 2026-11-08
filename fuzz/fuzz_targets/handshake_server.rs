#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    baft_tunnel_fuzz::handshake_server(data);
});
