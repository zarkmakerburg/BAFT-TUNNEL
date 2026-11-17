//! # Design Contract
//! **Role:** libFuzzer entry point: feeds raw bytes to the pure harness `fuzz_example::check_parse`.
//! **Patterns:** Harness as a pure function in the library (rs-fuzzing)
//! **Invariants:** no logic here — one call; the library function holds the oracle.
//! **Extend by:** a new target = new file in this directory + a seed directory under `fuzz/corpus/<name>/`.
//! **Skills:** rs-fuzzing

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    fuzz_example::check_parse(data);
});
