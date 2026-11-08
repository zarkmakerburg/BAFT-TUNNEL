//! Local store for pinned engine binaries: install, verify, upgrade and rollback.
//!
//! Layout under `<store>/<engine id>/`:
//!   `releases/<artifact sha256>/engine`        the binary (mode 0755)
//!   `releases/<artifact sha256>/manifest.json` the manifest it was installed against
//!   `current`  -> `releases/<sha>`             what the launcher runs
//!   `previous` -> `releases/<sha>`             what `rollback` returns to
//!
//! Nothing is downloaded: the binary comes from a signed release the operator already
//! holds. Every switch re-verifies the target against its own stored manifest first, so a
//! tampered or truncated release is refused and `current` is left untouched.

use crate::runtime_host::{Manifest, verify_artifact};
use anyhow::{Context, Result, bail};
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const PLATFORM: &str = "linux-amd64";

fn engine_dir(store: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id.len() > 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("invalid engine id {id:?}");
    }
    Ok(store.join(id))
}

fn load_manifest(path: &Path, id: &str) -> Result<crate::runtime_host::EngineRuntime> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let manifest = Manifest::parse(&text)?;
    let engine = manifest
        .engine(id)
        .with_context(|| format!("engine {id} is not in the manifest"))?
        .clone();
    if !engine.platforms.iter().any(|p| p == PLATFORM)
        || !cfg!(all(target_os = "linux", target_arch = "x86_64"))
    {
        bail!("engine {id} has no build for this platform ({PLATFORM})");
    }
    Ok(engine)
}

/// Check a binary against a manifest without touching any store.
pub fn check(manifest: &Path, id: &str, binary: &Path) -> Result<String> {
    let engine = load_manifest(manifest, id)?;
    verify_artifact(binary, &engine.artifact_sha256)?;
    Ok(engine.artifact_sha256)
}

fn read_link_name(link: &Path) -> Option<String> {
    fs::read_link(link)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
}

fn set_link(dir: &Path, name: &str, sha: &str) -> Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let _ = fs::remove_file(&tmp);
    symlink(Path::new("releases").join(sha), &tmp)?;
    fs::rename(&tmp, dir.join(name)).with_context(|| format!("switch {name}"))
}

/// Verify one stored release against the manifest stored beside it.
fn verify_release(dir: &Path, id: &str, sha: &str) -> Result<PathBuf> {
    let rel = dir.join("releases").join(sha);
    let engine = load_manifest(&rel.join("manifest.json"), id)?;
    if !engine.artifact_sha256.eq_ignore_ascii_case(sha) {
        bail!("stored manifest does not describe release {sha}");
    }
    let bin = rel.join("engine");
    verify_artifact(&bin, &engine.artifact_sha256)?;
    Ok(bin)
}

/// Install a verified binary and make it `current`; the old `current` becomes `previous`.
pub fn install(store: &Path, manifest: &Path, id: &str, binary: &Path) -> Result<String> {
    let sha = check(manifest, id, binary)?;
    let sha = sha.to_ascii_lowercase();
    let dir = engine_dir(store, id)?;
    let rel = dir.join("releases").join(&sha);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir.join("releases"))?;
    if !rel.join("engine").exists() {
        let tmp = dir.join("releases").join(format!(".{sha}.tmp"));
        let _ = fs::remove_dir_all(&tmp);
        fs::DirBuilder::new().mode(0o700).create(&tmp)?;
        fs::copy(binary, tmp.join("engine")).context("copy engine")?;
        fs::set_permissions(tmp.join("engine"), fs::Permissions::from_mode(0o755))?;
        fs::copy(manifest, tmp.join("manifest.json")).context("copy manifest")?;
        fs::rename(&tmp, &rel).context("publish release directory")?;
    }
    // Re-verify what is now on disk, not what we were handed.
    verify_release(&dir, id, &sha)?;
    let current = read_link_name(&dir.join("current"));
    if current.as_deref() == Some(sha.as_str()) {
        return Ok(sha);
    }
    if let Some(old) = current {
        set_link(&dir, "previous", &old)?;
    }
    set_link(&dir, "current", &sha)?;
    Ok(sha)
}

/// Verify the installed `current` release; returns the path the launcher should run.
pub fn verify_current(store: &Path, id: &str) -> Result<PathBuf> {
    let dir = engine_dir(store, id)?;
    let sha = read_link_name(&dir.join("current")).context("no current engine installed")?;
    verify_release(&dir, id, &sha)
}

