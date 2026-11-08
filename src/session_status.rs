//! Observable session indicator (ADR-0022, P0-2). While at least one *authenticated and authorized*
//! session exists, the running unit writes `$RUNTIME_DIRECTORY/session.json`; it is removed when the
//! last session ends. The agent reads it to tell "listener is up" apart from "a peer session was
//! authorized". It says nothing about data integrity: that stays Unknown until a data-path probe
//! observes it. The file holds no secrets: tunnel id, direction, counts and the process id.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ACTIVE: Mutex<u32> = Mutex::new(0);

pub const FILE: &str = "session.json";

fn dir() -> Option<PathBuf> {
    std::env::var_os("RUNTIME_DIRECTORY")
        .map(|v| {
            PathBuf::from(
                v.to_string_lossy()
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .filter(|p| p.is_absolute() && p.is_dir())
}

fn write(dir: &Path, tunnel: &str, direction: &str, count: u32) {
    use std::os::unix::fs::PermissionsExt;
    let body = serde_json::json!({
        "state": "authorized",
        "authorized_sessions": count,
        "tunnel_id": tunnel,
        "direction": direction,
        "pid": std::process::id(),
        "since_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    let tmp = dir.join(format!(".{FILE}.tmp"));
    if std::fs::write(&tmp, body.to_string()).is_ok() {
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644));
        let _ = std::fs::rename(&tmp, dir.join(FILE));
    }
}

/// Held for the lifetime of one authorized session.
pub struct SessionMark {
    tunnel: String,
    direction: String,
}

impl SessionMark {
    pub fn open(tunnel: &str, direction: impl std::fmt::Debug) -> Self {
        let direction = format!("{direction:?}").to_lowercase();
        let mut n = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        *n += 1;
        if let Some(d) = dir() {
            write(&d, tunnel, &direction, *n);
        }
        Self {
            tunnel: tunnel.to_owned(),
            direction,
        }
    }
}

impl Drop for SessionMark {
    fn drop(&mut self) {
        let mut n = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        *n = n.saturating_sub(1);
        if let Some(d) = dir() {
            if *n == 0 {
                let _ = std::fs::remove_file(d.join(FILE));
            } else {
                write(&d, &self.tunnel, &self.direction, *n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_open_and_clear_the_file() {
        let d = std::env::temp_dir().join(format!("baft-sess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // SAFETY: single test touching this variable in this module.
        unsafe { std::env::set_var("RUNTIME_DIRECTORY", &d) };
        let a = SessionMark::open("t1", "Direct");
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v["state"], "authorized");
        assert_eq!(v["authorized_sessions"], 1);
        assert_eq!(v["pid"], std::process::id());
        let b = SessionMark::open("t1", "Direct");
        drop(a);
        assert!(d.join(FILE).exists());
        drop(b);
        assert!(!d.join(FILE).exists());
        unsafe { std::env::remove_var("RUNTIME_DIRECTORY") };
    }
}
