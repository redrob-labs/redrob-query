//! The Redrob account this app spends: the launcher's shared `account.json`, read on demand.
//!
//! Redrob Launcher signs a computer in once and writes the file described in its
//! `docs/shared-account.md`; every Redrob app reads it instead of running a sign-in of its own.
//! The rules for readers, as that contract states them, are the rules here:
//!
//! 1. Read, never write. Only the launcher writes the shared file.
//! 2. Read it when it is needed, not once at start: the person may sign in or out while the app
//!    is open. A missing file, unreadable JSON or an empty `apiKey` all mean "not signed in".
//! 3. Never log, display or copy the key. Only `accountName` and `apiKeyName` are shown.
//! 4. The app's own key wins: `REDROB_API_KEY` in the environment, then an `account.json` of the
//!    same shape in this app's own config directory, then the shared file.

use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    time::SystemTime,
};

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// Where a key is spent when the file names no `apiBaseUrl`.
pub const DEFAULT_API_BASE_URL: &str = "https://console.redrob.ai/api/backend/v1";
/// Replaces the shared directory on every platform (tests, portable setups).
pub const SHARED_DIR_ENV: &str = "REDROB_SHARED_DIR";
/// A key exported in a terminal wins over every file (rule 4 of the contract).
pub const OWN_KEY_ENV: &str = "REDROB_API_KEY";
/// Where a person signs in: Redrob Launcher's latest release.
pub const LAUNCHER_URL: &str = "https://github.com/redrob-labs/redrob-launcher/releases/latest";
const FILE_NAME: &str = "account.json";
/// The file is a few hundred bytes; anything larger is not the launcher's.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Which file (or variable) the account in use came from. Shown in the status line; never the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountSource {
    /// `REDROB_API_KEY` in this process's environment.
    Environment,
    /// This app's own `account.json`, written by the launcher's Connect or by hand.
    Own,
    /// The launcher's shared `account.json`.
    Launcher,
}

/// A signed-in Redrob account. `Debug` and `Display` never show the key.
#[derive(Clone)]
pub struct SharedAccount {
    pub api_key: SecretString,
    pub api_key_name: String,
    pub account_name: String,
    pub api_base_url: String,
    pub source: AccountSource,
}

impl fmt::Debug for SharedAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedAccount")
            .field("api_key", &"[redacted]")
            .field("api_key_name", &self.api_key_name)
            .field("account_name", &self.account_name)
            .field("api_base_url", &self.api_base_url)
            .field("source", &self.source)
            .finish()
    }
}

impl SharedAccount {
    /// `<apiBaseUrl>/chat/completions`, the OpenAI-compatible route the console serves.
    #[must_use]
    pub fn chat_completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.api_base_url.trim_end_matches('/')
        )
    }

    /// What the status line shows: the account, and whose key it is. Never the key.
    #[must_use]
    pub fn label(&self) -> String {
        match (self.account_name.trim(), self.api_key_name.trim()) {
            ("", "") => "Signed in".to_owned(),
            (account, "") => format!("Signed in as {account}"),
            ("", key) => format!("Signed in with {key}"),
            (account, key) => format!("{account} ({key})"),
        }
    }
}

/// The wire shape of `account.json`. Unknown fields are ignored so the launcher can grow it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountFile {
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    api_key_name: String,
    #[serde(default)]
    account_name: String,
    #[serde(default)]
    api_base_url: String,
}

/// The shared directory for `os` given an environment: `REDROB_SHARED_DIR`, else the platform
/// convention (Linux `$XDG_CONFIG_HOME/redrob-shared`, macOS
/// `~/Library/Application Support/ai.redrob.shared`, Windows `%APPDATA%\Redrob\Shared`).
#[must_use]
pub fn shared_dir_with(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env(SHARED_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = env("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    match os {
        "macos" => home.map(|h| h.join("Library/Application Support/ai.redrob.shared")),
        "windows" => env("APPDATA")
            .filter(|a| !a.is_empty())
            .map(|a| PathBuf::from(a).join("Redrob").join("Shared")),
        _ => env("XDG_CONFIG_HOME")
            .filter(|x| !x.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
            .map(|c| c.join("redrob-shared")),
    }
}

/// The shared `account.json` on this machine, or `None` when no home is known.
#[must_use]
pub fn shared_account_path() -> Option<PathBuf> {
    shared_dir_with(std::env::consts::OS, |key| std::env::var_os(key)).map(|d| d.join(FILE_NAME))
}

/// Reads one `account.json`. `None` for a missing file, unreadable JSON or an empty key.
#[must_use]
pub fn read_account_file(path: &Path, source: AccountSource) -> Option<SharedAccount> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let file: AccountFile = serde_json::from_slice(&bytes).ok()?;
    let api_key = file.api_key.trim();
    if api_key.is_empty() {
        return None;
    }
    let api_base_url = match file.api_base_url.trim() {
        "" => DEFAULT_API_BASE_URL.to_owned(),
        url => url.trim_end_matches('/').to_owned(),
    };
    Some(SharedAccount {
        api_key: SecretString::from(api_key.to_owned()),
        api_key_name: file.api_key_name.trim().to_owned(),
        account_name: file.account_name.trim().to_owned(),
        api_base_url,
        source,
    })
}

/// The account in force right now, by the contract's precedence: `REDROB_API_KEY`, then
/// `own_file` (this app's own `account.json`, when it has one), then the launcher's shared file.
#[must_use]
pub fn current_account(own_file: Option<&Path>) -> Option<SharedAccount> {
    if let Some(key) = std::env::var(OWN_KEY_ENV)
        .ok()
        .map(|k| k.trim().to_owned())
        .filter(|k| !k.is_empty())
    {
        let api_base_url = std::env::var("REDROB_CONSOLE_API")
            .ok()
            .map(|u| u.trim().trim_end_matches('/').to_owned())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| DEFAULT_API_BASE_URL.to_owned());
        return Some(SharedAccount {
            api_key: SecretString::from(key),
            api_key_name: String::new(),
            account_name: String::new(),
            api_base_url,
            source: AccountSource::Environment,
        });
    }
    if let Some(account) = own_file.and_then(|p| read_account_file(p, AccountSource::Own)) {
        return Some(account);
    }
    shared_account_path().and_then(|p| read_account_file(&p, AccountSource::Launcher))
}

/// A cheap fingerprint of the account files, so a caller can notice a sign-in or sign-out
/// without parsing: the modification time and size of each file that exists.
#[must_use]
pub fn account_files_stamp(own_file: Option<&Path>) -> Vec<(PathBuf, Option<(SystemTime, u64)>)> {
    own_file
        .map(Path::to_path_buf)
        .into_iter()
        .chain(shared_account_path())
        .map(|path| {
            let stamp = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok().map(|t| (t, m.len())));
            (path, stamp)
        })
        .collect()
}

/// `true` when the key is non-empty; a convenience for callers that only need the yes/no.
#[must_use]
pub fn has_key(account: Option<&SharedAccount>) -> bool {
    account.is_some_and(|a| !a.api_key.expose_secret().is_empty())
}