/// Switch back to `previous` after verifying it; `current` becomes the new `previous`.
pub fn rollback(store: &Path, id: &str) -> Result<String> {
    let dir = engine_dir(store, id)?;
    let prev =
        read_link_name(&dir.join("previous")).context("no previous engine to roll back to")?;
    verify_release(&dir, id, &prev)?;
    if let Some(cur) = read_link_name(&dir.join("current")) {
        set_link(&dir, "previous", &cur)?;
    }
    set_link(&dir, "current", &prev)?;
    Ok(prev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("engine-store-{:016x}", rand::random::<u64>()));
            fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn manifest_for(bytes: &[u8]) -> String {
        let sha = hex::encode(Sha256::digest(bytes));
        format!(
            r#"{{"schema_version":1,"engines":[{{"id":"demo","runtime_language":"go","version":"1","source_repository":"https://example.invalid/demo","source_commit":"{}","artifact":"engines/demo","artifact_sha256":"{sha}","platforms":["linux-amd64"],"config_adapter":"demo-v1","required_capabilities":[],"readiness":{{"kind":"tcp_listen","timeout_seconds":5}},"resources":{{"max_restarts":3,"restart_window_seconds":60,"max_memory_mib":64}},"license":{{"spdx":"MIT","source_url":"https://example.invalid/demo"}}}}]}}"#,
            "a".repeat(40)
        )
    }

    fn release(t: &Tmp, name: &str, bytes: &[u8]) -> (PathBuf, PathBuf) {
        let bin = t.0.join(format!("{name}.bin"));
        fs::write(&bin, bytes).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        let man = t.0.join(format!("{name}.json"));
        fs::write(&man, manifest_for(bytes)).unwrap();
        (man, bin)
    }

    #[test]
    fn install_upgrade_rollback_and_negative_probes() {
        let t = Tmp::new();
        let store = t.0.join("store");
        let (m1, b1) = release(&t, "v1", b"engine one");
        let (m2, b2) = release(&t, "v2", b"engine two");

        let sha1 = install(&store, &m1, "demo", &b1).unwrap();
        assert_eq!(
            verify_current(&store, "demo").unwrap().file_name().unwrap(),
            "engine"
        );
        assert!(
            rollback(&store, "demo").is_err(),
            "nothing to roll back to yet"
        );

        let sha2 = install(&store, &m2, "demo", &b2).unwrap();
        assert_ne!(sha1, sha2);
        let dir = store.join("demo");
        assert_eq!(read_link_name(&dir.join("current")).unwrap(), sha2);
        assert_eq!(read_link_name(&dir.join("previous")).unwrap(), sha1);
        // Reinstalling the active release changes nothing.
        install(&store, &m2, "demo", &b2).unwrap();
        assert_eq!(read_link_name(&dir.join("previous")).unwrap(), sha1);

        // Roll back, then forward again by rolling back once more.
        assert_eq!(rollback(&store, "demo").unwrap(), sha1);
        assert_eq!(read_link_name(&dir.join("current")).unwrap(), sha1);
        assert_eq!(rollback(&store, "demo").unwrap(), sha2);

        // A tampered stored binary is refused and current is not switched to it.
        let victim = dir.join("releases").join(&sha1).join("engine");
        fs::write(&victim, b"tampered").unwrap();
        assert!(rollback(&store, "demo").is_err());
        assert_eq!(read_link_name(&dir.join("current")).unwrap(), sha2);
        // ...and verifying a tampered current fails too.
        fs::write(dir.join("releases").join(&sha2).join("engine"), b"tampered").unwrap();
        assert!(verify_current(&store, "demo").is_err());
    }

    #[test]
    fn wrong_hash_wrong_id_and_writable_binary_are_refused() {
        let t = Tmp::new();
        let store = t.0.join("store");
        let (m, b) = release(&t, "v1", b"engine one");
        // Binary that does not match the manifest.
        fs::write(&b, b"something else").unwrap();
        assert!(install(&store, &m, "demo", &b).is_err());
        assert!(!store.join("demo").join("current").exists());
        // Unknown engine id and unsafe id.
        fs::write(&b, b"engine one").unwrap();
        assert!(install(&store, &m, "nope", &b).is_err());
        assert!(install(&store, &m, "../x", &b).is_err());
        // Group/other-writable binary.
        fs::set_permissions(&b, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(check(&m, "demo", &b).is_err());
        fs::set_permissions(&b, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check(&m, "demo", &b).is_ok());
    }
}
