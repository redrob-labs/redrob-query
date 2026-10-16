//! The `Workspace`: the engine's surface for the desktop shell, one method per former command.
//!
//! The Tauri shell exposed `DataService` as fifteen thin commands (list / save / remove profiles and
//! secrets, load warnings, test / connect / disconnect, metadata, query, primary keys, table changes,
//! cell edits, AI status and chat). The egui shell keeps that surface, as plain Rust: each call hands
//! the work to a tokio runtime the workspace owns and returns a [`Pending`] the UI polls once per
//! frame, so a slow database never blocks a paint. Synchronous reads (load warnings, batch splitting,
//! the account status) answer at once.
//!
//! Signing in is not here: the account comes from Redrob Launcher's shared file (`crate::account`),
//! read on every request. The one thing this app does about sign-in is open the launcher's page.

use std::{
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tokio::sync::oneshot;
use uuid::Uuid;

use crate::{
    AiAssistantRequest, AiChatResponse, ConnectionProfile, ConnectionStatus, DataError,
    DataService, MetadataNode, MetadataRequest, MutationResult, QueryRequest, QueryResultPage,
    RemoveProfileOutcome, Result, SaveProfileOutcome,
    account::{self, SharedAccount},
    ai::{AiKeyStatus, build_assistant_chat_request},
    cell_edits::CellEditSet,
    ddl::TableChange,
    keys::TableKeys,
    query::read_only_statements,
};

/// A result that is on its way. Poll it once per frame; it answers exactly once.
pub struct Pending<T> {
    receiver: Option<oneshot::Receiver<Result<T>>>,
    /// The engine task behind the answer, so the shell's Esc can stop waiting for it.
    task: Option<tokio::task::AbortHandle>,
}

impl<T> Pending<T> {
    /// An already-answered pending, for paths that fail before any work is scheduled.
    pub fn ready(value: Result<T>) -> Self {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(value);
        Self {
            receiver: Some(receiver),
            task: None,
        }
    }

    /// Stop waiting: the engine task is aborted at its next await point (a driver that is
    /// mid-request finishes or times out on its own), and the next [`Self::poll`] answers with a
    /// cancellation error.
    pub fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    /// The answer, the first time it is available; `None` while the work is still running and
    /// forever after the answer was taken.
    pub fn poll(&mut self) -> Option<Result<T>> {
        let receiver = self.receiver.as_mut()?;
        match receiver.try_recv() {
            Ok(value) => {
                self.receiver = None;
                Some(value)
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.receiver = None;
                let message = if self.task.is_none() {
                    "cancelled"
                } else {
                    "the operation was dropped before it answered"
                };
                Some(Err(DataError::Unsupported(message.to_owned())))
            }
        }
    }

    /// Whether an answer is still owed.
    #[must_use]
    pub fn in_flight(&self) -> bool {
        self.receiver.is_some()
    }
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// The shell's handle on the engine.
pub struct Workspace {
    service: Arc<DataService>,
    runtime: tokio::runtime::Runtime,
    own_account_file: Option<PathBuf>,
    wake: Mutex<Option<Wake>>,
}

impl Workspace {
    /// A workspace over a persistent service: `connections.json` and `account.json` in `data_dir`,
    /// passwords in the OS keyring.
    pub fn persistent(data_dir: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let service = DataService::persistent(data_dir.join("connections.json"));
        Self::over(service, Some(data_dir.join("account.json")))
    }

    /// A workspace over an ephemeral service: nothing is written to disk.
    pub fn ephemeral() -> std::io::Result<Self> {
        Self::over(DataService::new(), None)
    }

    fn over(service: DataService, own_account_file: Option<PathBuf>) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("redrob-query-engine")
            .enable_all()
            .build()?;
        Ok(Self {
            service: Arc::new(service),
            runtime,
            own_account_file,
            wake: Mutex::new(None),
        })
    }

    /// Called when a pending answer lands, so the UI repaints without a timer.
    pub fn set_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(Arc::new(wake));
        }
    }

    fn spawn<T, F>(&self, make: impl FnOnce(Arc<DataService>) -> F) -> Pending<T>
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        let future = make(Arc::clone(&self.service));
        let wake = self.wake.lock().ok().and_then(|slot| slot.clone());
        let handle = self.runtime.spawn(async move {
            let value = future.await;
            let _ = sender.send(value);
            if let Some(wake) = wake {
                wake();
            }
        });
        Pending {
            receiver: Some(receiver),
            task: Some(handle.abort_handle()),
        }
    }

    // ---- profiles ---------------------------------------------------------------------------

    pub fn list_profiles(&self) -> Pending<Vec<ConnectionProfile>> {
        self.spawn(|service| async move { Ok(service.list_profiles().await) })
    }

    /// Warnings produced while loading stored profiles (a corrupt file, storage owned by another
    /// instance). Sanitised; safe to show.
    #[must_use]
    pub fn profile_load_warnings(&self) -> Vec<String> {
        self.service.profile_load_warnings()
    }

    /// Save a profile and, when given, its password (into the keyring, never the profile file).
    pub fn save_profile(
        &self,
        profile: ConnectionProfile,
        secret: Option<String>,
    ) -> Pending<SaveProfileOutcome> {
        self.spawn(|service| async move {
            service
                .save_profile_with_secret_outcome(profile, secret.as_deref())
                .await
        })
    }

    pub fn remove_profile(&self, id: Uuid) -> Pending<RemoveProfileOutcome> {
        self.spawn(move |service| async move { service.remove_profile_outcome(id).await })
    }

    // ---- connections ------------------------------------------------------------------------

    /// Try the profile as entered in the form, with the password typed there (or the stored one).
    pub fn test_connection(
        &self,
        profile: ConnectionProfile,
        secret: Option<String>,
    ) -> Pending<ConnectionStatus> {
        self.spawn(
            |service| async move { service.test_connection(&profile, secret.as_deref()).await },
        )
    }

    pub fn connect(&self, id: Uuid) -> Pending<ConnectionStatus> {
        self.spawn(move |service| async move { service.connect(id).await })
    }

    pub fn disconnect(&self, id: Uuid) -> Pending<()> {
        self.spawn(move |service| async move { service.disconnect(id).await })
    }

    // ---- metadata ---------------------------------------------------------------------------

    pub fn load_metadata(&self, request: MetadataRequest) -> Pending<Vec<MetadataNode>> {
        self.spawn(|service| async move { service.load_metadata(request).await })
    }

    /// The primary-key columns of a table; empty when it has none (the result stays read-only).
    pub fn primary_key_columns(
        &self,
        connection_id: Uuid,
        schema: Option<String>,
        table: String,
    ) -> Pending<Vec<String>> {
        self.spawn(move |service| async move {
            service
                .primary_key_columns(connection_id, schema, table)
                .await
        })
    }

    /// Primary key, foreign keys and indexes, for the structure panel.
    pub fn table_keys(
        &self,
        connection_id: Uuid,
        schema: Option<String>,
        table: String,
    ) -> Pending<TableKeys> {
        self.spawn(
            move |service| async move { service.table_keys(connection_id, schema, table).await },
        )
    }

    // ---- queries ----------------------------------------------------------------------------

    /// Run one statement through the engine's read-only gate (writes go through the changes panel
    /// and `apply_cell_edits`, or the table-change form and `apply_table_change`).
    pub fn execute_query(&self, request: QueryRequest) -> Pending<QueryResultPage> {
        self.spawn(|service| async move { service.execute_query(request).await })
    }

    /// The statements of a batch the editor may run one by one, each through `execute_query` and
    /// its own single-statement gate. `None` unless every statement is read-only, so a batch never
    /// widens what may run; it only lets the grid show each statement's result.
    #[must_use]
    pub fn split_read_only_batch(query: &str) -> Option<Vec<String>> {
        read_only_statements(query)
    }

    // ---- writes -----------------------------------------------------------------------------

    /// Create a table or add a column from the form. Types come from a fixed list (`ddl`).
    pub fn apply_table_change(&self, connection_id: Uuid, change: TableChange) -> Pending<()> {
        self.spawn(
            move |service| async move { service.apply_table_change(connection_id, change).await },
        )
    }

    /// Apply a change set the person reviewed in the changes panel. The engine checks the profile
    /// is writable, binds every value, and commits only if every row update hits exactly one row.
    pub fn apply_cell_edits(
        &self,
        connection_id: Uuid,
        edits: CellEditSet,
    ) -> Pending<MutationResult> {
        self.spawn(
            move |service| async move { service.apply_cell_edits(connection_id, edits).await },
        )
    }

    // ---- AI and account ---------------------------------------------------------------------

    /// Whether a Redrob account is signed in, and whose. Read fresh; never a key.
    #[must_use]
    pub fn ai_key_status(&self) -> AiKeyStatus {
        self.service.ai_key_status()
    }

    /// The account in force, by the contract's precedence (environment, own file, shared file).
    #[must_use]
    pub fn account(&self) -> Option<SharedAccount> {
        account::current_account(self.own_account_file.as_deref())
    }

    /// A fingerprint of the account files, so the shell can notice a sign-in or sign-out.
    #[must_use]
    pub fn account_stamp(&self) -> Vec<(PathBuf, Option<(std::time::SystemTime, u64)>)> {
        account::account_files_stamp(self.own_account_file.as_deref())
    }

    /// One assistant turn: the prompt and the active query, nothing else, to the console with the
    /// shared account. The request is validated before anything leaves the machine.
    pub fn ai_chat(&self, request: AiAssistantRequest) -> Pending<AiChatResponse> {
        if let Err(error) = build_assistant_chat_request(&request) {
            return Pending::ready(Err(error));
        }
        self.spawn(|service| async move { service.ai_chat(request).await })
    }

    /// Where a person signs in: the launcher's latest release page.
    #[must_use]
    pub fn launcher_url() -> &'static str {
        account::LAUNCHER_URL
    }
}
