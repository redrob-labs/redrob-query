//! Where Redrob Query keeps its per-user data, and the names every platform shows for it.
//!
//! One config directory for the whole app (see [`config_dir`]): `connections.json` (profiles, no
//! secrets; the engine's own file), `account.json` (this app's own sign-in, if the launcher's
//! Connect wrote one), `workspace.json` (open tabs and history) and eframe's `ui.ron` (window
//! geometry, panel sizes). Passwords are in the OS keyring, never here.

use std::ffi::OsString;
use std::path::PathBuf;

/// Desktop app ID: the Linux `.desktop` file name, hicolor icon name and Wayland app id.
pub const APP_ID: &str = "ai.redrob.query";
/// The window title and the brand text in the top bar.
pub const APP_NAME: &str = "Redrob Query";
/// `REDROB_QUERY_CONFIG_DIR` overrides the platform directory (tests, agents, custom setups).
pub const CONFIG_DIR_ENV: &str = "REDROB_QUERY_CONFIG_DIR";
/// `REDROB_QUERY_NO_PREFS`: read and write no workspace or window state (tests, demos). Profiles
/// are still stored: they are the engine's, and a demo without them would have nothing to connect.
pub const NO_PREFS_ENV: &str = "REDROB_QUERY_NO_PREFS";

/// Per-user settings directory: `REDROB_QUERY_CONFIG_DIR`, else the platform convention
/// (macOS `~/Library/Application Support/Redrob Query`, Windows `%APPDATA%\Redrob Query`,
/// elsewhere `$XDG_CONFIG_HOME/redrob-query` or `~/.config/redrob-query`).
pub fn config_dir() -> Option<PathBuf> {
    config_dir_with(std::env::consts::OS, |key| std::env::var_os(key))
}

/// [`config_dir`] from an explicit environment, so tests can fake `HOME` / `APPDATA`.
pub fn config_dir_with(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(root) = env(CONFIG_DIR_ENV) {
        // An explicitly empty override must not fall back to the user's profile.
        return (!root.is_empty()).then(|| PathBuf::from(root));
    }
    let home = env("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    match os {
        "macos" => home.map(|h| h.join("Library/Application Support").join(APP_NAME)),
        "windows" => env("APPDATA")
            .filter(|a| !a.is_empty())
            .map(|a| PathBuf::from(a).join(APP_NAME)),
        _ => env("XDG_CONFIG_HOME")
            .filter(|x| !x.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
            .map(|c| c.join("redrob-query")),
    }
}

/// Runs without preferences neither read nor write the workspace or window state.
pub fn prefs_enabled() -> bool {
    std::env::var_os(NO_PREFS_ENV).is_none()
}

/// eframe's own persistence file (window geometry, panel sizes).
pub fn persistence_path() -> Option<PathBuf> {
    config_dir()
        .filter(|_| prefs_enabled())
        .map(|d| d.join("ui.ron"))
}

/// The open tabs and the query history (`workspace.rs`).
pub fn workspace_path() -> Option<PathBuf> {
    config_dir()
        .filter(|_| prefs_enabled())
        .map(|d| d.join("workspace.json"))
}
