//! Agent-side reading of the evidence a running unit publishes in its runtime directory
//! (`session_status`, ADR-0024). Read-only. Every file is checked (regular file, small, valid
//! JSON, written by a live process and, when the unit's main PID is known, by that process);
//! anything that fails a check is treated as absent, so Unknown is never upgraded.
use std::path::Path;

use serde_json::{Value, json};

use crate::session_status::{COUNTERS_FILE, FAILURE_FILE, FILE as SESSION_FILE};

const MAX_FILE: u64 = 4096;

/// Evidence read from one runtime directory at one moment.
#[derive(Default)]
pub struct Evidence {
    pub session: Option<Value>,
    pub counters: Option<Value>,
    pub failure: Option<Value>,
}

fn alive(pid: u64) -> bool {
    pid > 0 && Path::new("/proc").join(pid.to_string()).is_dir()
}

fn read_json(dir: &Path, name: &str) -> Option<Value> {
    let path = dir.join(name);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn owned(v: &Value, expected_pid: Option<u32>) -> bool {
    let Some(pid) = v["pid"].as_u64() else {
        return false;
    };
    alive(pid) && expected_pid.is_none_or(|e| u64::from(e) == pid)
}

/// Read the unit's evidence. `expected_pid` is the unit's main PID when systemd reported it.
pub fn read(dir: &Path, expected_pid: Option<u32>) -> Evidence {
    Evidence {
        session: read_json(dir, SESSION_FILE)
            .filter(|v| v["state"] == "authorized" && owned(v, expected_pid)),
        counters: read_json(dir, COUNTERS_FILE)
            .filter(|v| v["counters_version"] == 1 && owned(v, expected_pid)),
        // A failure from an earlier process is still history; it is reported with its time.
        failure: read_json(dir, FAILURE_FILE).filter(|v| v["stage"].is_string()),
    }
}

/// One reading of the unit's cumulative counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrafficSample {
    pub t: u64,
    pub pid: u64,
    pub from_peer: u64,
    pub to_peer: u64,
}

pub fn sample(counters: &Value) -> Option<TrafficSample> {
    Some(TrafficSample {
        t: counters["sampled_unix"].as_u64()?,
        pid: counters["pid"].as_u64()?,
        from_peer: counters["bytes_from_peer"].as_u64()?,
        to_peer: counters["bytes_to_peer"].as_u64()?,
    })
}

/// Rates between consecutive samples of the same process. A restart (new PID) or a counter that
/// went backwards starts a new series instead of producing a bogus rate.
pub fn rates(samples: &[TrafficSample]) -> Vec<Value> {
    samples
        .windows(2)
        .filter_map(|w| {
            let (a, b) = (w[0], w[1]);
            if a.pid != b.pid || b.t <= a.t || b.from_peer < a.from_peer || b.to_peer < a.to_peer {
                return None;
            }
            let window = (b.t - a.t) as f64;
            Some(json!({
                "t": b.t,
                "window_s": b.t - a.t,
                "from_peer_bps": (b.from_peer - a.from_peer) as f64 / window,
                "to_peer_bps": (b.to_peer - a.to_peer) as f64 / window,
            }))
        })
        .collect()
}

/// The latest sample time at which payload grew in *both* directions since the previous sample
/// of the same process: the strongest data-path signal the agent can observe without payload.
pub fn last_bidirectional(samples: &[TrafficSample]) -> Option<u64> {
    samples
        .windows(2)
        .filter(|w| {
            let (a, b) = (w[0], w[1]);
            a.pid == b.pid && b.from_peer > a.from_peer && b.to_peer > a.to_peer
        })
        .map(|w| w[1].t)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: u64, pid: u64, from_peer: u64, to_peer: u64) -> TrafficSample {
        TrafficSample {
            t,
            pid,
            from_peer,
            to_peer,
        }
    }

    #[test]
    fn rates_skip_restarts_and_resets() {
        let list = [
            s(10, 1, 0, 0),
            s(15, 1, 500, 100),
            s(20, 2, 0, 0),
            s(25, 2, 50, 0),
        ];
        let r = rates(&list);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["from_peer_bps"], 100.0);
        assert_eq!(r[0]["to_peer_bps"], 20.0);
        assert_eq!(r[1]["t"], 25);
        assert_eq!(last_bidirectional(&list), Some(15));
        assert_eq!(last_bidirectional(&list[2..]), None);
    }

    #[test]
    fn evidence_needs_a_live_owner_and_small_regular_files() {
        let d = std::env::temp_dir().join(format!("baft-observe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let me = std::process::id();
        std::fs::write(
            d.join(SESSION_FILE),
            json!({"state": "authorized", "pid": me}).to_string(),
        )
        .unwrap();
        std::fs::write(
            d.join(COUNTERS_FILE),
            json!({"counters_version": 1, "pid": u32::MAX}).to_string(),
        )
        .unwrap();
        let e = read(&d, None);
        assert!(e.session.is_some());
        assert!(e.counters.is_none(), "dead writer must not count");
        assert!(read(&d, Some(me + 1)).session.is_none(), "PID mismatch");
        std::fs::write(d.join(SESSION_FILE), "x".repeat(5000)).unwrap();
        assert!(read(&d, None).session.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
