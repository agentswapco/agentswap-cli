// Credential caching helpers for the standalone AgentSwap CLI.
// Exports: credentials_path, load_api_key, save_api_key.
// Deps: std file creation/rename, getrandom, zeroize, eyre.

use eyre::Result;
use std::path::PathBuf;
use std::io::{Read, Write};
use zeroize::Zeroizing;

/// Returns the ~/.agentswap/credentials path, using $HOME if available.
pub fn credentials_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(home).join(".agentswap").join("credentials"))
}

/// Loads the cached API key, trimming whitespace and ignoring empty files.
pub fn load_api_key() -> Option<String> {
    let path = credentials_path()?;
    let mut content = Zeroizing::new(String::new());
    std::fs::File::open(&path).ok()?.read_to_string(&mut content).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Atomically replaces the cache with a file private from creation on Unix.
pub fn save_api_key(api_key: &str) -> Result<()> {
    let path = credentials_path().ok_or_else(|| eyre::eyre!("cannot determine home directory"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| eyre::eyre!("credential nonce failed: {e}"))?;
    let temporary = path.with_file_name(format!(".credentials-{}.tmp", hex::encode(nonce)));
    let mut file = create_private_file(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(api_key.as_bytes())?;
        file.sync_all()?;
        // Rename replaces the directory entry, never the target of a symlink/hard link.
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn create_private_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::{
        env,
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
    };

    // Serialize tests that mutate the HOME env var.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    struct HomeGuard {
        previous: Option<OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let lock = HOME_LOCK.lock().unwrap_or_else(|error| error.into_inner());
            let previous = env::var_os("HOME");
            unsafe { env::set_var("HOME", path) };
            Self {
                previous,
                _lock: lock,
            }
        }

        fn unset() -> Self {
            let lock = HOME_LOCK.lock().unwrap_or_else(|error| error.into_inner());
            let previous = env::var_os("HOME");
            unsafe { env::remove_var("HOME") };
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.take() {
                unsafe { env::set_var("HOME", value) };
            } else {
                unsafe { env::remove_var("HOME") };
            }
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        env::temp_dir().join(format!("sr_cred_test_{prefix}_{}", std::process::id()))
    }

    #[test]
    fn save_and_load_api_key_roundtrip() {
        let temp_dir = unique_temp_dir("roundtrip");
        let _guard = HomeGuard::set(&temp_dir);
        save_api_key("round-trip-key").expect("failed to save api key");
        assert_eq!(load_api_key(), Some("round-trip-key".to_string()));
    }

    #[test]
    fn load_api_key_missing_file_returns_none() {
        let temp_dir = unique_temp_dir("missing");
        let _guard = HomeGuard::set(&temp_dir);
        assert!(load_api_key().is_none());
    }

    #[test]
    fn load_api_key_empty_file_returns_none() {
        let temp_dir = unique_temp_dir("empty");
        let _guard = HomeGuard::set(&temp_dir);
        let credentials_dir = temp_dir.join(".agentswap");
        fs::create_dir_all(&credentials_dir).expect("create credentials directory");
        fs::write(credentials_dir.join("credentials"), "   ").expect("write empty file");
        assert!(load_api_key().is_none());
    }

    #[test]
    fn credentials_path_none_without_home() {
        let _guard = HomeGuard::unset();
        assert!(credentials_path().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn save_replaces_inode_without_exposing_new_key_through_old_links() {
        use std::os::unix::fs::PermissionsExt;
        let dir = unique_temp_dir("atomic");
        let _guard = HomeGuard::set(&dir);
        fs::create_dir_all(dir.join(".agentswap")).unwrap();
        let path = credentials_path().unwrap();
        fs::write(&path, "old-inert-canary").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let link = dir.join("old-link");
        fs::hard_link(&path, &link).unwrap();
        save_api_key("new-inert-canary").unwrap();
        assert!(fs::read_to_string(link).unwrap() == "old-inert-canary", "old inode was clobbered");
        assert!(load_api_key().as_deref() == Some("new-inert-canary"));
        assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_does_not_clobber_symlink_target() {
        use std::os::unix::fs::symlink;
        let dir = unique_temp_dir("symlink");
        let _guard = HomeGuard::set(&dir);
        fs::create_dir_all(dir.join(".agentswap")).unwrap();
        let target = dir.join("unrelated");
        fs::write(&target, "unrelated-inert-canary").unwrap();
        symlink(&target, credentials_path().unwrap()).unwrap();
        save_api_key("new-inert-canary").unwrap();
        assert!(fs::read_to_string(target).unwrap() == "unrelated-inert-canary", "symlink target was clobbered");
        assert!(!fs::symlink_metadata(credentials_path().unwrap()).unwrap().file_type().is_symlink());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn new_credential_file_is_private_and_failed_save_leaves_no_temporary_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = unique_temp_dir("private");
        let _guard = HomeGuard::set(&dir);
        save_api_key("inert-canary").unwrap();
        let path = credentials_path().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(save_api_key("inert-canary").is_err());
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_creation_precedes_first_write_and_refuses_existing_paths() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = unique_temp_dir("creation");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("new");
        let mut file = create_private_file(&path).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(file.metadata().unwrap().len(), 0);
        file.write_all(b"inert-canary").unwrap();
        assert!(create_private_file(&path).is_err());
        let link = dir.join("link");
        symlink(&path, &link).unwrap();
        assert!(create_private_file(&link).is_err());
        assert!(fs::read_to_string(&path).unwrap() == "inert-canary");
        fs::remove_dir_all(dir).unwrap();
    }
}
