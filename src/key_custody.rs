use std::{
    env, fs,
    path::{Component, PathBuf},
};

use anyhow::{Context, Result, bail};

const MAX_CREDENTIAL_BYTES: u64 = 16 * 1024;

/// Resolve a systemd-provided credential without accepting arbitrary paths.
/// The returned path remains inside the service-scoped credential directory.
pub fn systemd_credential_path(identifier: &str) -> Result<PathBuf> {
    if identifier.is_empty()
        || identifier.len() > 128
        || !identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        || identifier == "."
        || identifier == ".."
    {
        bail!("invalid systemd credential identifier");
    }

    let directory = env::var_os("CREDENTIALS_DIRECTORY").context(
        "CREDENTIALS_DIRECTORY is not set; systemd credential mode requires systemd delivery",
    )?;
    let directory = PathBuf::from(directory);
    let canonical_directory =
        fs::canonicalize(&directory).context("resolve systemd credential directory")?;

    let candidate = directory.join(identifier);
    let metadata = fs::symlink_metadata(&candidate).context("inspect systemd credential file")?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        bail!("systemd credential must be a regular non-symlink file");
    }
    if metadata.len() == 0 || metadata.len() > MAX_CREDENTIAL_BYTES {
        bail!("systemd credential size is outside the permitted range");
    }

    let canonical_file = fs::canonicalize(&candidate).context("resolve systemd credential file")?;
    if canonical_file.parent() != Some(canonical_directory.as_path()) {
        bail!("systemd credential resolves outside its credential directory");
    }
    if canonical_file
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        bail!("invalid systemd credential path");
    }
    Ok(canonical_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "baft-key-credential-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn credential_identifier_is_strict_and_file_must_be_regular_bounded_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let directory = temp_dir();
        fs::write(directory.join("node-key"), b"not-a-real-key-but-nonempty").unwrap();
        unsafe { env::set_var("CREDENTIALS_DIRECTORY", &directory) };
        assert_eq!(
            systemd_credential_path("node-key").unwrap(),
            fs::canonicalize(directory.join("node-key")).unwrap()
        );
        assert!(systemd_credential_path("../escape").is_err());
        assert!(systemd_credential_path("nested/key").is_err());
        assert!(systemd_credential_path("").is_err());
        fs::remove_dir_all(directory).unwrap();
        unsafe { env::remove_var("CREDENTIALS_DIRECTORY") };
    }

    #[test]
    fn missing_empty_oversized_and_non_regular_credentials_are_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        let directory = temp_dir();
        unsafe { env::set_var("CREDENTIALS_DIRECTORY", &directory) };

        assert!(systemd_credential_path("missing-key").is_err());

        fs::write(directory.join("empty-key"), b"").unwrap();
        assert!(systemd_credential_path("empty-key").is_err());

        fs::write(
            directory.join("oversized-key"),
            vec![b'K'; (MAX_CREDENTIAL_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(systemd_credential_path("oversized-key").is_err());

        fs::create_dir(directory.join("directory-key")).unwrap();
        assert!(systemd_credential_path("directory-key").is_err());

        fs::remove_dir_all(directory).unwrap();
        unsafe { env::remove_var("CREDENTIALS_DIRECTORY") };
    }

    #[cfg(unix)]
    #[test]
    fn credential_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;
        let _guard = ENV_LOCK.lock().unwrap();
        let directory = temp_dir();
        let outside = directory.with_extension("outside");
        fs::write(&outside, b"secret").unwrap();
        symlink(&outside, directory.join("node-key")).unwrap();
        unsafe { env::set_var("CREDENTIALS_DIRECTORY", &directory) };
        assert!(systemd_credential_path("node-key").is_err());
        fs::remove_dir_all(directory).unwrap();
        fs::remove_file(outside).unwrap();
        unsafe { env::remove_var("CREDENTIALS_DIRECTORY") };
    }

    #[test]
    fn missing_credentials_directory_fails_closed() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { env::remove_var("CREDENTIALS_DIRECTORY") };
        assert!(systemd_credential_path("node-key").is_err());
    }
}
