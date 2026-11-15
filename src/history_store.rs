//! Bounded, persisted operational history (B5). One-minute buckets of real local probe results and
//! unit payload-counter deltas, kept for 24 hours in `<state_dir>/history.json` so they survive an
//! agent restart. Nothing is invented: a minute with no probe has no bucket, and a counter delta is
//! counted only between two readings of the same unit process.
use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const BUCKET_SECONDS: u64 = 60;
pub const RETENTION_SECONDS: u64 = 24 * 3600;
pub const MAX_BUCKETS: usize = (RETENTION_SECONDS / BUCKET_SECONDS) as usize;
const FILE: &str = "history.json";
const MAX_FILE_BYTES: u64 = 512 * 1024;
const VERSION: u32 = 1;
/// Probes needed in a window before an availability alert may fire.
const MIN_PROBES_1H: u32 = 60;
const MIN_PROBES_24H: u32 = 360;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    /// Start of the minute, unix seconds.
    pub t: u64,
    pub probes: u32,
    pub up: u32,
    /// Sum of round-trip times of the successful probes, milliseconds.
    pub rtt_sum_ms: f64,
    /// Payload bytes relayed in this minute, from counter deltas of one unit process.
    pub from_peer_bytes: u64,
    pub to_peer_bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    v: u32,
    bucket_s: u64,
    buckets: Vec<Bucket>,
}

pub struct LongHistory {
    path: Option<PathBuf>,
    buckets: VecDeque<Bucket>,
    last_counters: Option<(u64, u64, u64)>,
    dirty: bool,
    last_flush: u64,
    /// Why the stored file was ignored or could not be written; reported, never hidden.
    pub load_error: Option<String>,
    pub write_error: Option<String>,
}

impl LongHistory {
    /// `state_dir = None` keeps history in memory only (reported as `persisted: false`).
    pub fn open(state_dir: Option<&Path>, now: u64) -> Self {
        let mut h = Self {
            path: state_dir.map(|d| d.join(FILE)),
            buckets: VecDeque::new(),
            last_counters: None,
            dirty: false,
            last_flush: now,
            load_error: None,
            write_error: None,
        };
        if let Some(path) = h.path.clone() {
            match load(&path, now) {
                Ok(b) => h.buckets = b.into(),
                Err(e) => h.load_error = Some(e),
            }
        }
        h
    }

    pub fn persisted(&self) -> bool {
        self.path.is_some()
    }

    /// Count one real probe and, when the unit published counters, their growth.
    pub fn record(
        &mut self,
        now: u64,
        up: bool,
        rtt_ms: Option<f64>,
        counters: Option<(u64, u64, u64)>,
    ) {
        let t = now - now % BUCKET_SECONDS;
        if self.buckets.back().is_none_or(|b| b.t < t) {
            self.buckets.push_back(Bucket {
                t,
                probes: 0,
                up: 0,
                rtt_sum_ms: 0.0,
                from_peer_bytes: 0,
                to_peer_bytes: 0,
            });
        }
        while self.buckets.len() > MAX_BUCKETS
            || self
                .buckets
                .front()
                .is_some_and(|b| b.t + RETENTION_SECONDS <= now)
        {
            self.buckets.pop_front();
        }
        let Some(b) = self.buckets.back_mut().filter(|b| b.t == t) else {
            return; // the clock went backwards; do not rewrite history
        };
        b.probes += 1;
        if up {
            b.up += 1;
            b.rtt_sum_ms += rtt_ms.unwrap_or(0.0);
        }
        if let Some((pid, from, to)) = counters {
            if let Some((p, f, o)) = self.last_counters {
                if p == pid && from >= f && to >= o {
                    b.from_peer_bytes = b.from_peer_bytes.saturating_add(from - f);
                    b.to_peer_bytes = b.to_peer_bytes.saturating_add(to - o);
                }
            }
            self.last_counters = Some((pid, from, to));
        }
        self.dirty = true;
    }

    /// Write at most once a minute, atomically, with owner-only permissions.
    pub fn flush_if_due(&mut self, now: u64) {
        if !self.dirty || now < self.last_flush + BUCKET_SECONDS {
            return;
        }
        self.flush(now);
    }

    pub fn flush(&mut self, now: u64) {
        self.last_flush = now;
        let Some(path) = self.path.clone() else {
            return;
        };
        let stored = Stored {
            v: VERSION,
            bucket_s: BUCKET_SECONDS,
            buckets: self.buckets.iter().cloned().collect(),
        };
        match write_atomic(&path, &stored) {
            Ok(()) => {
                self.dirty = false;
                self.write_error = None;
            }
            Err(e) => self.write_error = Some(e),
        }
    }

    #[cfg(test)]
    pub fn buckets(&self) -> &VecDeque<Bucket> {
        &self.buckets
    }

