//! The shared Redrob account in the shell: who is signed in (never the key), and a watcher that
//! notices the launcher writing or deleting `account.json` while the app is open.
//!
//! The contract (redrob-launcher `docs/shared-account.md`) asks apps to read the file when they
//! need it rather than once at start. The engine does exactly that on every AI request; the shell
//! additionally polls the files' modification stamps every couple of seconds so the status line
//! and the AI dock follow a sign-in or sign-out without a restart. There is no sign-in UI of this
//! app's own beyond opening Redrob Launcher.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use redrob_query_core::{Workspace, ai::AiKeyStatus};

const POLL_EVERY: Duration = Duration::from_secs(2);

pub struct AccountWatch {
    pub status: AiKeyStatus,
    stamp: Vec<(PathBuf, Option<(SystemTime, u64)>)>,
    last_poll: Instant,
}

impl AccountWatch {
    pub fn new(workspace: &Workspace) -> Self {
        Self {
            status: workspace.ai_key_status(),
            stamp: workspace.account_stamp(),
            last_poll: Instant::now(),
        }
    }

    /// Re-reads the account when a file changed. Returns `true` when the status moved, so the
    /// caller can refresh what shows it. Cheap: metadata only, every [`POLL_EVERY`].
    pub fn poll(&mut self, workspace: &Workspace) -> bool {
        if self.last_poll.elapsed() < POLL_EVERY {
            return false;
        }
        self.last_poll = Instant::now();
        let stamp = workspace.account_stamp();
        if stamp == self.stamp {
            return false;
        }
        self.stamp = stamp;
        let status = workspace.ai_key_status();
        let changed = status != self.status;
        self.status = status;
        changed
    }

    /// Where the shared file is read from, for the status tooltip.
    pub fn describe_paths(&self) -> String {
        self.stamp
            .iter()
            .map(|(path, stamp)| {
                format!(
                    "{} ({})",
                    path.display(),
                    if stamp.is_some() { "present" } else { "absent" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
