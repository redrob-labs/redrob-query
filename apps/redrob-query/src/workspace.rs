//! Workspace persistence: the open query tabs and the run history, in `workspace.json` under the
//! config directory. At most [`MAX_TABS`] tabs and [`MAX_HISTORY`] history entries are kept; with
//! `REDROB_QUERY_NO_PREFS` set nothing is read or written. Written atomically (temp file, rename).

use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::services;

pub const MAX_TABS: usize = 30;
pub const MAX_HISTORY: usize = 50;
/// A tab's text past this is not persisted (the tab is kept, truncated); the editor is for
/// queries, not for pasting a dump.
const MAX_TAB_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedTab {
    pub title: String,
    pub text: String,
    pub connection_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryEntry {
    pub connection_id: Option<Uuid>,
    pub connection_name: String,
    pub query: String,
    /// RFC 3339, local time is not needed: the list shows relative order.
    pub ran_at_unix: u64,
    pub rows: usize,
    pub elapsed_ms: u64,
    pub ok: bool,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            connection_id: None,
            connection_name: String::new(),
            query: String::new(),
            ran_at_unix: 0,
            rows: 0,
            elapsed_ms: 0,
            ok: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceFile {
    pub schema: u32,
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
    pub history: Vec<HistoryEntry>,
    /// The AI dock and the structure panel are remembered open or closed.
    pub ai_dock_open: bool,
    pub structure_open: bool,
    /// Help > Theme: `system`, `light` or `dark`.
    pub theme: String,
}

impl WorkspaceFile {
    pub fn load() -> Option<Self> {
        let path = services::workspace_path()?;
        let bytes = std::fs::read(path).ok()?;
        let mut file: Self = serde_json::from_slice(&bytes).ok()?;
        file.clamp();
        Some(file)
    }

    /// Writes the file; a failure is logged, not shown -- losing the tab list is not worth a dialog.
    pub fn save(&self) {
        let Some(path) = services::workspace_path() else {
            return;
        };
        let mut copy = self.clone();
        copy.schema = 1;
        copy.clamp();
        if let Err(error) = write_atomic(&path, &copy) {
            log::warn!("workspace not saved: {error}");
        }
    }

    fn clamp(&mut self) {
        if self.tabs.len() > MAX_TABS {
            self.tabs.truncate(MAX_TABS);
        }
        for tab in &mut self.tabs {
            if tab.text.len() > MAX_TAB_BYTES {
                let mut end = MAX_TAB_BYTES;
                while !tab.text.is_char_boundary(end) {
                    end -= 1;
                }
                tab.text.truncate(end);
            }
        }
        if self.history.len() > MAX_HISTORY {
            let drop = self.history.len() - MAX_HISTORY;
            self.history.drain(..drop);
        }
        if self.active_tab >= self.tabs.len() {
            self.active_tab = 0;
        }
    }
}

fn write_atomic(path: &Path, file: &WorkspaceFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path)
}

/// Seconds since the Unix epoch, for history stamps.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
