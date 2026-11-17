//! Replays every seed, corpus entry and past crash through the harness in plain `cargo test` (stable toolchain).
//! A crash found once stays fixed: drop the artifact into fuzz/regressions/ and it runs forever.
use std::{fs, path::Path};

fn replay(dir: &str) -> usize {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    let Ok(entries) = fs::read_dir(&dir) else { return 0 };
    let mut n = 0;
    for e in entries.flatten().filter(|e| e.path().is_file()) {
        fuzz_example::check_parse(&fs::read(e.path()).unwrap());
        n += 1;
    }
    n
}

#[test]
fn corpus_and_regressions_do_not_panic() {
    let n = replay("fuzz/corpus/frame_parse") + replay("fuzz/regressions");
    println!("replayed {n} inputs");
}

#[test]
fn hand_written_edges() {
    for input in [&b""[..], &[0], &[0, 0], &[0, 0, 5, 1], &[1, 0xff, 0xff], &[7, 0, 0]] {
        fuzz_example::check_parse(input);
    }
}
