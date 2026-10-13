//! Redrob Launcher's shared account file.
//!
//! The launcher signs a computer in to a Redrob workspace once and writes the workspace key to a
//! file every Redrob app may read (redrob-launcher `docs/shared-account.md`). This app reads that
//! file as the fallback when it has no key of its own, fresh on every use, and never writes it.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use secrecy::SecretString;
use serde::Deserialize;

/// Overrides the shared account file's directory on every platform (tests, portable setups).
pub const SHARED_DIR_ENV: &str = "REDROB_SHARED_DIR";

/// The workspace the computer is signed in to, as the launcher wrote it. The key is a secret;
/// `account_name` and `api_key_name` are the only fields meant for display.
pub struct SharedAccount {
    pub api_key: SecretString,
    pub api_key_name: String,
    pub account_name: String,
    /// Where the key is spent, without a trailing slash; empty when the launcher recorded none.
    pub api_base_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wire {
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    api_key_name: String,
    #[serde(default)]
    account_name: String,
    #[serde(default)]
    api_base_url: String,
}

/// The shared account file's path for this platform, or `None` when the platform offers no
/// per-user configuration directory.
#[must_use]
pub fn shared_account_path() -> Option<PathBuf> {
    shared_account_path_with(|key| std::env::var_os(key))
}

/// [`shared_account_path`] over the given environment, so the platform rules can be checked
/// without touching the process environment.
pub fn shared_account_path_with(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let env = |key: &str| {
        var(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let dir = if let Some(dir) = env(SHARED_DIR_ENV) {
        dir
    } else if cfg!(windows) {
        env("APPDATA")?.join("Redrob").join("Shared")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/ai.redrob.shared")
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|home| home.join(".config")))?
            .join("redrob-shared")
    };
    Some(dir.join("account.json"))
}

/// Reads the shared account now. A missing file, unreadable JSON or an empty key all mean the
/// computer is not signed in through the launcher.
#[must_use]
pub fn read_shared_account() -> Option<SharedAccount> {
    read_shared_account_at(&shared_account_path()?)
}

/// [`read_shared_account`] for a file at a known path.
#[must_use]
pub fn read_shared_account_at(path: &Path) -> Option<SharedAccount> {
    let bytes = std::fs::read(path).ok()?;
    let wire: Wire = serde_json::from_slice(&bytes).ok()?;
    if wire.api_key.trim().is_empty() {
        return None;
    }
    Some(SharedAccount {
        api_key: SecretString::from(wire.api_key),
        api_key_name: wire.api_key_name,
        account_name: wire.account_name,
        api_base_url: wire.api_base_url.trim().trim_end_matches('/').to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use secrecy::ExposeSecret;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn redrob_shared_dir_replaces_the_directory_on_every_platform() {
        let path = shared_account_path_with(env(&[
            ("REDROB_SHARED_DIR", "/portable/shared"),
            ("HOME", "/home/someone"),
            ("APPDATA", "C:\\Users\\someone\\AppData\\Roaming"),
        ]))
        .unwrap();
        assert_eq!(path, PathBuf::from("/portable/shared").join("account.json"));
        let empty_is_unset = shared_account_path_with(env(&[("REDROB_SHARED_DIR", "")]));
        assert_ne!(
            empty_is_unset,
            Some(PathBuf::from("").join("account.json")),
            "an empty override is ignored like an unset one"
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_uses_xdg_config_home_then_dot_config() {
        let xdg = shared_account_path_with(env(&[
            ("XDG_CONFIG_HOME", "/xdg"),
            ("HOME", "/home/someone"),
        ]))
        .unwrap();
        assert_eq!(xdg, PathBuf::from("/xdg/redrob-shared/account.json"));
        let home = shared_account_path_with(env(&[("HOME", "/home/someone")])).unwrap();
        assert_eq!(
            home,
            PathBuf::from("/home/someone/.config/redrob-shared/account.json")
        );
        assert!(shared_account_path_with(env(&[])).is_none());
    }

    #[test]
    fn missing_malformed_or_empty_key_means_not_signed_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("account.json");
        assert!(read_shared_account_at(&path).is_none(), "missing file");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(read_shared_account_at(&path).is_none(), "malformed file");
        std::fs::write(&path, br#"{"apiKey":"  ","apiKeyName":"Redrob Launcher"}"#).unwrap();
        assert!(read_shared_account_at(&path).is_none(), "blank key");
        std::fs::write(&path, b"[]").unwrap();
        assert!(read_shared_account_at(&path).is_none(), "wrong shape");
    }

    #[test]
    fn reads_the_launcher_file_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("account.json");
        std::fs::write(
            &path,
            br#"{"apiKey":"rk_shared","apiKeyName":"Redrob Launcher","accountName":"Acme","apiBaseUrl":"https://console.redrob.ai/api/backend/v1/"}"#,
        )
        .unwrap();
        let account = read_shared_account_at(&path).unwrap();
        assert_eq!(account.api_key.expose_secret(), "rk_shared");
        assert_eq!(account.api_key_name, "Redrob Launcher");
        assert_eq!(account.account_name, "Acme");
        assert_eq!(
            account.api_base_url,
            "https://console.redrob.ai/api/backend/v1"
        );
        // Only the key is required; the launcher may omit the rest.
        std::fs::write(&path, br#"{"apiKey":"rk_only"}"#).unwrap();
        let account = read_shared_account_at(&path).unwrap();
        assert!(account.api_base_url.is_empty() && account.account_name.is_empty());
    }
}
