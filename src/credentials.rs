// Credential caching helpers for the standalone AgentSwap CLI.
// Exports: credentials_path, load_api_key, save_api_key.
// Deps: dirs, std::fs, std::path, eyre.

use eyre::Result;
use std::path::PathBuf;

/// Returns the ~/.agentswap/credentials path, using $HOME if available.
pub fn credentials_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".agentswap").join("credentials"))
}

/// Loads the cached API key, trimming whitespace and ignoring empty files.
pub fn load_api_key() -> Option<String> {
    let path = credentials_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Saves the provided API key, creating directories and securing permissions on Unix.
pub fn save_api_key(api_key: &str) -> Result<()> {
    let path = credentials_path().ok_or_else(|| eyre::eyre!("cannot determine home directory"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, api_key)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
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
            let lock = HOME_LOCK.lock().expect("lock HOME mutex");
            let previous = env::var_os("HOME");
            unsafe { env::set_var("HOME", path) };
            Self {
                previous,
                _lock: lock,
            }
        }

        fn unset() -> Self {
            let lock = HOME_LOCK.lock().expect("lock HOME mutex");
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
}