    pub fn view(&self, now: u64) -> Value {
        let cutoff = now.saturating_sub(RETENTION_SECONDS);
        let (mut probes, mut up, mut from, mut to) = (0u64, 0u64, 0u64, 0u64);
        let samples: Vec<Value> = self
            .buckets
            .iter()
            .filter(|b| b.t > cutoff)
            .map(|b| {
                probes += u64::from(b.probes);
                up += u64::from(b.up);
                from += b.from_peer_bytes;
                to += b.to_peer_bytes;
                json!({
                    "t": b.t,
                    "probes": b.probes,
                    "up": b.up,
                    "rtt_ms": (b.up > 0).then(|| b.rtt_sum_ms / f64::from(b.up)),
                    "from_peer_bytes": b.from_peer_bytes,
                    "to_peer_bytes": b.to_peer_bytes,
                })
            })
            .collect();
        json!({
            "bucket_s": BUCKET_SECONDS,
            "retention_s": RETENTION_SECONDS,
            "capacity": MAX_BUCKETS,
            "persisted": self.persisted(),
            "load_error": self.load_error,
            "write_error": self.write_error,
            "totals": {"probes": probes, "up": up, "from_peer_bytes": from, "to_peer_bytes": to},
            "samples": samples,
        })
    }

    /// Alerts derived only from stored probes; each carries its evidence.
    pub fn alerts(&self, now: u64) -> Vec<Value> {
        let mut out = Vec::new();
        for (id, window, min, threshold) in [
            ("availability_1h", 3600u64, MIN_PROBES_1H, 0.90f64),
            (
                "availability_24h",
                RETENTION_SECONDS,
                MIN_PROBES_24H,
                0.95f64,
            ),
        ] {
            let (mut probes, mut up) = (0u32, 0u32);
            for b in self.buckets.iter().filter(|b| b.t + window > now) {
                probes += b.probes;
                up += b.up;
            }
            if probes < min {
                continue;
            }
            let ratio = f64::from(up) / f64::from(probes);
            if ratio < threshold {
                out.push(json!({
                    "id": id,
                    "code": format!("alert.{id}"),
                    "severity": "warning",
                    "evidence": {
                        "probes": probes,
                        "up": up,
                        "availability_percent": (ratio * 1000.0).round() / 10.0,
                        "threshold_percent": threshold * 100.0,
                        "window_s": window,
                    },
                }));
            }
        }
        out
    }
}

const DIAG_FILE: &str = "diagnostics-history.json";
pub const MAX_DIAG_RUNS: usize = 100;

/// One finished diagnostics run, reduced to what is worth keeping: when, per-tier result codes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiagRun {
    pub at_unix: u64,
    pub highest_passed_tier: String,
    pub verified: bool,
    pub tiers: Vec<(String, String)>,
}

impl LongHistory {
    /// Append a diagnostics run, keeping the newest `MAX_DIAG_RUNS` within the retention window.
    pub fn record_diagnostics(&mut self, run: DiagRun) {
        let Some(dir) = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
        else {
            return;
        };
        let path = dir.join(DIAG_FILE);
        let mut runs = read_diag(&path, run.at_unix).unwrap_or_default();
        runs.push(run);
        if runs.len() > MAX_DIAG_RUNS {
            runs.drain(..runs.len() - MAX_DIAG_RUNS);
        }
        if let Err(e) = write_atomic_json(&path, &runs) {
            self.write_error = Some(e);
        }
    }

    pub fn diagnostics_runs(&self, now: u64) -> Vec<DiagRun> {
        self.path
            .as_ref()
            .and_then(|p| p.parent())
            .and_then(|d| read_diag(&d.join(DIAG_FILE), now))
            .unwrap_or_default()
    }
}

fn read_diag(path: &Path, now: u64) -> Option<Vec<DiagRun>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let runs: Vec<DiagRun> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(
        runs.into_iter()
            .filter(|r| r.at_unix + RETENTION_SECONDS > now)
            .collect(),
    )
}

fn load(path: &Path, now: u64) -> Result<Vec<Bucket>, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("history file unreadable: {e}")),
    };
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err("history file is not a regular file of bounded size; ignored".into());
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("history file unreadable: {e}"))?;
    let stored: Stored =
        serde_json::from_str(&text).map_err(|e| format!("history file malformed; ignored: {e}"))?;
    if stored.v != VERSION || stored.bucket_s != BUCKET_SECONDS {
        return Err("history file version differs; ignored".into());
    }
    let mut last = 0u64;
    let mut out = Vec::new();
    for b in stored.buckets {
        let ok = b.t % BUCKET_SECONDS == 0
            && b.t > last
            && b.t <= now
            && b.up <= b.probes
            && b.rtt_sum_ms.is_finite()
            && b.rtt_sum_ms >= 0.0;
        if !ok {
            return Err("history file inconsistent; ignored".into());
        }
        last = b.t;
        if b.t + RETENTION_SECONDS > now {
            out.push(b);
        }
    }
    if out.len() > MAX_BUCKETS {
        out.drain(..out.len() - MAX_BUCKETS);
    }
    Ok(out)
}

fn write_atomic(path: &Path, stored: &Stored) -> Result<(), String> {
    write_atomic_json(path, stored)
}

fn write_atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let result = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    result.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("history not saved: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("baft-hist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const T0: u64 = 1_700_000_040;

    #[test]
    fn buckets_aggregate_and_are_bounded() {
        let mut h = LongHistory::open(None, T0);
        for i in 0..(MAX_BUCKETS as u64 + 50) {
            h.record(T0 + i * 60, true, Some(2.0), None);
        }
        assert_eq!(h.buckets().len(), MAX_BUCKETS);
        let first = h.buckets().front().unwrap().t;
        assert_eq!(
            first,
            (T0 - T0 % 60) + 50 * 60,
            "oldest minutes are dropped first"
        );
        assert!(!h.view(T0).get("persisted").unwrap().as_bool().unwrap());
    }

    #[test]
    fn counter_deltas_only_within_one_process() {
        let mut h = LongHistory::open(None, T0);
        h.record(T0, true, Some(1.0), Some((7, 100, 200)));
        h.record(T0 + 5, true, Some(1.0), Some((7, 150, 260)));
        h.record(T0 + 10, true, Some(1.0), Some((8, 10, 10))); // restart: no delta
        h.record(T0 + 15, true, Some(1.0), Some((8, 30, 15)));
        let b = h.buckets().back().unwrap();
        assert_eq!((b.from_peer_bytes, b.to_peer_bytes), (50 + 20, 60 + 5));
    }

    #[test]
    fn persists_reloads_and_rejects_bad_files() {
        let d = dir("persist");
        let mut h = LongHistory::open(Some(&d), T0);
        h.record(T0, true, Some(4.0), None);
        h.record(T0 + 5, false, None, None);
        h.flush(T0 + 60);
        assert!(h.write_error.is_none());
        let again = LongHistory::open(Some(&d), T0 + 120);
        assert_eq!(again.buckets().len(), 1);
        assert_eq!(again.buckets()[0].probes, 2);
        assert_eq!(again.buckets()[0].up, 1);
        assert!(again.load_error.is_none());
        // entries older than retention are dropped on load
        let old = LongHistory::open(Some(&d), T0 + RETENTION_SECONDS + 120);
        assert!(old.buckets().is_empty());
        // corrupt file: ignored and reported, never trusted
        std::fs::write(d.join(FILE), "{not json").unwrap();
        let bad = LongHistory::open(Some(&d), T0);
        assert!(bad.buckets().is_empty() && bad.load_error.is_some());
        // inconsistent file (up > probes)
        std::fs::write(
            d.join(FILE),
            r#"{"v":1,"bucket_s":60,"buckets":[{"t":1700000040,"probes":1,"up":5,"rtt_sum_ms":0.0,"from_peer_bytes":0,"to_peer_bytes":0}]}"#,
        )
        .unwrap();
        assert!(LongHistory::open(Some(&d), T0 + 60).load_error.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn diagnostics_runs_are_bounded_and_persisted() {
        let d = dir("diag");
        let mut h = LongHistory::open(Some(&d), T0);
        for i in 0..(MAX_DIAG_RUNS as u64 + 5) {
            h.record_diagnostics(DiagRun {
                at_unix: T0 + i,
                highest_passed_tier: "tcp_reachability".into(),
                verified: false,
                tiers: vec![("tcp_reachability".into(), "pass".into())],
            });
        }
        let runs = h.diagnostics_runs(T0 + 200);
        assert_eq!(runs.len(), MAX_DIAG_RUNS);
        assert_eq!(runs[0].at_unix, T0 + 5, "oldest runs are dropped first");
        assert!(h.diagnostics_runs(T0 + RETENTION_SECONDS + 500).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn availability_alert_needs_enough_real_probes() {
        let mut h = LongHistory::open(None, T0);
        for i in 0..30u64 {
            h.record(T0 + i * 5, false, None, None);
        }
        assert!(
            h.alerts(T0 + 150).is_empty(),
            "too few probes: no alert, no guess"
        );
        for i in 30..120u64 {
            h.record(T0 + i * 5, i % 2 == 0, Some(1.0), None);
        }
        let a = h.alerts(T0 + 600);
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0]["id"], "availability_1h");
        assert_eq!(a[0]["evidence"]["probes"], 120);
    }
}
