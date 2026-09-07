//! Profile registry, live connection lifecycle, and database operations.

use std::{collections::HashMap, net::IpAddr, path::PathBuf, sync::Arc, time::Instant};

use base64::Engine as _;
use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat, Utc};
use futures_util::TryStreamExt;
use mongodb::{
    Client as MongoClient,
    bson::{Bson, Document, doc},
};
use serde::Deserialize;
use sqlx::{
    AssertSqlSafe, Column, Encode, MySql, MySqlPool, PgPool, Postgres, Row, Sqlite, SqlitePool,
    Type, TypeInfo, ValueRef,
    encode::IsNull,
    mysql::{MySqlArguments, MySqlPoolOptions, MySqlRow, types::MySqlTime},
    postgres::{PgArgumentBuffer, PgArguments, PgPoolOptions, PgRow, PgTypeInfo, types::Oid},
    query::Query,
    sqlite::{SqliteArguments, SqlitePoolOptions, SqliteRow},
};
use tokio::{
    sync::{Mutex, RwLock},
    time::timeout,
};
use url::Url;
use uuid::Uuid;

use crate::{
    AiAssistantRequest, AiChatResponse, ConnectionConfig, ConnectionProfile, ConnectionState,
    ConnectionStatus, DEMO_PROFILE_ID, DataError, DataValue, DatabaseKind, MetadataKind,
    MetadataNode, MetadataRequest, MutationPlan, MutationResult, MutationRisk, QueryClassification,
    QueryColumn, QueryLanguage, QueryRequest, QueryResultPage, QueryStats, Result,
    ai::{AI_SECRET_ID, AiClient, build_assistant_chat_request},
    profile_store::{
        JournalOperation, JournalPhase, PersistencePhase, ProfileJournal, ProfileRepository,
        RepositoryOpenError, deserialize_journal, profiles_from_map, reconcile, remove_journal,
        replace_journal, replace_target_profiles, repository, serialize_profiles,
    },
    query::{classify_sql, is_single_read_only_statement, risk_for},
    secret::{FallbackSecretStore, KeyringSecretStore, SecretStore, exposed},
};

const PROFILE_STORAGE_OWNED_WARNING: &str = "Connection profiles are unavailable because another Redrob Data instance owns profile storage; profile and keyring changes and connections are disabled.";
const PROFILE_STORAGE_UNAVAILABLE_WARNING: &str = "Connection profile storage is unavailable; stored profiles are unavailable and profile and keyring changes and connections are disabled.";

#[derive(Clone)]
enum ActiveConnection {
    PostgreSql(PgPool),
    MySql(MySqlPool),
    SQLite(SqlitePool),
    Mongo {
        client: MongoClient,
        database: String,
    },
}

impl ActiveConnection {
    async fn close(self) {
        match self {
            Self::PostgreSql(pool) => pool.close().await,
            Self::MySql(pool) => pool.close().await,
            Self::SQLite(pool) => pool.close().await,
            Self::Mongo { .. } => {}
        }
    }
}

struct Approval {
    plan_id: Uuid,
    connection_id: Uuid,
    statement: String,
    parameters: Vec<DataValue>,
    risk: MutationRisk,
    expires_at: DateTime<Utc>,
}

/// Thread-safe backend service managed by Tauri.
pub struct DataService {
    profiles: RwLock<HashMap<Uuid, ConnectionProfile>>,
    active: RwLock<HashMap<Uuid, ActiveConnection>>,
    approvals: RwLock<HashMap<String, Approval>>,
    connection_lock: Mutex<()>,
    profile_store_lock: Mutex<()>,
    profile_repository: Option<Arc<dyn ProfileRepository>>,
    profile_load_warnings: Vec<String>,
    recovery_blocked: bool,
    persistent_storage_unavailable: bool,
    secrets: Arc<dyn SecretStore>,
    ai: AiClient,
}

impl Default for DataService {
    fn default() -> Self {
        Self::new()
    }
}

impl DataService {
    /// Creates an ephemeral service. Connection profiles are not written to disk.
    #[must_use]
    pub fn new() -> Self {
        Self::with_secret_store(Arc::new(FallbackSecretStore::default()))
    }

    /// Creates a service backed by an atomic, non-secret JSON profile file and
    /// the durable OS credential store. An exclusive OS lock is acquired before
    /// reconciliation and retained for the service lifetime. Lock contention
    /// produces a Demo-only, fail-closed service.
    #[must_use]
    pub fn persistent(path: impl Into<PathBuf>) -> Self {
        Self::build_persistent(Arc::new(KeyringSecretStore), path.into())
    }

    /// Creates an ephemeral service with an injectable secret store for tests.
    #[must_use]
    pub fn with_secret_store(secrets: Arc<dyn SecretStore>) -> Self {
        Self::build(secrets, None, None)
    }

    fn build_persistent(secrets: Arc<dyn SecretStore>, path: PathBuf) -> Self {
        match repository(path) {
            Ok(repository) => Self::build(secrets, Some(repository), None),
            Err(RepositoryOpenError::InUse) => {
                Self::build(secrets, None, Some(PROFILE_STORAGE_OWNED_WARNING))
            }
            Err(RepositoryOpenError::Unavailable) => {
                Self::build(secrets, None, Some(PROFILE_STORAGE_UNAVAILABLE_WARNING))
            }
        }
    }

    fn build(
        secrets: Arc<dyn SecretStore>,
        profile_repository: Option<Arc<dyn ProfileRepository>>,
        storage_warning: Option<&str>,
    ) -> Self {
        let mut profile_load_warnings: Vec<String> =
            storage_warning.into_iter().map(str::to_owned).collect();
        let persistent_storage_unavailable = storage_warning.is_some();
        let recovery_blocked = persistent_storage_unavailable
            || profile_repository.as_ref().is_some_and(|repository| {
                if reconcile(repository.as_ref(), secrets.as_ref()).is_err() {
                    profile_load_warnings.push(
                        "A pending connection update could not be recovered safely; stored connection profiles are unavailable and changes are disabled."
                            .to_owned(),
                    );
                    true
                } else {
                    false
                }
            });

        let demo = demo_profile();
        let mut profiles = HashMap::from([(demo.id, demo)]);
        if !recovery_blocked && let Some(repository) = profile_repository.as_ref() {
            let (loaded, warning) = load_profiles(repository.as_ref());
            profiles.extend(loaded.into_iter().map(|profile| (profile.id, profile)));
            profile_load_warnings.extend(warning);
        }
        Self {
            profiles: RwLock::new(profiles),
            active: RwLock::new(HashMap::new()),
            approvals: RwLock::new(HashMap::new()),
            connection_lock: Mutex::new(()),
            profile_store_lock: Mutex::new(()),
            profile_repository,
            profile_load_warnings,
            recovery_blocked,
            persistent_storage_unavailable,
            ai: AiClient::new(Arc::clone(&secrets)),
            secrets,
        }
    }

    /// Returns the first sanitized warning produced while loading stored profiles.
    #[must_use]
    pub fn profile_load_warning(&self) -> Option<&str> {
        self.profile_load_warnings.first().map(String::as_str)
    }

    /// Returns all sanitized warnings produced while loading stored profiles.
    #[must_use]
    pub fn profile_load_warnings(&self) -> Vec<String> {
        self.profile_load_warnings.clone()
    }

    #[cfg(test)]
    fn persistent_with_secret_store(
        path: impl Into<PathBuf>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self::build_persistent(secrets, path.into())
    }

    #[cfg(test)]
    fn persistent_with_repository(
        repository: Arc<dyn ProfileRepository>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self::build(secrets, Some(repository), None)
    }

    pub async fn list_profiles(&self) -> Vec<ConnectionProfile> {
        let mut profiles: Vec<_> = self.profiles.read().await.values().cloned().collect();
        profiles.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        profiles
    }

    pub async fn save_profile(&self, profile: ConnectionProfile) -> Result<ConnectionProfile> {
        self.save_profile_with_secret(profile, None).await
    }

    /// Saves a normalized profile and, when supplied, its secret as one logical operation.
    /// Existing secrets are preserved when `secret` is `None`.
    pub async fn save_profile_with_secret(
        &self,
        mut profile: ConnectionProfile,
        secret: Option<&str>,
    ) -> Result<ConnectionProfile> {
        Self::normalize_and_validate_profile(&mut profile)?;
        if secret.is_some_and(str::is_empty) {
            return Err(DataError::SecretUnavailable);
        }

        let _store_guard = self.profile_store_lock.lock().await;
        self.ensure_recovery_available()?;
        let original_profiles = self.profiles.read().await.clone();
        let mut updated_profiles = original_profiles.clone();
        updated_profiles.insert(profile.id, profile.clone());

        let result = if let Some(repository) = self.profile_repository.as_ref() {
            self.save_profile_persistently(
                repository.as_ref(),
                &original_profiles,
                &updated_profiles,
                profile.id,
                secret,
            )
        } else {
            self.save_profile_ephemerally(&original_profiles, profile.id, secret)
        };
        let committed_after_error = result.is_err()
            && self.profile_repository.as_ref().is_some_and(|repository| {
                let target_visible = serialize_profiles(&profiles_from_map(&updated_profiles))
                    .ok()
                    .is_some_and(|target| {
                        repository
                            .read_profiles()
                            .ok()
                            .flatten()
                            .is_some_and(|stored| stored == target)
                    });
                let journal_committed = match repository.read_journal() {
                    Ok(None) => true,
                    Ok(Some(bytes)) => deserialize_journal(&bytes)
                        .is_ok_and(|journal| journal.phase == JournalPhase::Applied),
                    Err(_) => false,
                };
                target_visible && journal_committed
            });
        if result.is_err() && !committed_after_error {
            return result.and(Ok(profile));
        }
        *self.profiles.write().await = updated_profiles;

        if let Some(active) = self.active.write().await.remove(&profile.id) {
            active.close().await;
        }
        result.map(|()| profile)
    }

    pub async fn remove_profile(&self, id: Uuid) -> Result<()> {
        if id == DEMO_PROFILE_ID {
            return Err(DataError::InvalidProfile(
                "the demo profile cannot be removed".to_owned(),
            ));
        }

        let _store_guard = self.profile_store_lock.lock().await;
        self.ensure_recovery_available()?;
        let original_profiles = self.profiles.read().await.clone();
        if !original_profiles.contains_key(&id) {
            return Err(DataError::ProfileNotFound);
        }
        let mut updated_profiles = original_profiles.clone();
        updated_profiles.remove(&id);

        let result = if let Some(repository) = self.profile_repository.as_ref() {
            self.remove_profile_persistently(
                repository.as_ref(),
                &original_profiles,
                &updated_profiles,
                id,
            )
        } else {
            let previous_secret = self.secrets.get(id)?;
            if let Err(error) = self.secrets.delete(id) {
                self.restore_secret(id, previous_secret.as_ref())?;
                return Err(error);
            }
            Ok(())
        };

        let target_is_visible = self.profile_repository.as_ref().is_some_and(|repository| {
            serialize_profiles(&profiles_from_map(&updated_profiles))
                .ok()
                .is_some_and(|target| {
                    repository
                        .read_profiles()
                        .ok()
                        .flatten()
                        .is_some_and(|stored| stored == target)
                })
        });
        // Once a durable delete journal and target profile file exist, failures are
        // completed forward. Keep this process from republishing the removed profile.
        if result.is_ok() || target_is_visible {
            *self.profiles.write().await = updated_profiles;
            if let Some(active) = self.active.write().await.remove(&id) {
                active.close().await;
            }
        }
        result
    }

    fn save_profile_ephemerally(
        &self,
        _original_profiles: &HashMap<Uuid, ConnectionProfile>,
        id: Uuid,
        secret: Option<&str>,
    ) -> Result<()> {
        let previous_secret = if secret.is_some() {
            self.secrets.get(id)?
        } else {
            None
        };
        if let Some(secret) = secret
            && let Err(error) = self.secrets.set(id, secret)
        {
            self.restore_secret(id, previous_secret.as_ref())?;
            return Err(error);
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Keep the durable transaction order visible in one place.
    fn save_profile_persistently(
        &self,
        repository: &dyn ProfileRepository,
        original_profiles: &HashMap<Uuid, ConnectionProfile>,
        updated_profiles: &HashMap<Uuid, ConnectionProfile>,
        profile_id: Uuid,
        secret: Option<&str>,
    ) -> Result<()> {
        let target_profiles = profiles_from_map(updated_profiles);
        let staging_id = secret.map(|_| Self::new_staging_id(updated_profiles, profile_id));
        let previous_secret = if secret.is_some() {
            self.secrets.get(profile_id)?
        } else {
            None
        };
        let mut journal = ProfileJournal::save(profile_id, staging_id, target_profiles.clone());
        replace_journal(repository, &journal)?;
        repository.checkpoint(PersistencePhase::JournalPrepared);

        if let (Some(staging_id), Some(secret)) = (staging_id, secret) {
            if let Err(error) = self.secrets.set(staging_id, secret) {
                return self
                    .abort_prepared_save(
                        repository,
                        &journal,
                        original_profiles,
                        previous_secret.as_ref(),
                        false,
                    )
                    .and(Err(error));
            }
            repository.checkpoint(PersistencePhase::SecretStaged);
            journal.phase = JournalPhase::Staged;
            if let Err(error) = replace_journal(repository, &journal) {
                return self
                    .abort_prepared_save(
                        repository,
                        &journal,
                        original_profiles,
                        previous_secret.as_ref(),
                        false,
                    )
                    .and(Err(error));
            }
        }

        if let Err(error) = replace_target_profiles(repository, &target_profiles) {
            return self
                .abort_prepared_save(
                    repository,
                    &journal,
                    original_profiles,
                    previous_secret.as_ref(),
                    false,
                )
                .and(Err(error));
        }
        repository.checkpoint(PersistencePhase::ProfilesReplaced);
        journal.phase = JournalPhase::ProfilesReplaced;
        if let Err(error) = replace_journal(repository, &journal) {
            return self
                .abort_prepared_save(
                    repository,
                    &journal,
                    original_profiles,
                    previous_secret.as_ref(),
                    false,
                )
                .and(Err(error));
        }

        if let Some(staging_id) = staging_id {
            let staged = match self.secrets.get(staging_id) {
                Ok(Some(staged)) => staged,
                Ok(None) => {
                    self.abort_prepared_save(
                        repository,
                        &journal,
                        original_profiles,
                        previous_secret.as_ref(),
                        false,
                    )?;
                    return Err(DataError::PersistenceConsistency);
                }
                Err(error) => {
                    return self
                        .abort_prepared_save(
                            repository,
                            &journal,
                            original_profiles,
                            previous_secret.as_ref(),
                            false,
                        )
                        .and(Err(error));
                }
            };
            if let Err(error) = self.secrets.set(profile_id, exposed(&staged)) {
                return self
                    .abort_prepared_save(
                        repository,
                        &journal,
                        original_profiles,
                        previous_secret.as_ref(),
                        true,
                    )
                    .and(Err(error));
            }
            repository.checkpoint(PersistencePhase::SecretPromoted);
        }

        journal.phase = JournalPhase::Applied;
        if let Err(error) = replace_journal(repository, &journal)
            && !Self::journal_is_durable(repository, &journal)
        {
            return self
                .abort_prepared_save(
                    repository,
                    &journal,
                    original_profiles,
                    previous_secret.as_ref(),
                    staging_id.is_some(),
                )
                .and(Err(error));
        }
        repository.checkpoint(PersistencePhase::JournalApplied);

        if let Some(staging_id) = staging_id {
            self.secrets
                .delete(staging_id)
                .map_err(|_| DataError::PersistenceConsistency)?;
            repository.checkpoint(PersistencePhase::StagingDeleted);
        }
        remove_journal(repository, journal.operation_id())
            .map_err(|_| DataError::PersistenceConsistency)?;
        repository.checkpoint(PersistencePhase::JournalRemoved);
        Ok(())
    }

    fn journal_is_durable(repository: &dyn ProfileRepository, expected: &ProfileJournal) -> bool {
        repository
            .read_journal()
            .ok()
            .flatten()
            .and_then(|bytes| deserialize_journal(&bytes).ok())
            .is_some_and(|stored| {
                stored.operation_id() == expected.operation_id() && stored.phase == expected.phase
            })
    }

    fn abort_prepared_save(
        &self,
        repository: &dyn ProfileRepository,
        journal: &ProfileJournal,
        original_profiles: &HashMap<Uuid, ConnectionProfile>,
        previous_secret: Option<&secrecy::SecretString>,
        restore_real_secret: bool,
    ) -> Result<()> {
        let target_bytes = serialize_profiles(&journal.target_profiles)?;
        let target_may_be_visible = repository
            .read_profiles()
            .ok()
            .flatten()
            .is_some_and(|bytes| bytes == target_bytes);
        if target_may_be_visible {
            replace_target_profiles(repository, &profiles_from_map(original_profiles))?;
            repository.checkpoint(PersistencePhase::ProfilesRestored);
        }
        if restore_real_secret {
            let JournalOperation::Save { profile_id, .. } = journal.operation else {
                return Err(DataError::PersistenceConsistency);
            };
            self.restore_secret(profile_id, previous_secret)?;
            repository.checkpoint(PersistencePhase::SecretRestored);
        }

        let mut rolled_back = journal.clone();
        rolled_back.phase = JournalPhase::RolledBack;
        if replace_journal(repository, &rolled_back).is_err()
            && !Self::journal_is_durable(repository, &rolled_back)
        {
            return Err(DataError::PersistenceConsistency);
        }
        repository.checkpoint(PersistencePhase::JournalRolledBack);
        if let Some(staging_id) = journal.staging_id() {
            self.secrets
                .delete(staging_id)
                .map_err(|_| DataError::PersistenceConsistency)?;
            repository.checkpoint(PersistencePhase::StagingDeleted);
        }
        remove_journal(repository, journal.operation_id())
            .map_err(|_| DataError::PersistenceConsistency)?;
        repository.checkpoint(PersistencePhase::JournalRemoved);
        Ok(())
    }

    fn remove_profile_persistently(
        &self,
        repository: &dyn ProfileRepository,
        original_profiles: &HashMap<Uuid, ConnectionProfile>,
        updated_profiles: &HashMap<Uuid, ConnectionProfile>,
        id: Uuid,
    ) -> Result<()> {
        let target_profiles = profiles_from_map(updated_profiles);
        let journal = ProfileJournal::delete(id, target_profiles.clone());
        replace_journal(repository, &journal)?;
        repository.checkpoint(PersistencePhase::JournalPrepared);

        if let Err(error) = replace_target_profiles(repository, &target_profiles) {
            let target_bytes = serialize_profiles(&target_profiles)?;
            if repository
                .read_profiles()
                .ok()
                .flatten()
                .is_some_and(|bytes| bytes == target_bytes)
            {
                replace_target_profiles(repository, &profiles_from_map(original_profiles))?;
            }
            remove_journal(repository, journal.operation_id())?;
            return Err(error);
        }
        repository.checkpoint(PersistencePhase::ProfilesReplaced);

        if self.secrets.delete(id).is_err() {
            return Err(DataError::PersistenceConsistency);
        }
        repository.checkpoint(PersistencePhase::SecretDeleted);
        remove_journal(repository, journal.operation_id())
            .map_err(|_| DataError::PersistenceConsistency)?;
        repository.checkpoint(PersistencePhase::JournalRemoved);
        Ok(())
    }

    fn new_staging_id(profiles: &HashMap<Uuid, ConnectionProfile>, profile_id: Uuid) -> Uuid {
        loop {
            let candidate = Uuid::new_v4();
            if candidate != profile_id
                && candidate != DEMO_PROFILE_ID
                && candidate != AI_SECRET_ID
                && !profiles.contains_key(&candidate)
            {
                return candidate;
            }
        }
    }

    fn ensure_recovery_available(&self) -> Result<()> {
        if self.recovery_blocked {
            return Err(DataError::PersistenceConsistency);
        }
        if let Some(repository) = self.profile_repository.as_ref()
            && repository
                .read_journal()
                .map_err(|_| DataError::PersistenceConsistency)?
                .is_some()
        {
            return Err(DataError::PersistenceConsistency);
        }
        Ok(())
    }

    fn ensure_persistent_storage_available(&self) -> Result<()> {
        if self.persistent_storage_unavailable {
            Err(DataError::PersistenceConsistency)
        } else {
            Ok(())
        }
    }

    fn normalize_and_validate_profile(profile: &mut ConnectionProfile) -> Result<()> {
        if profile.built_in && profile.id != DEMO_PROFILE_ID {
            return Err(DataError::InvalidProfile(
                "custom profiles cannot be marked built-in".to_owned(),
            ));
        }
        profile.normalize();
        profile.validate()?;
        if profile.id.is_nil() || profile.id == DEMO_PROFILE_ID || profile.id == AI_SECRET_ID {
            return Err(DataError::InvalidProfile(
                "the selected profile identifier is reserved".to_owned(),
            ));
        }
        Ok(())
    }

    fn restore_secret(&self, id: Uuid, previous: Option<&secrecy::SecretString>) -> Result<()> {
        let result = previous.map_or_else(
            || self.secrets.delete(id),
            |secret| self.secrets.set(id, exposed(secret)),
        );
        result.map_err(|_| DataError::PersistenceConsistency)
    }

    pub async fn save_connection_secret(&self, id: Uuid, secret: &str) -> Result<()> {
        if id == AI_SECRET_ID || id == DEMO_PROFILE_ID {
            return Err(DataError::InvalidProfile(
                "the selected profile identifier is reserved".to_owned(),
            ));
        }
        if secret.is_empty() {
            return Err(DataError::SecretUnavailable);
        }

        let _store_guard = self.profile_store_lock.lock().await;
        self.ensure_recovery_available()?;
        let profiles = self.profiles.read().await.clone();
        if !profiles.contains_key(&id) {
            return Err(DataError::ProfileNotFound);
        }
        if let Some(repository) = self.profile_repository.as_ref() {
            self.save_profile_persistently(
                repository.as_ref(),
                &profiles,
                &profiles,
                id,
                Some(secret),
            )
        } else {
            self.secrets.set(id, secret)
        }
    }

    pub fn save_ai_secret(&self, secret: &str) -> Result<()> {
        self.ensure_persistent_storage_available()?;
        if secret.is_empty() {
            return Err(DataError::SecretUnavailable);
        }
        self.secrets.set(AI_SECRET_ID, secret)
    }

    pub async fn test_connection(
        &self,
        profile: &ConnectionProfile,
        secret: Option<&str>,
    ) -> Result<ConnectionStatus> {
        self.ensure_persistent_storage_available()?;
        profile.validate()?;
        if profile.kind == DatabaseKind::SqlServer {
            return Ok(unsupported_status(profile));
        }
        let started = Instant::now();
        let active = self.open_connection(profile, secret).await?;
        active.close().await;
        Ok(ConnectionStatus {
            profile_id: profile.id,
            state: ConnectionState::Connected,
            message: "Connection test succeeded".to_owned(),
            capabilities: profile.kind.capabilities(),
            latency_ms: Some(elapsed_ms(started)),
        })
    }

    pub async fn connect(&self, id: Uuid) -> Result<ConnectionStatus> {
        self.ensure_persistent_storage_available()?;
        let profile = self.profile(id).await?;
        if profile.kind == DatabaseKind::SqlServer {
            return Ok(unsupported_status(&profile));
        }
        let _connection_guard = self.connection_lock.lock().await;
        if self.active.read().await.contains_key(&id) {
            return Ok(connected_status(&profile, None));
        }
        let started = Instant::now();
        let secret = if profile.kind == DatabaseKind::SQLite {
            None
        } else {
            self.secrets.get(id)?
        };
        let active = self
            .open_connection(&profile, secret.as_ref().map(exposed))
            .await?;
        self.active.write().await.insert(id, active);
        Ok(connected_status(&profile, Some(elapsed_ms(started))))
    }

    pub async fn disconnect(&self, id: Uuid) -> Result<()> {
        self.ensure_persistent_storage_available()?;
        self.profile(id).await?;
        if let Some(active) = self.active.write().await.remove(&id) {
            active.close().await;
        }
        Ok(())
    }

    pub async fn load_metadata(&self, request: MetadataRequest) -> Result<Vec<MetadataNode>> {
        self.ensure_persistent_storage_available()?;
        self.profile(request.connection_id).await?;
        let active = self.ensure_connected(request.connection_id).await?;
        let operation = async {
            match active {
                ActiveConnection::PostgreSql(pool) => {
                    load_postgres_metadata(&pool, request.parent_id.as_deref()).await
                }
                ActiveConnection::MySql(pool) => {
                    load_mysql_metadata(&pool, request.parent_id.as_deref()).await
                }
                ActiveConnection::SQLite(pool) => {
                    load_sqlite_metadata(&pool, request.parent_id.as_deref()).await
                }
                ActiveConnection::Mongo { client, database } => {
                    load_mongo_metadata(&client, &database, request.parent_id.as_deref()).await
                }
            }
        };
        timeout(std::time::Duration::from_secs(30), operation)
            .await
            .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(30)))?
    }

    pub async fn execute_query(&self, request: QueryRequest) -> Result<QueryResultPage> {
        self.ensure_persistent_storage_available()?;
        let profile = self.profile(request.connection_id).await?;
        if profile.kind == DatabaseKind::SqlServer {
            return Err(DataError::Unsupported(
                "SQL Server is unsupported in this preview".to_owned(),
            ));
        }
        validate_query_language(profile.kind, request.language)?;
        if profile.kind != DatabaseKind::MongoDb && !is_single_read_only_statement(&request.query) {
            return Err(DataError::ReadOnlyViolation(
                "queries must contain exactly one read-only statement".to_owned(),
            ));
        }

        let active = self.ensure_connected(request.connection_id).await?;
        match active {
            ActiveConnection::PostgreSql(pool) => execute_postgres_query(&pool, &request).await,
            ActiveConnection::MySql(pool) => execute_mysql_query(&pool, &request).await,
            ActiveConnection::SQLite(pool) => execute_sqlite_query(&pool, &request).await,
            ActiveConnection::Mongo { client, database } => {
                execute_mongo_query(&client, &database, &request).await
            }
        }
    }

    /// Creates a short-lived, single-use approval for a mutation after classifying its risk.
    pub async fn prepare_mutation(
        &self,
        connection_id: Uuid,
        statement: String,
        parameters: Vec<DataValue>,
    ) -> Result<MutationPlan> {
        self.ensure_persistent_storage_available()?;
        let profile = self.profile(connection_id).await?;
        if profile.kind == DatabaseKind::SqlServer {
            return Err(DataError::Unsupported(
                "SQL Server is unsupported in this preview".to_owned(),
            ));
        }
        if profile.read_only {
            return Err(DataError::ReadOnlyViolation(
                "profile is read-only".to_owned(),
            ));
        }
        let risk = if profile.kind == DatabaseKind::MongoDb {
            mongo_mutation_risk(&statement)?
        } else {
            let classification = classify_sql(&statement);
            if classification == QueryClassification::ReadOnly {
                return Err(DataError::InvalidQuery(
                    "read-only statements do not require a mutation plan".to_owned(),
                ));
            }
            risk_for(classification)
        };
        let plan_id = Uuid::new_v4();
        let token = Uuid::new_v4().as_simple().to_string();
        let expires_at = Utc::now() + chrono::Duration::minutes(5);
        let mut approvals = self.approvals.write().await;
        approvals.retain(|_, approval| approval.expires_at >= Utc::now());
        if approvals.len() >= 1_024 {
            return Err(DataError::ApprovalRequired);
        }
        approvals.insert(
            token.clone(),
            Approval {
                plan_id,
                connection_id,
                statement: statement.clone(),
                parameters: parameters.clone(),
                risk,
                expires_at,
            },
        );
        Ok(MutationPlan {
            id: plan_id,
            connection_id,
            statement,
            parameters,
            risk,
            approval_token: Some(token),
            approval_expires_at: Some(expires_at),
        })
    }

    pub async fn apply_mutation(&self, plan: MutationPlan) -> Result<MutationResult> {
        self.ensure_persistent_storage_available()?;
        let profile = self.profile(plan.connection_id).await?;
        if profile.read_only {
            return Err(DataError::ReadOnlyViolation(
                "profile is read-only".to_owned(),
            ));
        }
        self.consume_approval(&plan).await?;
        let active = self.ensure_connected(plan.connection_id).await?;
        let started = Instant::now();
        let mutation = async {
            match active {
                ActiveConnection::PostgreSql(pool) => apply_postgres_mutation(&pool, &plan).await,
                ActiveConnection::MySql(pool) => apply_mysql_mutation(&pool, &plan).await,
                ActiveConnection::SQLite(pool) => apply_sqlite_mutation(&pool, &plan).await,
                ActiveConnection::Mongo { client, database } => {
                    apply_mongo_mutation(&client, &database, &plan.statement).await
                }
            }
        };
        let rows_affected = timeout(std::time::Duration::from_secs(30), mutation)
            .await
            .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(30)))??;
        Ok(MutationResult {
            rows_affected,
            elapsed_ms: elapsed_ms(started),
            committed: true,
        })
    }

    pub async fn ai_chat(&self, input: AiAssistantRequest) -> Result<AiChatResponse> {
        self.ai.chat(build_assistant_chat_request(&input)?).await
    }

    async fn profile(&self, id: Uuid) -> Result<ConnectionProfile> {
        self.profiles
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or(DataError::ProfileNotFound)
    }

    async fn ensure_connected(&self, id: Uuid) -> Result<ActiveConnection> {
        let profile = self.profile(id).await?;
        if profile.kind == DatabaseKind::SqlServer {
            return Err(DataError::Unsupported(
                "SQL Server is unsupported in this preview".to_owned(),
            ));
        }
        if let Some(active) = self.active.read().await.get(&id).cloned() {
            return Ok(active);
        }
        self.connect(id).await?;
        self.active
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| DataError::Unsupported("connection is unavailable".to_owned()))
    }

    async fn open_connection(
        &self,
        profile: &ConnectionProfile,
        secret: Option<&str>,
    ) -> Result<ActiveConnection> {
        match profile.kind {
            DatabaseKind::PostgreSql => {
                let url = relational_url(profile, secret)?;
                let pool = timeout(
                    std::time::Duration::from_secs(15),
                    PgPoolOptions::new().max_connections(5).connect(&url),
                )
                .await
                .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(15)))?
                .map_err(|error| DataError::connection(&error))?;
                health_check_postgres(&pool).await?;
                Ok(ActiveConnection::PostgreSql(pool))
            }
            DatabaseKind::MySql => {
                let url = relational_url(profile, secret)?;
                let pool = timeout(
                    std::time::Duration::from_secs(15),
                    MySqlPoolOptions::new()
                        .max_connections(5)
                        .after_connect(|connection, _metadata| {
                            Box::pin(async move {
                                sqlx::query("SET time_zone = '+00:00'")
                                    .execute(connection)
                                    .await?;
                                Ok(())
                            })
                        })
                        .connect(&url),
                )
                .await
                .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(15)))?
                .map_err(|error| DataError::connection(&error))?;
                health_check_mysql(&pool).await?;
                Ok(ActiveConnection::MySql(pool))
            }
            DatabaseKind::SQLite => {
                let url = relational_url(profile, secret)?;
                let max_connections = if profile.config.file_path.as_deref() == Some(":memory:") {
                    1
                } else {
                    5
                };
                let pool = timeout(
                    std::time::Duration::from_secs(15),
                    SqlitePoolOptions::new()
                        .max_connections(max_connections)
                        .connect(&url),
                )
                .await
                .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(15)))?
                .map_err(|error| DataError::connection(&error))?;
                health_check_sqlite(&pool).await?;
                if profile.id == DEMO_PROFILE_ID {
                    seed_demo(&pool).await?;
                }
                Ok(ActiveConnection::SQLite(pool))
            }
            DatabaseKind::MongoDb => {
                let url = server_url(profile, secret)?;
                let client = timeout(
                    std::time::Duration::from_secs(15),
                    MongoClient::with_uri_str(url.as_str()),
                )
                .await
                .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(15)))?
                .map_err(|error| DataError::connection(&error))?;
                let database = profile
                    .config
                    .database
                    .clone()
                    .unwrap_or_else(|| "test".to_owned());
                timeout(
                    std::time::Duration::from_secs(5),
                    client.database(&database).run_command(doc! {"ping": 1}),
                )
                .await
                .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(5)))?
                .map_err(|error| DataError::connection(&error))?;
                Ok(ActiveConnection::Mongo { client, database })
            }
            DatabaseKind::SqlServer => Err(DataError::Unsupported(
                "SQL Server is unsupported in this preview".to_owned(),
            )),
        }
    }

    async fn consume_approval(&self, plan: &MutationPlan) -> Result<()> {
        let token = plan
            .approval_token
            .as_deref()
            .ok_or(DataError::ApprovalRequired)?;
        let now = Utc::now();
        let mut approvals = self.approvals.write().await;
        approvals.retain(|_, approval| approval.expires_at >= now);
        let approval = approvals.remove(token).ok_or(DataError::InvalidApproval)?;
        if approval.expires_at < Utc::now()
            || approval.plan_id != plan.id
            || approval.connection_id != plan.connection_id
            || approval.statement != plan.statement
            || approval.parameters != plan.parameters
            || approval.risk != plan.risk
        {
            return Err(DataError::InvalidApproval);
        }
        Ok(())
    }
}

fn load_profiles(repository: &dyn ProfileRepository) -> (Vec<ConnectionProfile>, Option<String>) {
    let bytes = match repository.read_profiles() {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return (Vec::new(), None),
        Err(_) => {
            return (
                Vec::new(),
                Some(
                    "Connection profiles could not be loaded; the Demo profile remains available."
                        .to_owned(),
                ),
            );
        }
    };
    let values: Vec<serde_json::Value> = match serde_json::from_slice(&bytes) {
        Ok(values) => values,
        Err(_) => {
            return (
                Vec::new(),
                Some(
                    "Connection profiles could not be loaded; the Demo profile remains available."
                        .to_owned(),
                ),
            );
        }
    };

    let mut profiles = HashMap::new();
    let mut skipped = false;
    for value in values {
        let Ok(mut profile) = serde_json::from_value::<ConnectionProfile>(value) else {
            skipped = true;
            continue;
        };
        profile.normalize();
        if profile.id.is_nil()
            || profile.id == DEMO_PROFILE_ID
            || profile.id == AI_SECRET_ID
            || profile.built_in
            || profile.validate().is_err()
            || profiles.insert(profile.id, profile).is_some()
        {
            skipped = true;
        }
    }
    let warning = skipped.then(|| {
        "Some connection profiles could not be loaded; valid profiles remain available.".to_owned()
    });
    (profiles.into_values().collect(), warning)
}

fn demo_profile() -> ConnectionProfile {
    ConnectionProfile {
        id: DEMO_PROFILE_ID,
        name: "Demo SQLite".to_owned(),
        kind: DatabaseKind::SQLite,
        config: ConnectionConfig {
            file_path: Some(":memory:".to_owned()),
            ..ConnectionConfig::default()
        },
        read_only: false,
        built_in: true,
        capabilities: DatabaseKind::SQLite.capabilities(),
    }
}

fn unsupported_status(profile: &ConnectionProfile) -> ConnectionStatus {
    ConnectionStatus {
        profile_id: profile.id,
        state: ConnectionState::Unsupported,
        message: profile
            .kind
            .capabilities()
            .unsupported_reason
            .clone()
            .unwrap_or_else(|| "Unsupported".to_owned()),
        capabilities: profile.kind.capabilities(),
        latency_ms: None,
    }
}

fn connected_status(profile: &ConnectionProfile, latency_ms: Option<u64>) -> ConnectionStatus {
    ConnectionStatus {
        profile_id: profile.id,
        state: ConnectionState::Connected,
        message: "Connected".to_owned(),
        capabilities: profile.kind.capabilities(),
        latency_ms,
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn validate_query_language(kind: DatabaseKind, language: QueryLanguage) -> Result<()> {
    match (kind, language) {
        (DatabaseKind::MongoDb, QueryLanguage::Sql) => Err(DataError::InvalidQuery(
            "SQL cannot run on MongoDB".to_owned(),
        )),
        (DatabaseKind::MongoDb, QueryLanguage::Auto | QueryLanguage::MongoJson)
        | (_, QueryLanguage::Auto | QueryLanguage::Sql) => Ok(()),
        (_, QueryLanguage::MongoJson) => Err(DataError::InvalidQuery(
            "Mongo JSON cannot run on a SQL connection".to_owned(),
        )),
    }
}

fn relational_url(profile: &ConnectionProfile, secret: Option<&str>) -> Result<String> {
    if profile.kind == DatabaseKind::SQLite {
        let path = profile
            .config
            .file_path
            .as_deref()
            .ok_or_else(|| DataError::InvalidProfile("SQLite path is required".to_owned()))?;
        return Ok(if path == ":memory:" {
            "sqlite::memory:".to_owned()
        } else {
            format!("sqlite://{path}?mode=rwc")
        });
    }
    Ok(server_url(profile, secret)?.to_string())
}

fn server_url(profile: &ConnectionProfile, secret: Option<&str>) -> Result<Url> {
    profile.validate()?;
    let srv = profile.kind == DatabaseKind::MongoDb && profile.config.srv;
    let scheme = match profile.kind {
        DatabaseKind::PostgreSql => "postgresql",
        DatabaseKind::MySql => "mysql",
        DatabaseKind::MongoDb if srv => "mongodb+srv",
        DatabaseKind::MongoDb => "mongodb",
        DatabaseKind::SqlServer => "sqlserver",
        DatabaseKind::SQLite => {
            return Err(DataError::InvalidProfile(
                "not a server connection".to_owned(),
            ));
        }
    };
    let host = profile
        .config
        .host
        .as_deref()
        .ok_or_else(|| DataError::InvalidProfile("host is required".to_owned()))?;
    let authority = match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("[{host}]"),
        _ => host.to_owned(),
    };
    let mut url = Url::parse(&format!("{scheme}://{authority}"))
        .map_err(|_| DataError::InvalidProfile("host is invalid".to_owned()))?;
    if !srv {
        let port = profile
            .config
            .port
            .or(profile.kind.default_port())
            .ok_or_else(|| DataError::InvalidProfile("port is required".to_owned()))?;
        url.set_port(Some(port))
            .map_err(|()| DataError::InvalidProfile("port is invalid".to_owned()))?;
    }
    if let Some(username) = profile.config.username.as_deref() {
        url.set_username(username)
            .map_err(|()| DataError::InvalidProfile("username is invalid".to_owned()))?;
    }
    if let Some(password) = secret {
        url.set_password(Some(password)).map_err(|()| {
            DataError::InvalidProfile("secret contains unsupported characters".to_owned())
        })?;
    }
    url.set_path(profile.config.database.as_deref().unwrap_or(""));
    if profile.config.tls {
        url.query_pairs_mut().append_pair(
            match profile.kind {
                DatabaseKind::MongoDb => "tls",
                DatabaseKind::MySql => "ssl-mode",
                _ => "sslmode",
            },
            match profile.kind {
                DatabaseKind::MongoDb => "true",
                DatabaseKind::MySql => "VERIFY_IDENTITY",
                DatabaseKind::PostgreSql => "verify-full",
                _ => "require",
            },
        );
    }
    for (key, value) in &profile.config.options {
        url.query_pairs_mut().append_pair(key, value);
    }
    Ok(url)
}

async fn health_check_postgres(pool: &PgPool) -> Result<()> {
    timeout(
        std::time::Duration::from_secs(5),
        sqlx::query("SELECT 1").execute(pool),
    )
    .await
    .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(5)))?
    .map_err(|error| DataError::connection(&error))?;
    Ok(())
}

async fn health_check_mysql(pool: &MySqlPool) -> Result<()> {
    timeout(
        std::time::Duration::from_secs(5),
        sqlx::query("SELECT 1").execute(pool),
    )
    .await
    .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(5)))?
    .map_err(|error| DataError::connection(&error))?;
    Ok(())
}

async fn health_check_sqlite(pool: &SqlitePool) -> Result<()> {
    timeout(
        std::time::Duration::from_secs(5),
        sqlx::query("SELECT 1").execute(pool),
    )
    .await
    .map_err(|_| DataError::Timeout(std::time::Duration::from_secs(5)))?
    .map_err(|error| DataError::connection(&error))?;
    Ok(())
}

async fn seed_demo(pool: &SqlitePool) -> Result<()> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE, city TEXT NOT NULL, created_at TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), status TEXT NOT NULL, total_cents INTEGER NOT NULL, ordered_at TEXT NOT NULL)",
        "INSERT OR IGNORE INTO customers (id,name,email,city,created_at) VALUES (1,'Ada Lovelace','ada@example.test','London','2025-01-05T10:00:00Z'),(2,'Grace Hopper','grace@example.test','New York','2025-01-08T11:30:00Z'),(3,'Katherine Johnson','katherine@example.test','Hampton','2025-01-10T09:15:00Z'),(4,'Edsger Dijkstra','edsger@example.test','Nuenen','2025-01-14T16:45:00Z')",
        "INSERT OR IGNORE INTO orders (id,customer_id,status,total_cents,ordered_at) VALUES (101,1,'shipped',12900,'2025-02-01T12:00:00Z'),(102,2,'processing',7550,'2025-02-03T14:20:00Z'),(103,1,'delivered',2499,'2025-02-05T08:10:00Z'),(104,3,'cancelled',4000,'2025-02-06T19:00:00Z'),(105,4,'shipped',31000,'2025-02-08T10:30:00Z')",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|error| DataError::database("failed to seed demo database", &error))?;
    }
    Ok(())
}

async fn load_postgres_metadata(pool: &PgPool, parent: Option<&str>) -> Result<Vec<MetadataNode>> {
    if let Some(id) = parent {
        let qualified = id
            .strip_prefix("table:")
            .ok_or_else(|| DataError::InvalidQuery("invalid metadata node".to_owned()))?;
        let (schema, table) = qualified
            .split_once('.')
            .ok_or_else(|| DataError::InvalidQuery("invalid table metadata node".to_owned()))?;
        let rows = sqlx::query(
            "SELECT column_name, data_type, is_nullable FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(|error| DataError::database("column metadata load failed", &error))?;
        return rows
            .into_iter()
            .map(|row| relational_column_metadata(qualified, &row))
            .collect();
    }
    let rows = sqlx::query(
        "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema, table_name",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| DataError::database("metadata load failed", &error))?;
    rows.into_iter()
        .map(|row| relational_table_metadata(&row))
        .collect()
}

async fn load_mysql_metadata(pool: &MySqlPool, parent: Option<&str>) -> Result<Vec<MetadataNode>> {
    if let Some(id) = parent {
        let qualified = id
            .strip_prefix("table:")
            .ok_or_else(|| DataError::InvalidQuery("invalid metadata node".to_owned()))?;
        let (schema, table) = qualified
            .split_once('.')
            .ok_or_else(|| DataError::InvalidQuery("invalid table metadata node".to_owned()))?;
        let rows = sqlx::query(
            "SELECT column_name, data_type, is_nullable FROM information_schema.columns WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(|error| DataError::database("column metadata load failed", &error))?;
        return rows
            .into_iter()
            .map(|row| relational_column_metadata(qualified, &row))
            .collect();
    }
    let rows = sqlx::query(
        "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema = DATABASE() ORDER BY table_name",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| DataError::database("metadata load failed", &error))?;
    rows.into_iter()
        .map(|row| relational_table_metadata(&row))
        .collect()
}

fn relational_column_metadata<R>(qualified: &str, row: &R) -> Result<MetadataNode>
where
    R: Row,
    usize: sqlx::ColumnIndex<R>,
    String: for<'r> sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    let name: String = row
        .try_get(0)
        .map_err(|error| DataError::database("column metadata conversion failed", &error))?;
    let data_type: String = row
        .try_get(1)
        .map_err(|error| DataError::database("column metadata conversion failed", &error))?;
    let nullable: String = row
        .try_get(2)
        .map_err(|error| DataError::database("column metadata conversion failed", &error))?;
    Ok(MetadataNode {
        id: format!("column:{qualified}.{name}"),
        name,
        kind: MetadataKind::Column,
        data_type: Some(data_type),
        nullable: Some(nullable.eq_ignore_ascii_case("yes")),
        has_children: false,
        children: Vec::new(),
        attributes: std::collections::BTreeMap::new(),
    })
}

fn relational_table_metadata<R>(row: &R) -> Result<MetadataNode>
where
    R: Row,
    usize: sqlx::ColumnIndex<R>,
    String: for<'r> sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    let schema: String = row
        .try_get(0)
        .map_err(|error| DataError::database("metadata conversion failed", &error))?;
    let name: String = row
        .try_get(1)
        .map_err(|error| DataError::database("metadata conversion failed", &error))?;
    let table_type: String = row
        .try_get(2)
        .map_err(|error| DataError::database("metadata conversion failed", &error))?;
    Ok(MetadataNode {
        id: format!("table:{schema}.{name}"),
        name,
        kind: if table_type.to_ascii_lowercase().contains("view") {
            MetadataKind::View
        } else {
            MetadataKind::Table
        },
        data_type: None,
        nullable: None,
        has_children: true,
        children: Vec::new(),
        attributes: std::collections::BTreeMap::from([("schema".to_owned(), schema)]),
    })
}

async fn load_sqlite_metadata(
    pool: &SqlitePool,
    parent: Option<&str>,
) -> Result<Vec<MetadataNode>> {
    if let Some(id) = parent {
        let table = id
            .strip_prefix("table:")
            .ok_or_else(|| DataError::InvalidQuery("invalid metadata node".to_owned()))?;
        let quoted = table.replace('"', "\"\"");
        let pragma = format!("PRAGMA table_info(\"{quoted}\")");
        // The table name came from a server-generated metadata ID and embedded quotes are doubled.
        let rows = sqlx::query(AssertSqlSafe(pragma.as_str()))
            .fetch_all(pool)
            .await
            .map_err(|error| DataError::database("column metadata load failed", &error))?;
        return rows
            .into_iter()
            .map(|row| {
                let name: String = row.try_get("name").map_err(|error| {
                    DataError::database("column metadata conversion failed", &error)
                })?;
                let data_type: String = row.try_get("type").map_err(|error| {
                    DataError::database("column metadata conversion failed", &error)
                })?;
                let not_null: i64 = row.try_get("notnull").map_err(|error| {
                    DataError::database("column metadata conversion failed", &error)
                })?;
                Ok(MetadataNode {
                    id: format!("column:{table}.{name}"),
                    name,
                    kind: MetadataKind::Column,
                    data_type: Some(data_type),
                    nullable: Some(not_null == 0),
                    has_children: false,
                    children: Vec::new(),
                    attributes: std::collections::BTreeMap::new(),
                })
            })
            .collect();
    }
    let rows = sqlx::query("SELECT name, type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(pool).await.map_err(|error| DataError::database("table metadata load failed", &error))?;
    rows.into_iter()
        .map(|row| {
            let name: String = row
                .try_get("name")
                .map_err(|error| DataError::database("table metadata conversion failed", &error))?;
            let kind_value: String = row
                .try_get("type")
                .map_err(|error| DataError::database("table metadata conversion failed", &error))?;
            Ok(MetadataNode {
                id: format!("table:{name}"),
                name,
                kind: if kind_value == "view" {
                    MetadataKind::View
                } else {
                    MetadataKind::Table
                },
                data_type: None,
                nullable: None,
                has_children: true,
                children: Vec::new(),
                attributes: std::collections::BTreeMap::new(),
            })
        })
        .collect()
}

async fn load_mongo_metadata(
    client: &MongoClient,
    database: &str,
    parent: Option<&str>,
) -> Result<Vec<MetadataNode>> {
    let db = client.database(database);
    if let Some(id) = parent {
        let collection = id
            .strip_prefix("collection:")
            .ok_or_else(|| DataError::InvalidQuery("invalid metadata node".to_owned()))?;
        let sample = db
            .collection::<Document>(collection)
            .find_one(doc! {})
            .await
            .map_err(|error| DataError::database("MongoDB metadata load failed", &error))?;
        return Ok(sample.map_or_else(Vec::new, |document| {
            document
                .into_iter()
                .map(|(name, value)| MetadataNode {
                    id: format!("field:{collection}.{name}"),
                    name,
                    kind: MetadataKind::Column,
                    data_type: Some(bson_type_name(&value).to_owned()),
                    nullable: None,
                    has_children: false,
                    children: Vec::new(),
                    attributes: std::collections::BTreeMap::new(),
                })
                .collect()
        }));
    }
    let names = db
        .list_collection_names()
        .await
        .map_err(|error| DataError::database("MongoDB metadata load failed", &error))?;
    Ok(names
        .into_iter()
        .map(|name| MetadataNode {
            id: format!("collection:{name}"),
            name,
            kind: MetadataKind::Collection,
            data_type: None,
            nullable: None,
            has_children: true,
            children: Vec::new(),
            attributes: std::collections::BTreeMap::new(),
        })
        .collect())
}

async fn execute_postgres_query(pool: &PgPool, request: &QueryRequest) -> Result<QueryResultPage> {
    let limit = request.validated_limit()?;
    let started = Instant::now();
    let query = bind_postgres_query(
        sqlx::query(AssertSqlSafe(request.query.as_str())),
        &request.parameters,
    )?;
    let future = async {
        let mut transaction = pool
            .begin()
            .await
            .map_err(|error| DataError::database("read-only transaction failed", &error))?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(|error| DataError::database("read-only transaction failed", &error))?;
        let result = async {
            let mut stream = query.fetch(&mut *transaction);
            let mut columns = Vec::new();
            let mut rows = Vec::new();
            let mut seen = 0_usize;
            while let Some(row) = stream
                .try_next()
                .await
                .map_err(|error| DataError::database("query execution failed", &error))?
            {
                if columns.is_empty() {
                    columns = query_columns(&row);
                }
                if seen >= request.offset {
                    rows.push(convert_postgres_row(&row)?);
                    if rows.len() > limit {
                        break;
                    }
                }
                seen += 1;
            }
            Ok::<_, DataError>(paginated_rows(columns, rows, limit, request.offset))
        }
        .await;
        transaction
            .rollback()
            .await
            .map_err(|error| DataError::database("read-only transaction cleanup failed", &error))?;
        result
    };
    let (columns, rows, truncated, next_offset) = timeout(request.timeout(), future)
        .await
        .map_err(|_| DataError::Timeout(request.timeout()))??;
    Ok(sql_query_page(
        columns,
        rows,
        truncated,
        next_offset,
        elapsed_ms(started),
    ))
}

async fn execute_mysql_query(pool: &MySqlPool, request: &QueryRequest) -> Result<QueryResultPage> {
    if !is_single_read_only_statement(&request.query) {
        return Err(DataError::ReadOnlyViolation(
            "queries must contain exactly one read-only statement".to_owned(),
        ));
    }
    let limit = request.validated_limit()?;
    let started = Instant::now();
    let query = bind_mysql_query(
        sqlx::query(AssertSqlSafe(request.query.as_str())),
        &request.parameters,
    )?;
    let future = async {
        let mut connection = pool
            .acquire()
            .await
            .map_err(|error| DataError::database("connection checkout failed", &error))?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *connection)
            .await
            .map_err(|error| DataError::database("read-only transaction failed", &error))?;
        let mut transaction = sqlx::Connection::begin(&mut *connection)
            .await
            .map_err(|error| DataError::database("read-only transaction failed", &error))?;
        let result = async {
            let mut stream = query.fetch(&mut *transaction);
            let mut columns = Vec::new();
            let mut rows = Vec::new();
            let mut seen = 0_usize;
            while let Some(row) = stream
                .try_next()
                .await
                .map_err(|error| DataError::database("query execution failed", &error))?
            {
                if columns.is_empty() {
                    columns = query_columns(&row);
                }
                if seen >= request.offset {
                    rows.push(convert_mysql_row(&row)?);
                    if rows.len() > limit {
                        break;
                    }
                }
                seen += 1;
            }
            Ok::<_, DataError>(paginated_rows(columns, rows, limit, request.offset))
        }
        .await;
        transaction
            .rollback()
            .await
            .map_err(|error| DataError::database("read-only transaction cleanup failed", &error))?;
        result
    };
    let (columns, rows, truncated, next_offset) = timeout(request.timeout(), future)
        .await
        .map_err(|_| DataError::Timeout(request.timeout()))??;
    Ok(sql_query_page(
        columns,
        rows,
        truncated,
        next_offset,
        elapsed_ms(started),
    ))
}

async fn execute_sqlite_query(
    pool: &SqlitePool,
    request: &QueryRequest,
) -> Result<QueryResultPage> {
    let limit = request.validated_limit()?;
    let started = Instant::now();
    let query = bind_sqlite_query(
        sqlx::query(AssertSqlSafe(request.query.as_str())),
        &request.parameters,
    )?;
    let future = async {
        let mut connection = pool
            .acquire()
            .await
            .map_err(|error| DataError::database("connection checkout failed", &error))?;
        let mut stream = query.fetch(&mut *connection);
        let mut columns = Vec::new();
        let mut rows = Vec::new();
        let mut seen = 0_usize;
        while let Some(row) = stream
            .try_next()
            .await
            .map_err(|error| DataError::database("query execution failed", &error))?
        {
            if columns.is_empty() {
                columns = query_columns(&row);
            }
            if seen >= request.offset {
                rows.push(convert_sqlite_row(&row)?);
                if rows.len() > limit {
                    break;
                }
            }
            seen += 1;
        }
        Ok::<_, DataError>(paginated_rows(columns, rows, limit, request.offset))
    };
    let (columns, rows, truncated, next_offset) = timeout(request.timeout(), future)
        .await
        .map_err(|_| DataError::Timeout(request.timeout()))??;
    Ok(sql_query_page(
        columns,
        rows,
        truncated,
        next_offset,
        elapsed_ms(started),
    ))
}

fn query_columns<R: Row>(row: &R) -> Vec<QueryColumn> {
    row.columns()
        .iter()
        .map(|column| QueryColumn {
            name: column.name().to_owned(),
            data_type: column.type_info().name().to_owned(),
            nullable: None,
        })
        .collect()
}

type PaginatedSqlRows = (Vec<QueryColumn>, Vec<Vec<DataValue>>, bool, Option<usize>);

fn paginated_rows(
    columns: Vec<QueryColumn>,
    mut rows: Vec<Vec<DataValue>>,
    limit: usize,
    offset: usize,
) -> PaginatedSqlRows {
    let truncated = rows.len() > limit;
    if truncated {
        rows.pop();
    }
    let next_offset = truncated.then_some(offset + rows.len());
    (columns, rows, truncated, next_offset)
}

fn sql_query_page(
    columns: Vec<QueryColumn>,
    rows: Vec<Vec<DataValue>>,
    truncated: bool,
    next_offset: Option<usize>,
    elapsed_ms: u64,
) -> QueryResultPage {
    QueryResultPage {
        columns,
        stats: QueryStats {
            elapsed_ms,
            rows_returned: rows.len(),
            rows_affected: 0,
            truncated,
        },
        rows,
        next_offset,
    }
}

struct PgNull;

impl Type<Postgres> for PgNull {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(0))
    }
}

impl Encode<'_, Postgres> for PgNull {
    fn encode_by_ref(
        &self,
        _buffer: &mut PgArgumentBuffer,
    ) -> std::result::Result<IsNull, Box<dyn std::error::Error + Send + Sync>> {
        Ok(IsNull::Yes)
    }
}

fn bind_postgres_query<'q>(
    mut query: Query<'q, Postgres, PgArguments>,
    parameters: &[DataValue],
) -> Result<Query<'q, Postgres, PgArguments>> {
    for value in parameters {
        query = match value {
            DataValue::Null => query.bind(PgNull),
            DataValue::Boolean(value) => query.bind(*value),
            DataValue::Integer(value) => query.bind(parse_i64_parameter(value)?),
            DataValue::Float(value) => query.bind(*value),
            DataValue::Decimal(value) => query.bind(parse_decimal_parameter(value)?),
            DataValue::Text(value) => query.bind(value.clone()),
            DataValue::Binary(value) => query.bind(decode_binary_parameter(value)?),
            DataValue::Date(value) => query.bind(parse_date_parameter(value)?),
            DataValue::Time(value) => query.bind(parse_time_parameter(value)?),
            DataValue::DateTime(value) => match parse_date_time_parameter(value)? {
                ParsedDateTime::Naive(value) => query.bind(value),
                ParsedDateTime::Utc(value) => query.bind(value),
            },
            DataValue::Uuid(value) => query.bind(*value),
            DataValue::Json(value) | DataValue::Bson(value) => query.bind(value.clone()),
        };
    }
    Ok(query)
}

fn bind_mysql_query<'q>(
    mut query: Query<'q, MySql, MySqlArguments>,
    parameters: &[DataValue],
) -> Result<Query<'q, MySql, MySqlArguments>> {
    for value in parameters {
        query = match value {
            DataValue::Null => query.bind(Option::<String>::None),
            DataValue::Boolean(value) => query.bind(*value),
            DataValue::Integer(value) => match value.parse::<i64>() {
                Ok(value) => query.bind(value),
                Err(_) => query.bind(value.parse::<u64>().map_err(|_| {
                    DataError::Conversion("integer parameter is out of range".to_owned())
                })?),
            },
            DataValue::Float(value) => query.bind(*value),
            DataValue::Decimal(value) => query.bind(parse_decimal_parameter(value)?),
            DataValue::Text(value) => query.bind(value.clone()),
            DataValue::Binary(value) => query.bind(decode_binary_parameter(value)?),
            DataValue::Date(value) => query.bind(parse_date_parameter(value)?),
            DataValue::Time(value) => query.bind(parse_time_parameter(value)?),
            DataValue::DateTime(value) => match parse_date_time_parameter(value)? {
                ParsedDateTime::Naive(value) => query.bind(value),
                ParsedDateTime::Utc(value) => query.bind(value),
            },
            DataValue::Uuid(value) => query.bind(value.to_string()),
            DataValue::Json(value) | DataValue::Bson(value) => query.bind(value.clone()),
        };
    }
    Ok(query)
}

fn bind_sqlite_query<'q>(
    mut query: Query<'q, Sqlite, SqliteArguments>,
    parameters: &[DataValue],
) -> Result<Query<'q, Sqlite, SqliteArguments>> {
    for value in parameters {
        query = match value {
            DataValue::Null => query.bind(Option::<String>::None),
            DataValue::Boolean(value) => query.bind(*value),
            DataValue::Integer(value) => query.bind(parse_i64_parameter(value)?),
            DataValue::Float(value) => query.bind(*value),
            DataValue::Decimal(value)
            | DataValue::Text(value)
            | DataValue::Date(value)
            | DataValue::Time(value)
            | DataValue::DateTime(value) => query.bind(value.clone()),
            DataValue::Binary(value) => query.bind(decode_binary_parameter(value)?),
            DataValue::Uuid(value) => query.bind(value.to_string()),
            DataValue::Json(value) | DataValue::Bson(value) => query.bind(value.to_string()),
        };
    }
    Ok(query)
}

fn parse_i64_parameter(value: &str) -> Result<i64> {
    value
        .parse()
        .map_err(|_| DataError::Conversion("integer parameter is out of range".to_owned()))
}

fn parse_decimal_parameter(value: &str) -> Result<BigDecimal> {
    value
        .parse()
        .map_err(|_| DataError::Conversion("invalid decimal parameter".to_owned()))
}

fn decode_binary_parameter(value: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| DataError::Conversion("invalid base64 binary parameter".to_owned()))
}

fn parse_date_parameter(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| DataError::Conversion("invalid date parameter".to_owned()))
}

fn parse_time_parameter(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M:%S%.f")
        .map_err(|_| DataError::Conversion("invalid time parameter".to_owned()))
}

#[derive(Debug, PartialEq)]
enum ParsedDateTime {
    Naive(NaiveDateTime),
    Utc(DateTime<Utc>),
}

fn parse_date_time_parameter(value: &str) -> Result<ParsedDateTime> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return Ok(ParsedDateTime::Utc(value.with_timezone(&Utc)));
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"))
        .map(ParsedDateTime::Naive)
        .map_err(|_| DataError::Conversion("invalid date-time parameter".to_owned()))
}

fn convert_postgres_row(row: &PgRow) -> Result<Vec<DataValue>> {
    (0..row.len())
        .map(|index| convert_postgres_cell(row, index))
        .collect()
}

fn convert_postgres_cell(row: &PgRow, index: usize) -> Result<DataValue> {
    if row
        .try_get_raw(index)
        .map_err(|error| DataError::database("cell conversion failed", &error))?
        .is_null()
    {
        return Ok(DataValue::Null);
    }
    let type_name = row.column(index).type_info().name();
    match type_name {
        "BOOL" => decode_cell(row, index, DataValue::Boolean, "boolean"),
        "INT2" => decode_integer_cell::<i16, _>(row, index),
        "INT4" => decode_integer_cell::<i32, _>(row, index),
        "INT8" => decode_integer_cell::<i64, _>(row, index),
        "FLOAT4" => decode_cell(
            row,
            index,
            |value: f32| DataValue::Float(f64::from(value)),
            "float",
        ),
        "FLOAT8" => decode_cell(row, index, DataValue::Float, "float"),
        "BYTEA" => decode_binary_cell(row, index),
        "DATE" => decode_cell(
            row,
            index,
            |value: NaiveDate| DataValue::Date(value.format("%Y-%m-%d").to_string()),
            "date",
        ),
        "TIME" => decode_cell(
            row,
            index,
            |value: NaiveTime| DataValue::Time(format_naive_time(value)),
            "time",
        ),
        "TIMESTAMP" => decode_cell(
            row,
            index,
            |value: NaiveDateTime| DataValue::DateTime(format_naive_date_time(value)),
            "date-time",
        ),
        "TIMESTAMPTZ" => decode_cell(
            row,
            index,
            |value: DateTime<Utc>| DataValue::DateTime(format_utc_date_time(value)),
            "date-time",
        ),
        "NUMERIC" => decode_cell(
            row,
            index,
            |value: BigDecimal| DataValue::Decimal(value.to_string()),
            "decimal",
        ),
        "JSON" | "JSONB" => decode_cell(row, index, DataValue::Json, "JSON"),
        "UUID" => decode_cell(row, index, DataValue::Uuid, "UUID"),
        _ => decode_cell(row, index, DataValue::Text, "text"),
    }
}

fn convert_mysql_row(row: &MySqlRow) -> Result<Vec<DataValue>> {
    (0..row.len())
        .map(|index| convert_mysql_cell(row, index))
        .collect()
}

fn convert_mysql_cell(row: &MySqlRow, index: usize) -> Result<DataValue> {
    if row
        .try_get_raw(index)
        .map_err(|error| DataError::database("cell conversion failed", &error))?
        .is_null()
    {
        return Ok(DataValue::Null);
    }
    let type_name = row.column(index).type_info().name().to_ascii_uppercase();
    if matches!(type_name.as_str(), "BOOL" | "BOOLEAN") {
        return decode_cell(row, index, DataValue::Boolean, "boolean");
    }
    if type_name == "BIT" {
        return decode_mysql_bit(row, index);
    }
    if type_name.contains("UNSIGNED") {
        return if type_name.contains("TINYINT") {
            decode_integer_cell::<u8, _>(row, index)
        } else if type_name.contains("SMALLINT") {
            decode_integer_cell::<u16, _>(row, index)
        } else if type_name.contains("MEDIUMINT") || type_name == "INT UNSIGNED" {
            decode_integer_cell::<u32, _>(row, index)
        } else {
            decode_integer_cell::<u64, _>(row, index)
        };
    }
    // MySQL exposes BOOLEAN/BOOL aliases as protocol-equivalent signed TINYINT.
    // Decode from type metadata only and never infer a boolean from the cell value.
    if type_name.contains("TINYINT") {
        return decode_integer_cell::<i8, _>(row, index);
    }
    if type_name.contains("SMALLINT") {
        return decode_integer_cell::<i16, _>(row, index);
    }
    if type_name.contains("MEDIUMINT") || type_name == "INT" || type_name == "INTEGER" {
        return decode_integer_cell::<i32, _>(row, index);
    }
    if type_name.contains("BIGINT") {
        return decode_integer_cell::<i64, _>(row, index);
    }
    if type_name == "YEAR" {
        return decode_integer_cell::<u16, _>(row, index);
    }
    match type_name.as_str() {
        "FLOAT" => decode_cell(
            row,
            index,
            |value: f32| DataValue::Float(f64::from(value)),
            "float",
        ),
        "DOUBLE" => decode_cell(row, index, DataValue::Float, "float"),
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            decode_binary_cell(row, index)
        }
        "DATE" => decode_cell(
            row,
            index,
            |value: NaiveDate| DataValue::Date(value.format("%Y-%m-%d").to_string()),
            "date",
        ),
        "TIME" => decode_cell(
            row,
            index,
            |value: MySqlTime| DataValue::Time(format_mysql_time(value)),
            "time",
        ),
        "DATETIME" => decode_cell(
            row,
            index,
            |value: NaiveDateTime| DataValue::DateTime(format_naive_date_time(value)),
            "date-time",
        ),
        "TIMESTAMP" => decode_cell(
            row,
            index,
            |value: DateTime<Utc>| DataValue::DateTime(format_utc_date_time(value)),
            "date-time",
        ),
        "DECIMAL" | "NEWDECIMAL" => decode_cell(
            row,
            index,
            |value: BigDecimal| DataValue::Decimal(value.to_string()),
            "decimal",
        ),
        "JSON" => decode_cell(row, index, DataValue::Json, "JSON"),
        _ => decode_cell(row, index, DataValue::Text, "text"),
    }
}

fn decode_mysql_bit(row: &MySqlRow, index: usize) -> Result<DataValue> {
    let bytes: Vec<u8> = row
        .try_get_unchecked(index)
        .map_err(|error| DataError::database("bit conversion failed", &error))?;
    Ok(mysql_bit_value(&bytes))
}

fn mysql_bit_value(bytes: &[u8]) -> DataValue {
    let value = bytes
        .iter()
        .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte));
    DataValue::Integer(value.to_string())
}

fn convert_sqlite_row(row: &SqliteRow) -> Result<Vec<DataValue>> {
    (0..row.len())
        .map(|index| convert_sqlite_cell(row, index))
        .collect()
}

fn convert_sqlite_cell(row: &SqliteRow, index: usize) -> Result<DataValue> {
    let raw = row
        .try_get_raw(index)
        .map_err(|error| DataError::database("cell conversion failed", &error))?;
    if raw.is_null() {
        return Ok(DataValue::Null);
    }
    let storage_type = raw.type_info().name().to_ascii_uppercase();
    let declared_type = row.column(index).type_info().name().to_ascii_uppercase();
    match storage_type.as_str() {
        "INTEGER" => {
            let value: i64 = row
                .try_get(index)
                .map_err(|error| DataError::database("integer conversion failed", &error))?;
            if declared_type.contains("BOOL") {
                Ok(DataValue::Boolean(value != 0))
            } else if declared_type.contains("DECIMAL") || declared_type.contains("NUMERIC") {
                Ok(DataValue::Decimal(value.to_string()))
            } else {
                Ok(DataValue::Integer(value.to_string()))
            }
        }
        "REAL" => decode_cell(row, index, DataValue::Float, "float"),
        "BLOB" => decode_binary_cell(row, index),
        "TEXT" => {
            let value: String = row
                .try_get(index)
                .map_err(|error| DataError::database("text conversion failed", &error))?;
            sqlite_text_value(&declared_type, value)
        }
        _ => Err(DataError::Conversion(
            "database returned an unsupported SQLite storage class".to_owned(),
        )),
    }
}

fn sqlite_text_value(declared_type: &str, value: String) -> Result<DataValue> {
    if declared_type == "DATE" {
        Ok(DataValue::Date(value))
    } else if declared_type == "TIME" {
        Ok(DataValue::Time(value))
    } else if declared_type.contains("TIMESTAMP") || declared_type.contains("DATETIME") {
        Ok(DataValue::DateTime(value))
    } else if declared_type.contains("DECIMAL") || declared_type.contains("NUMERIC") {
        Ok(DataValue::Decimal(value))
    } else if declared_type.contains("JSON") {
        serde_json::from_str(&value)
            .map(DataValue::Json)
            .map_err(|_| DataError::Conversion("database returned invalid JSON".to_owned()))
    } else if declared_type.contains("UUID") {
        Uuid::parse_str(&value)
            .map(DataValue::Uuid)
            .map_err(|_| DataError::Conversion("database returned invalid UUID".to_owned()))
    } else {
        Ok(DataValue::Text(value))
    }
}

fn decode_cell<'r, R, T, F>(
    row: &'r R,
    index: usize,
    convert: F,
    kind: &'static str,
) -> Result<DataValue>
where
    R: Row,
    usize: sqlx::ColumnIndex<R>,
    T: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    F: FnOnce(T) -> DataValue,
{
    let context = match kind {
        "boolean" => "boolean conversion failed",
        "float" => "float conversion failed",
        "date" => "date conversion failed",
        "time" => "time conversion failed",
        "date-time" => "date-time conversion failed",
        "decimal" => "decimal conversion failed",
        "JSON" => "JSON conversion failed",
        "UUID" => "UUID conversion failed",
        "text" => "text conversion failed",
        "integer" => "integer conversion failed",
        "binary" => "binary conversion failed",
        _ => "cell conversion failed",
    };
    row.try_get(index)
        .map(convert)
        .map_err(|error| DataError::database(context, &error))
}

fn decode_integer_cell<'r, T, R>(row: &'r R, index: usize) -> Result<DataValue>
where
    R: Row,
    usize: sqlx::ColumnIndex<R>,
    T: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database> + ToString,
{
    decode_cell(
        row,
        index,
        |value: T| DataValue::Integer(value.to_string()),
        "integer",
    )
}

fn decode_binary_cell<'r, R>(row: &'r R, index: usize) -> Result<DataValue>
where
    R: Row,
    usize: sqlx::ColumnIndex<R>,
    Vec<u8>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    decode_cell(
        row,
        index,
        |value: Vec<u8>| DataValue::Binary(base64::engine::general_purpose::STANDARD.encode(value)),
        "binary",
    )
}

fn format_naive_time(value: NaiveTime) -> String {
    value.format("%H:%M:%S%.f").to_string()
}

fn format_naive_date_time(value: NaiveDateTime) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.f").to_string()
}

fn format_utc_date_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn format_mysql_time(value: MySqlTime) -> String {
    let sign = if !value.is_zero() && value.is_negative() {
        "-"
    } else {
        ""
    };
    let mut formatted = format!(
        "{sign}{:02}:{:02}:{:02}",
        value.hours(),
        value.minutes(),
        value.seconds()
    );
    if value.microseconds() != 0 {
        let fraction = format!("{:06}", value.microseconds());
        formatted.push('.');
        formatted.push_str(fraction.trim_end_matches('0'));
    }
    formatted
}

fn validate_mutation_plan(plan: &MutationPlan) -> Result<()> {
    if classify_sql(&plan.statement) == QueryClassification::ReadOnly {
        Err(DataError::InvalidApproval)
    } else {
        Ok(())
    }
}

async fn apply_postgres_mutation(pool: &PgPool, plan: &MutationPlan) -> Result<u64> {
    validate_mutation_plan(plan)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| DataError::database("transaction start failed", &error))?;
    let result = bind_postgres_query(
        sqlx::query(AssertSqlSafe(plan.statement.as_str())),
        &plan.parameters,
    )?
    .execute(&mut *transaction)
    .await
    .map_err(|error| DataError::database("mutation failed", &error))?;
    transaction
        .commit()
        .await
        .map_err(|error| DataError::database("transaction commit failed", &error))?;
    Ok(result.rows_affected())
}

async fn apply_mysql_mutation(pool: &MySqlPool, plan: &MutationPlan) -> Result<u64> {
    validate_mutation_plan(plan)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| DataError::database("transaction start failed", &error))?;
    let result = bind_mysql_query(
        sqlx::query(AssertSqlSafe(plan.statement.as_str())),
        &plan.parameters,
    )?
    .execute(&mut *transaction)
    .await
    .map_err(|error| DataError::database("mutation failed", &error))?;
    transaction
        .commit()
        .await
        .map_err(|error| DataError::database("transaction commit failed", &error))?;
    Ok(result.rows_affected())
}

async fn apply_sqlite_mutation(pool: &SqlitePool, plan: &MutationPlan) -> Result<u64> {
    validate_mutation_plan(plan)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| DataError::database("transaction start failed", &error))?;
    let result = bind_sqlite_query(
        sqlx::query(AssertSqlSafe(plan.statement.as_str())),
        &plan.parameters,
    )?
    .execute(&mut *transaction)
    .await
    .map_err(|error| DataError::database("mutation failed", &error))?;
    transaction
        .commit()
        .await
        .map_err(|error| DataError::database("transaction commit failed", &error))?;
    Ok(result.rows_affected())
}

const MAX_MONGO_QUERY_BYTES: usize = 1024 * 1024;
const MAX_MONGO_PIPELINE_STAGES: usize = 100;
const MAX_MONGO_STAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MongoExplainVerbosity {
    QueryPlanner,
    ExecutionStats,
    AllPlansExecution,
}

impl MongoExplainVerbosity {
    const fn command_value(self) -> &'static str {
        match self {
            Self::QueryPlanner => "queryPlanner",
            Self::ExecutionStats => "executionStats",
            Self::AllPlansExecution => "allPlansExecution",
        }
    }
}

#[derive(Debug, PartialEq)]
struct MongoFind {
    collection: String,
    filter: Document,
    projection: Option<Document>,
    sort: Option<Document>,
    limit: usize,
    explain: Option<MongoExplainVerbosity>,
}

#[derive(Debug, PartialEq)]
struct MongoAggregate {
    collection: String,
    pipeline: Vec<Document>,
    limit: usize,
    explain: Option<MongoExplainVerbosity>,
}

#[derive(Debug, PartialEq)]
enum MongoReadQuery {
    Find(Box<MongoFind>),
    Aggregate(MongoAggregate),
}

fn parse_mongo_read_query(query: &str, request_limit: usize) -> Result<MongoReadQuery> {
    if query.len() > MAX_MONGO_QUERY_BYTES {
        return Err(DataError::InvalidQuery(format!(
            "MongoDB query exceeds {MAX_MONGO_QUERY_BYTES} bytes"
        )));
    }
    let value: serde_json::Value = serde_json::from_str(query)
        .map_err(|_| DataError::InvalidQuery("MongoDB query must be a JSON object".to_owned()))?;
    let mut body = value
        .as_object()
        .cloned()
        .ok_or_else(|| DataError::InvalidQuery("MongoDB query must be a JSON object".to_owned()))?;
    let operation = match body.remove("operation") {
        None => "find",
        Some(serde_json::Value::String(value)) if value == "find" => "find",
        Some(serde_json::Value::String(value)) if value == "aggregate" => "aggregate",
        Some(serde_json::Value::String(_)) => {
            return Err(DataError::InvalidQuery(
                "operation must be find or aggregate".to_owned(),
            ));
        }
        Some(_) => {
            return Err(DataError::InvalidQuery(
                "operation must be a string".to_owned(),
            ));
        }
    };

    let allowed_fields: &[&str] = match operation {
        "find" => &[
            "collection",
            "filter",
            "projection",
            "sort",
            "limit",
            "explain",
        ],
        "aggregate" => &["collection", "pipeline", "limit", "explain"],
        _ => unreachable!("operation is validated above"),
    };
    if let Some(field) = body
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        let field = safe_mongo_field_name(field);
        return Err(DataError::InvalidQuery(format!(
            "field {field} is unknown or incompatible with the {operation} operation"
        )));
    }

    let collection = parse_mongo_collection(&mut body)?;
    let limit = parse_mongo_body_limit(&mut body, request_limit)?;
    let explain = parse_mongo_explain(&mut body)?;
    match operation {
        "find" => Ok(MongoReadQuery::Find(Box::new(MongoFind {
            collection,
            filter: parse_mongo_document(&mut body, "filter")?.unwrap_or_default(),
            projection: parse_mongo_document(&mut body, "projection")?,
            sort: parse_mongo_document(&mut body, "sort")?,
            limit,
            explain,
        }))),
        "aggregate" => Ok(MongoReadQuery::Aggregate(MongoAggregate {
            collection,
            pipeline: parse_mongo_pipeline(&mut body)?,
            limit,
            explain,
        })),
        _ => unreachable!("operation is validated above"),
    }
}

fn safe_mongo_field_name(field: &str) -> String {
    if field.len() <= 64
        && field
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        format!("'{field}'")
    } else {
        "<unrecognized>".to_owned()
    }
}

fn parse_mongo_collection(body: &mut serde_json::Map<String, serde_json::Value>) -> Result<String> {
    let collection = body
        .remove("collection")
        .ok_or_else(|| DataError::InvalidQuery("collection is required".to_owned()))?;
    let collection = collection
        .as_str()
        .ok_or_else(|| DataError::InvalidQuery("collection must be a string".to_owned()))?;
    if collection.trim().is_empty() {
        return Err(DataError::InvalidQuery(
            "collection must not be blank".to_owned(),
        ));
    }
    Ok(collection.to_owned())
}

fn parse_mongo_body_limit(
    body: &mut serde_json::Map<String, serde_json::Value>,
    request_limit: usize,
) -> Result<usize> {
    let Some(value) = body.remove("limit") else {
        return Ok(request_limit);
    };
    let limit = value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| DataError::InvalidQuery("limit must be a positive integer".to_owned()))?;
    if limit == 0 {
        return Err(DataError::InvalidQuery(
            "limit must be a positive integer".to_owned(),
        ));
    }
    if limit > request_limit {
        return Err(DataError::InvalidQuery(format!(
            "limit must not exceed the request limit ({request_limit})"
        )));
    }
    Ok(limit)
}

fn parse_mongo_explain(
    body: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<Option<MongoExplainVerbosity>> {
    let Some(value) = body.remove("explain") else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| DataError::InvalidQuery("explain must be a string".to_owned()))?;
    match value {
        "query_planner" => Ok(Some(MongoExplainVerbosity::QueryPlanner)),
        "execution_stats" => Ok(Some(MongoExplainVerbosity::ExecutionStats)),
        "all_plans_execution" => Ok(Some(MongoExplainVerbosity::AllPlansExecution)),
        _ => Err(DataError::InvalidQuery(
            "explain must be query_planner, execution_stats, or all_plans_execution".to_owned(),
        )),
    }
}

fn parse_mongo_document(
    body: &mut serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> Result<Option<Document>> {
    let Some(value) = body.remove(field) else {
        return Ok(None);
    };
    if !value.is_object() {
        return Err(DataError::InvalidQuery(format!(
            "{field} must be a JSON object"
        )));
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|_| DataError::InvalidQuery(format!("{field} contains an invalid BSON value")))
}

fn parse_mongo_pipeline(
    body: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<Document>> {
    let value = body
        .remove("pipeline")
        .ok_or_else(|| DataError::InvalidQuery("pipeline is required".to_owned()))?;
    let stages = value
        .as_array()
        .ok_or_else(|| DataError::InvalidQuery("pipeline must be an array".to_owned()))?;
    if stages.len() > MAX_MONGO_PIPELINE_STAGES {
        return Err(DataError::InvalidQuery(format!(
            "pipeline exceeds {MAX_MONGO_PIPELINE_STAGES} stages"
        )));
    }
    stages
        .iter()
        .map(|stage| {
            let object = stage.as_object().ok_or_else(|| {
                DataError::InvalidQuery(
                    "each pipeline stage must be a single-key JSON object".to_owned(),
                )
            })?;
            if object.len() != 1 || object.keys().next().is_none_or(|key| !key.starts_with('$')) {
                return Err(DataError::InvalidQuery(
                    "each pipeline stage must be a single MongoDB stage operator".to_owned(),
                ));
            }
            if contains_mongo_write_stage(stage) {
                return Err(DataError::ReadOnlyViolation(
                    "$out and $merge are not allowed in read queries".to_owned(),
                ));
            }
            if serde_json::to_vec(stage).map_or(true, |bytes| bytes.len() > MAX_MONGO_STAGE_BYTES) {
                return Err(DataError::InvalidQuery(format!(
                    "pipeline stage exceeds {MAX_MONGO_STAGE_BYTES} bytes"
                )));
            }
            serde_json::from_value(stage.clone()).map_err(|_| {
                DataError::InvalidQuery("pipeline stage contains an invalid BSON value".to_owned())
            })
        })
        .collect()
}

fn contains_mongo_write_stage(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(key.as_str(), "$out" | "$merge") || contains_mongo_write_stage(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_mongo_write_stage),
        _ => false,
    }
}

async fn execute_mongo_query(
    client: &MongoClient,
    database: &str,
    request: &QueryRequest,
) -> Result<QueryResultPage> {
    let request_limit = request.validated_limit()?;
    let parsed = parse_mongo_read_query(&request.query, request_limit)?;
    let started = Instant::now();
    let query_timeout = request.timeout();
    let (limit, future) = match parsed {
        MongoReadQuery::Find(find) => {
            let limit = find.limit;
            let future = execute_mongo_find(client, database, *find, request.offset, query_timeout);
            (limit, futures_util::future::Either::Left(future))
        }
        MongoReadQuery::Aggregate(aggregate) => {
            let limit = aggregate.limit;
            let future =
                execute_mongo_aggregate(client, database, aggregate, request.offset, query_timeout);
            (limit, futures_util::future::Either::Right(future))
        }
    };
    let documents = timeout(query_timeout, future)
        .await
        .map_err(|_| DataError::Timeout(query_timeout))??;
    Ok(shape_mongo_documents(
        documents,
        limit,
        request.offset,
        elapsed_ms(started),
    ))
}

async fn execute_mongo_find(
    client: &MongoClient,
    database: &str,
    find: MongoFind,
    offset: usize,
    query_timeout: std::time::Duration,
) -> Result<Vec<Document>> {
    let db = client.database(database);
    if let Some(verbosity) = find.explain {
        let mut command = doc! {
            "find": find.collection,
            "filter": find.filter,
            "skip": i64::try_from(offset).unwrap_or(i64::MAX),
            "limit": i64::try_from(find.limit + 1).unwrap_or(i64::MAX),
            "maxTimeMS": i64::try_from(query_timeout.as_millis()).unwrap_or(i64::MAX),
        };
        if let Some(projection) = find.projection {
            command.insert("projection", projection);
        }
        if let Some(sort) = find.sort {
            command.insert("sort", sort);
        }
        return db
            .run_command(doc! {
                "explain": command,
                "verbosity": verbosity.command_value(),
            })
            .await
            .map(|document| vec![document])
            .map_err(|error| DataError::database("MongoDB explain failed", &error));
    }

    let collection = db.collection::<Document>(&find.collection);
    let mut action = collection
        .find(find.filter)
        .skip(u64::try_from(offset).unwrap_or(u64::MAX))
        .limit(i64::try_from(find.limit + 1).unwrap_or(i64::MAX))
        .max_time(query_timeout);
    if let Some(projection) = find.projection {
        action = action.projection(projection);
    }
    if let Some(sort) = find.sort {
        action = action.sort(sort);
    }
    let cursor = action
        .await
        .map_err(|error| DataError::database("MongoDB query failed", &error))?;
    collect_mongo_documents(cursor).await
}

async fn execute_mongo_aggregate(
    client: &MongoClient,
    database: &str,
    aggregate: MongoAggregate,
    offset: usize,
    query_timeout: std::time::Duration,
) -> Result<Vec<Document>> {
    let db = client.database(database);
    let pipeline = paginated_mongo_pipeline(aggregate.pipeline, offset, aggregate.limit);
    if let Some(verbosity) = aggregate.explain {
        return db
            .run_command(doc! {
                "explain": {
                    "aggregate": aggregate.collection,
                    "pipeline": pipeline,
                    "cursor": {},
                    "maxTimeMS": i64::try_from(query_timeout.as_millis()).unwrap_or(i64::MAX),
                },
                "verbosity": verbosity.command_value(),
            })
            .await
            .map(|document| vec![document])
            .map_err(|error| DataError::database("MongoDB explain failed", &error));
    }

    let cursor = db
        .collection::<Document>(&aggregate.collection)
        .aggregate(pipeline)
        .max_time(query_timeout)
        .await
        .map_err(|error| DataError::database("MongoDB query failed", &error))?;
    collect_mongo_documents(cursor).await
}

fn paginated_mongo_pipeline(
    mut pipeline: Vec<Document>,
    offset: usize,
    limit: usize,
) -> Vec<Document> {
    if offset > 0 {
        pipeline.push(doc! { "$skip": i64::try_from(offset).unwrap_or(i64::MAX) });
    }
    pipeline.push(doc! { "$limit": i64::try_from(limit + 1).unwrap_or(i64::MAX) });
    pipeline
}

async fn collect_mongo_documents(mut cursor: mongodb::Cursor<Document>) -> Result<Vec<Document>> {
    let mut documents = Vec::new();
    while let Some(document) = cursor
        .try_next()
        .await
        .map_err(|error| DataError::database("MongoDB result failed", &error))?
    {
        documents.push(document);
    }
    Ok(documents)
}

fn mongo_data_value(value: Bson) -> DataValue {
    match value {
        Bson::Binary(binary) if binary.subtype != mongodb::bson::spec::BinarySubtype::Generic => {
            DataValue::Bson(Bson::Binary(binary).into_canonical_extjson())
        }
        other => DataValue::from_bson(other),
    }
}

fn shape_mongo_documents(
    mut documents: Vec<Document>,
    limit: usize,
    offset: usize,
    elapsed_ms: u64,
) -> QueryResultPage {
    let truncated = documents.len() > limit;
    if truncated {
        documents.truncate(limit);
    }
    let field_names: Vec<String> = documents
        .iter()
        .flat_map(|document| document.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let columns = field_names
        .iter()
        .map(|name| QueryColumn {
            name: name.clone(),
            data_type: "bson".to_owned(),
            nullable: None,
        })
        .collect();
    let rows = documents
        .into_iter()
        .map(|document| {
            field_names
                .iter()
                .map(|name| {
                    document
                        .get(name)
                        .cloned()
                        .map_or(DataValue::Null, mongo_data_value)
                })
                .collect()
        })
        .collect::<Vec<_>>();
    QueryResultPage {
        columns,
        stats: QueryStats {
            elapsed_ms,
            rows_returned: rows.len(),
            rows_affected: 0,
            truncated,
        },
        next_offset: truncated.then_some(offset + rows.len()),
        rows,
    }
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum MongoMutation {
    InsertOne {
        collection: String,
        document: Document,
    },
    UpdateMany {
        collection: String,
        filter: Document,
        update: Document,
    },
    DeleteMany {
        collection: String,
        filter: Document,
    },
}

fn parse_mongo_mutation(statement: &str) -> Result<MongoMutation> {
    serde_json::from_str(statement)
        .map_err(|_| DataError::InvalidQuery("expected a MongoDB mutation object".to_owned()))
}

fn mongo_mutation_risk(statement: &str) -> Result<MutationRisk> {
    Ok(match parse_mongo_mutation(statement)? {
        MongoMutation::InsertOne { .. } => MutationRisk::Elevated,
        MongoMutation::UpdateMany { .. } | MongoMutation::DeleteMany { .. } => {
            MutationRisk::Destructive
        }
    })
}

async fn apply_mongo_mutation(
    client: &MongoClient,
    database: &str,
    statement: &str,
) -> Result<u64> {
    let db = client.database(database);
    match parse_mongo_mutation(statement)? {
        MongoMutation::InsertOne {
            collection,
            document,
        } => {
            db.collection::<Document>(&collection)
                .insert_one(document)
                .await
                .map_err(|error| DataError::database("MongoDB insert failed", &error))?;
            Ok(1)
        }
        MongoMutation::UpdateMany {
            collection,
            filter,
            update,
        } => db
            .collection::<Document>(&collection)
            .update_many(filter, update)
            .await
            .map(|result| result.modified_count)
            .map_err(|error| DataError::database("MongoDB update failed", &error)),
        MongoMutation::DeleteMany { collection, filter } => db
            .collection::<Document>(&collection)
            .delete_many(filter)
            .await
            .map(|result| result.deleted_count)
            .map_err(|error| DataError::database("MongoDB delete failed", &error)),
    }
}

fn bson_type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Array(_) => "array",
        Bson::Document(_) => "document",
        Bson::Boolean(_) => "boolean",
        Bson::Null => "null",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::Decimal128(_) => "decimal128",
        Bson::DateTime(_) => "datetime",
        Bson::Binary(_) => "binary",
        Bson::ObjectId(_) => "object_id",
        _ => "bson",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AiChatRequest, AiMessage, AiRole, secret::MemorySecretStore};
    use secrecy::{ExposeSecret, SecretString};
    use std::{
        fs,
        sync::{
            Arc, Barrier, Mutex as StdMutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    #[derive(Default)]
    struct MemoryFaultProfileRepository {
        state: StdMutex<MemoryProfileState>,
        crash_after: StdMutex<Option<PersistencePhase>>,
    }

    #[derive(Default)]
    struct MemoryProfileState {
        profiles: Option<Vec<u8>>,
        journal: Option<Vec<u8>>,
    }

    impl MemoryFaultProfileRepository {
        fn crash_after(&self, phase: PersistencePhase) {
            *self.crash_after.lock().unwrap() = Some(phase);
        }

        fn disarm(&self) {
            *self.crash_after.lock().unwrap() = None;
        }

        fn journal(&self) -> Option<ProfileJournal> {
            self.state
                .lock()
                .unwrap()
                .journal
                .as_deref()
                .map(deserialize_journal)
                .transpose()
                .unwrap()
        }

        fn journal_bytes(&self) -> Option<Vec<u8>> {
            self.state.lock().unwrap().journal.clone()
        }
    }

    impl ProfileRepository for MemoryFaultProfileRepository {
        fn read_profiles(&self) -> Result<Option<Vec<u8>>> {
            Ok(self.state.lock().unwrap().profiles.clone())
        }

        fn replace_profiles(&self, bytes: &[u8]) -> Result<()> {
            self.state.lock().unwrap().profiles = Some(bytes.to_vec());
            Ok(())
        }

        fn read_journal(&self) -> Result<Option<Vec<u8>>> {
            Ok(self.state.lock().unwrap().journal.clone())
        }

        fn replace_journal(&self, bytes: &[u8]) -> Result<()> {
            self.state.lock().unwrap().journal = Some(bytes.to_vec());
            Ok(())
        }

        fn remove_journal(&self) -> Result<()> {
            self.state.lock().unwrap().journal = None;
            Ok(())
        }

        fn checkpoint(&self, phase: PersistencePhase) {
            let should_crash = self
                .crash_after
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|armed| *armed == phase);
            assert!(
                !should_crash,
                "simulated process interruption after {phase:?}"
            );
        }
    }

    #[derive(Default)]
    struct FaultInjectingSecretStore {
        values: StdMutex<HashMap<Uuid, SecretString>>,
        fail_next_set_after_write: AtomicBool,
    }

    impl FaultInjectingSecretStore {
        fn fail_next_set_after_write(&self) {
            self.fail_next_set_after_write.store(true, Ordering::SeqCst);
        }
    }

    impl SecretStore for FaultInjectingSecretStore {
        fn get(&self, id: Uuid) -> Result<Option<SecretString>> {
            Ok(self
                .values
                .lock()
                .map_err(|_| DataError::SecretUnavailable)?
                .get(&id)
                .cloned())
        }

        fn set(&self, id: Uuid, secret: &str) -> Result<()> {
            self.values
                .lock()
                .map_err(|_| DataError::SecretUnavailable)?
                .insert(id, SecretString::from(secret.to_owned()));
            if self.fail_next_set_after_write.swap(false, Ordering::SeqCst) {
                return Err(DataError::KeyringUnavailable);
            }
            Ok(())
        }

        fn delete(&self, id: Uuid) -> Result<()> {
            self.values
                .lock()
                .map_err(|_| DataError::SecretUnavailable)?
                .remove(&id);
            Ok(())
        }
    }

    struct CoordinatedDeleteSecretStore {
        values: StdMutex<HashMap<Uuid, SecretString>>,
        delete_started: Arc<Barrier>,
        allow_delete: Arc<Barrier>,
    }

    impl CoordinatedDeleteSecretStore {
        fn new() -> (Self, Arc<Barrier>, Arc<Barrier>) {
            let delete_started = Arc::new(Barrier::new(2));
            let allow_delete = Arc::new(Barrier::new(2));
            (
                Self {
                    values: StdMutex::new(HashMap::new()),
                    delete_started: Arc::clone(&delete_started),
                    allow_delete: Arc::clone(&allow_delete),
                },
                delete_started,
                allow_delete,
            )
        }
    }

    impl SecretStore for CoordinatedDeleteSecretStore {
        fn get(&self, id: Uuid) -> Result<Option<SecretString>> {
            Ok(self
                .values
                .lock()
                .map_err(|_| DataError::SecretUnavailable)?
                .get(&id)
                .cloned())
        }

        fn set(&self, id: Uuid, secret: &str) -> Result<()> {
            self.values
                .lock()
                .map_err(|_| DataError::SecretUnavailable)?
                .insert(id, SecretString::from(secret.to_owned()));
            Ok(())
        }

        fn delete(&self, id: Uuid) -> Result<()> {
            self.values
                .lock()
                .map_err(|_| DataError::SecretUnavailable)?
                .remove(&id);
            self.delete_started.wait();
            self.allow_delete.wait();
            Ok(())
        }
    }

    struct UnavailableReadSecretStore;

    impl SecretStore for UnavailableReadSecretStore {
        fn get(&self, _id: Uuid) -> Result<Option<SecretString>> {
            Err(DataError::KeyringUnavailable)
        }

        fn set(&self, _id: Uuid, _secret: &str) -> Result<()> {
            Err(DataError::KeyringUnavailable)
        }

        fn delete(&self, _id: Uuid) -> Result<()> {
            Err(DataError::KeyringUnavailable)
        }
    }

    fn service() -> DataService {
        DataService::with_secret_store(Arc::new(MemorySecretStore::default()))
    }

    fn query(sql: &str) -> QueryRequest {
        QueryRequest {
            connection_id: DEMO_PROFILE_ID,
            query: sql.to_owned(),
            language: QueryLanguage::Sql,
            parameters: Vec::new(),
            limit: 100,
            offset: 0,
            timeout_ms: 5_000,
        }
    }

    fn mongo_query(value: &serde_json::Value, limit: usize, offset: usize) -> QueryRequest {
        QueryRequest {
            connection_id: DEMO_PROFILE_ID,
            query: value.to_string(),
            language: QueryLanguage::MongoJson,
            parameters: Vec::new(),
            limit,
            offset,
            timeout_ms: 5_000,
        }
    }

    fn mongo_result_value<'a>(
        page: &'a QueryResultPage,
        row: usize,
        column: &str,
    ) -> &'a DataValue {
        let index = page
            .columns
            .iter()
            .position(|candidate| candidate.name == column)
            .unwrap_or_else(|| panic!("missing MongoDB result column: {column}"));
        &page.rows[row][index]
    }

    fn assert_sqlx_codec<DB, T>()
    where
        DB: sqlx::Database,
        T: sqlx::Type<DB>,
        for<'q> T: sqlx::Encode<'q, DB>,
        for<'r> T: sqlx::Decode<'r, DB>,
    {
    }

    #[test]
    fn native_scalar_types_have_compile_time_sqlx_codecs() {
        assert_sqlx_codec::<Postgres, BigDecimal>();
        assert_sqlx_codec::<MySql, BigDecimal>();
        assert_sqlx_codec::<Postgres, NaiveDate>();
        assert_sqlx_codec::<MySql, NaiveDate>();
        assert_sqlx_codec::<Postgres, NaiveTime>();
        assert_sqlx_codec::<MySql, NaiveTime>();
        assert_sqlx_codec::<MySql, MySqlTime>();
        assert_sqlx_codec::<Postgres, NaiveDateTime>();
        assert_sqlx_codec::<MySql, NaiveDateTime>();
        assert_sqlx_codec::<Postgres, DateTime<Utc>>();
        assert_sqlx_codec::<MySql, DateTime<Utc>>();
        assert_sqlx_codec::<Postgres, Uuid>();
        assert_sqlx_codec::<Postgres, serde_json::Value>();
        assert_sqlx_codec::<MySql, serde_json::Value>();
    }

    #[test]
    fn mysql_bit_values_are_always_exact_integers() {
        for (bytes, expected) in [
            (&[0_u8][..], "0"),
            (&[1_u8][..], "1"),
            (&[2_u8][..], "2"),
            (&[5_u8][..], "5"),
            (&[1_u8, 0_u8][..], "256"),
        ] {
            assert_eq!(
                mysql_bit_value(bytes),
                DataValue::Integer(expected.to_owned())
            );
        }
    }

    #[test]
    fn backend_binders_accept_supported_data_value_variants() {
        assert_eq!(PgNull::type_info().oid(), Some(Oid(0)));
        let id = Uuid::parse_str("12345678-1234-5678-90ab-1234567890ab").unwrap();
        let common = vec![
            DataValue::Null,
            DataValue::Boolean(true),
            DataValue::Integer("9223372036854775807".to_owned()),
            DataValue::Float(1.25),
            DataValue::Decimal("12345678901234567890.0012300".to_owned()),
            DataValue::Text("hello".to_owned()),
            DataValue::Binary("AP8=".to_owned()),
            DataValue::Date("2025-02-03".to_owned()),
            DataValue::Time("23:59:58.123456".to_owned()),
            DataValue::DateTime("2025-02-03T04:05:06.700800Z".to_owned()),
            DataValue::Uuid(id),
            DataValue::Json(serde_json::json!({"safe": true})),
            DataValue::Bson(serde_json::json!({"$numberLong": "7"})),
        ];
        assert!(bind_postgres_query(sqlx::query("SELECT 1"), &common).is_ok());
        assert!(bind_mysql_query(sqlx::query("SELECT 1"), &common).is_ok());
        assert!(bind_sqlite_query(sqlx::query("SELECT 1"), &common).is_ok());
        assert!(
            bind_mysql_query(
                sqlx::query("SELECT ?"),
                &[DataValue::Integer(u64::MAX.to_string())],
            )
            .is_ok()
        );
    }

    #[test]
    fn native_temporal_formatting_is_stable_and_exact() {
        let time = NaiveTime::from_hms_micro_opt(7, 8, 9, 120_300).unwrap();
        assert_eq!(format_naive_time(time), "07:08:09.120300");
        let date_time = NaiveDate::from_ymd_opt(2025, 2, 3)
            .unwrap()
            .and_hms_micro_opt(4, 5, 6, 700_800)
            .unwrap();
        assert_eq!(
            format_naive_date_time(date_time),
            "2025-02-03T04:05:06.700800"
        );
        assert_eq!(
            format_utc_date_time(date_time.and_utc()),
            "2025-02-03T04:05:06.700800Z"
        );
        assert_eq!(format_mysql_time(MySqlTime::ZERO), "00:00:00");
    }

    #[test]
    fn exact_decimal_and_parameter_parsers_preserve_units() {
        let decimal = "123456789012345678901234567890.0012300";
        assert_eq!(
            parse_decimal_parameter(decimal).unwrap().to_string(),
            decimal
        );
        assert_eq!(
            parse_time_parameter("23:59:58.123456").unwrap(),
            NaiveTime::from_hms_micro_opt(23, 59, 58, 123_456).unwrap()
        );
        assert_eq!(
            parse_date_time_parameter("2025-02-03T04:05:06.700800Z").unwrap(),
            ParsedDateTime::Utc(
                NaiveDate::from_ymd_opt(2025, 2, 3)
                    .unwrap()
                    .and_hms_micro_opt(4, 5, 6, 700_800)
                    .unwrap()
                    .and_utc()
            )
        );
        assert_eq!(
            parse_date_time_parameter("2025-02-03 04:05:06.700800").unwrap(),
            ParsedDateTime::Naive(
                NaiveDate::from_ymd_opt(2025, 2, 3)
                    .unwrap()
                    .and_hms_micro_opt(4, 5, 6, 700_800)
                    .unwrap()
            )
        );
    }

    #[tokio::test]
    async fn live_postgres_native_scalars_when_configured() {
        let Ok(url) = std::env::var("REDROB_TEST_POSTGRES_URL") else {
            return;
        };
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let result = execute_postgres_query(
            &pool,
            &query(
                "SELECT NULL::text, TRUE, -32768::int2, 2147483647::int4, 9223372036854775807::int8, 1.5::float4, 2.5::float8, decode('AP8=', 'base64')::bytea, DATE '2025-02-03', TIME '23:59:58.123456', TIMESTAMP '2025-02-03 04:05:06.700800', TIMESTAMPTZ '2025-02-03 04:05:06.700800+02', 123456789012345678901234567890.0012300::numeric, '{\"safe\":true}'::json, '{\"exact\":\"9007199254740993\"}'::jsonb, '12345678-1234-5678-90ab-1234567890ab'::uuid, 'hello'::text",
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            result.rows[0],
            vec![
                DataValue::Null,
                DataValue::Boolean(true),
                DataValue::Integer("-32768".to_owned()),
                DataValue::Integer("2147483647".to_owned()),
                DataValue::Integer("9223372036854775807".to_owned()),
                DataValue::Float(1.5),
                DataValue::Float(2.5),
                DataValue::Binary("AP8=".to_owned()),
                DataValue::Date("2025-02-03".to_owned()),
                DataValue::Time("23:59:58.123456".to_owned()),
                DataValue::DateTime("2025-02-03T04:05:06.700800".to_owned()),
                DataValue::DateTime("2025-02-03T02:05:06.700800Z".to_owned()),
                DataValue::Decimal("123456789012345678901234567890.0012300".to_owned()),
                DataValue::Json(serde_json::json!({"safe": true})),
                DataValue::Json(serde_json::json!({"exact": "9007199254740993"})),
                DataValue::Uuid(Uuid::parse_str("12345678-1234-5678-90ab-1234567890ab").unwrap(),),
                DataValue::Text("hello".to_owned()),
            ]
        );

        let mut null_request = query("SELECT $1::INTEGER");
        null_request.parameters = vec![DataValue::Null];
        assert_eq!(
            execute_postgres_query(&pool, &null_request)
                .await
                .unwrap()
                .rows[0][0],
            DataValue::Null
        );
        let mut timestamp_request = query("SELECT $1::TIMESTAMPTZ");
        timestamp_request.parameters = vec![DataValue::DateTime(
            "2025-02-03T04:05:06.700800+02:00".to_owned(),
        )];
        assert_eq!(
            execute_postgres_query(&pool, &timestamp_request)
                .await
                .unwrap()
                .rows[0][0],
            DataValue::DateTime("2025-02-03T02:05:06.700800Z".to_owned())
        );
        pool.close().await;
    }

    async fn assert_live_mysql_tinyints_and_file_output_rejection(pool: &MySqlPool) {
        // MySQL's BOOLEAN alias is protocol-equivalent to signed TINYINT, so neither
        // declaration can be distinguished reliably and both decode as integers.
        sqlx::query(
            "CREATE TEMPORARY TABLE redrob_tinyint_values (boolean_alias BOOLEAN NOT NULL, signed_tinyint TINYINT NOT NULL)",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO redrob_tinyint_values VALUES (0, 0), (1, 1), (2, 2)")
            .execute(pool)
            .await
            .unwrap();
        let tinyints = execute_mysql_query(
            pool,
            &query(
                "SELECT boolean_alias, signed_tinyint FROM redrob_tinyint_values ORDER BY signed_tinyint",
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            tinyints.rows,
            vec![
                vec![
                    DataValue::Integer("0".to_owned()),
                    DataValue::Integer("0".to_owned()),
                ],
                vec![
                    DataValue::Integer("1".to_owned()),
                    DataValue::Integer("1".to_owned()),
                ],
                vec![
                    DataValue::Integer("2".to_owned()),
                    DataValue::Integer("2".to_owned()),
                ],
            ]
        );

        sqlx::query(
            "CREATE TEMPORARY TABLE redrob_bit_values (bit_zero BIT(1) NOT NULL, bit_one BIT(1) NOT NULL, bit_two BIT(2) NOT NULL, bit_five BIT(5) NOT NULL)",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO redrob_bit_values VALUES (b'0', b'1', b'10', b'00101')")
            .execute(pool)
            .await
            .unwrap();
        let bits = execute_mysql_query(
            pool,
            &query("SELECT bit_zero, bit_one, bit_two, bit_five FROM redrob_bit_values"),
        )
        .await
        .unwrap();
        assert_eq!(
            bits.rows,
            vec![vec![
                DataValue::Integer("0".to_owned()),
                DataValue::Integer("1".to_owned()),
                DataValue::Integer("2".to_owned()),
                DataValue::Integer("5".to_owned()),
            ]]
        );

        for sql in [
            "SELECT 1 INTO OUTFILE '/tmp/redrob-must-not-exist.csv'",
            "SELECT 1 INTO /* server file */ DUMPFILE '/tmp/redrob-must-not-exist.bin'",
            "SELECT 1 INTO # server file follows\n OUTFILE '/tmp/redrob-must-not-exist.csv'",
            "SELECT 1 /*!50000 INTO DUMPFILE '/tmp/redrob-must-not-exist.bin' */",
            "SELECT 1 INTO /*!50000 OUTFILE */ '/tmp/redrob-must-not-exist.csv'",
            "SELECT 1 InTo /* separator */ /*!50000\nDuMpFiLe */ '/tmp/redrob-must-not-exist.bin'",
        ] {
            assert!(
                matches!(
                    execute_mysql_query(pool, &query(sql)).await,
                    Err(DataError::ReadOnlyViolation(_))
                ),
                "query should be rejected before reaching MySQL: {sql}"
            );
        }
    }

    #[tokio::test]
    async fn live_mysql_native_scalars_when_configured() {
        let Ok(url) = std::env::var("REDROB_TEST_MYSQL_URL") else {
            return;
        };
        let pool = MySqlPoolOptions::new()
            .max_connections(1)
            .after_connect(|connection, _metadata| {
                Box::pin(async move {
                    sqlx::query("SET time_zone = '+00:00'")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        sqlx::query("DROP TEMPORARY TABLE IF EXISTS redrob_scalar_values")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TEMPORARY TABLE redrob_scalar_values (nullable INT NULL, bool_value BOOLEAN NOT NULL, bit_value BIT(8) NOT NULL, signed_value BIGINT NOT NULL, unsigned_value BIGINT UNSIGNED NOT NULL, float_value FLOAT NOT NULL, double_value DOUBLE NOT NULL, bytes_value BLOB NOT NULL, date_value DATE NOT NULL, time_value TIME(6) NOT NULL, datetime_value DATETIME(6) NOT NULL, timestamp_value TIMESTAMP(6) NOT NULL, decimal_value DECIMAL(65,20) NOT NULL, json_value JSON NOT NULL, text_value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO redrob_scalar_values VALUES (NULL, TRUE, b'101', -9223372036854775808, 18446744073709551615, 1.5, 2.5, X'00FF', '2025-02-03', '-25:02:03.123456', '2025-02-03 04:05:06.700800', '2025-02-03 04:05:06.700800', 123456789012345678901234567890.00123000000000000000, '{\"safe\": true}', 'hello')")
            .execute(&pool)
            .await
            .unwrap();
        let result = execute_mysql_query(&pool, &query("SELECT * FROM redrob_scalar_values"))
            .await
            .unwrap();
        assert_eq!(
            result.rows[0],
            vec![
                DataValue::Null,
                DataValue::Integer("1".to_owned()),
                DataValue::Integer("5".to_owned()),
                DataValue::Integer("-9223372036854775808".to_owned()),
                DataValue::Integer(u64::MAX.to_string()),
                DataValue::Float(1.5),
                DataValue::Float(2.5),
                DataValue::Binary("AP8=".to_owned()),
                DataValue::Date("2025-02-03".to_owned()),
                DataValue::Time("-25:02:03.123456".to_owned()),
                DataValue::DateTime("2025-02-03T04:05:06.700800".to_owned()),
                DataValue::DateTime("2025-02-03T04:05:06.700800Z".to_owned()),
                DataValue::Decimal(
                    "123456789012345678901234567890.00123000000000000000".to_owned(),
                ),
                DataValue::Json(serde_json::json!({"safe": true})),
                DataValue::Text("hello".to_owned()),
            ]
        );

        assert_live_mysql_tinyints_and_file_output_rejection(&pool).await;

        let mut parameter_request = query("SELECT CAST(? AS UNSIGNED), CAST(? AS DATETIME(6))");
        parameter_request.parameters = vec![
            DataValue::Integer(u64::MAX.to_string()),
            DataValue::DateTime("2025-02-03 04:05:06.700800".to_owned()),
        ];
        let parameters = execute_mysql_query(&pool, &parameter_request)
            .await
            .unwrap();
        assert_eq!(
            parameters.rows[0][0],
            DataValue::Integer(u64::MAX.to_string())
        );
        assert_eq!(
            parameters.rows[0][1],
            DataValue::DateTime("2025-02-03T04:05:06.700800".to_owned())
        );
        pool.close().await;
    }

    #[test]
    fn sqlite_text_type_hints_preserve_semantic_values_when_available() {
        let id = Uuid::parse_str("12345678-1234-5678-90ab-1234567890ab").unwrap();
        assert_eq!(
            sqlite_text_value("DATE", "2025-02-03".to_owned()).unwrap(),
            DataValue::Date("2025-02-03".to_owned())
        );
        assert_eq!(
            sqlite_text_value("TIME", "23:59:58.123456".to_owned()).unwrap(),
            DataValue::Time("23:59:58.123456".to_owned())
        );
        assert_eq!(
            sqlite_text_value("DATETIME", "2025-02-03T04:05:06Z".to_owned()).unwrap(),
            DataValue::DateTime("2025-02-03T04:05:06Z".to_owned())
        );
        assert_eq!(
            sqlite_text_value("JSON", "{\"safe\":true}".to_owned()).unwrap(),
            DataValue::Json(serde_json::json!({"safe": true}))
        );
        assert_eq!(
            sqlite_text_value("UUID", id.to_string()).unwrap(),
            DataValue::Uuid(id)
        );
    }

    #[tokio::test]
    async fn sqlite_dynamic_scalars_and_typed_text_round_trip() {
        let service = service();
        let create = service
            .prepare_mutation(
                DEMO_PROFILE_ID,
                "CREATE TABLE scalar_values (nullable TEXT, enabled BOOLEAN, signed INTEGER, real_value REAL, exact_value DECIMAL TEXT, text_value TEXT, bytes BLOB, date_value DATE TEXT, time_value TIME TEXT, datetime_value DATETIME TEXT, uuid_value UUID TEXT, json_value JSON TEXT, bson_value JSON TEXT)".to_owned(),
                Vec::new(),
            )
            .await
            .unwrap();
        service.apply_mutation(create).await.unwrap();

        let id = Uuid::parse_str("12345678-1234-5678-90ab-1234567890ab").unwrap();
        let parameters = vec![
            DataValue::Null,
            DataValue::Boolean(true),
            DataValue::Integer("-9223372036854775808".to_owned()),
            DataValue::Float(1.25),
            DataValue::Decimal("12345678901234567890.0012300".to_owned()),
            DataValue::Text("hello".to_owned()),
            DataValue::Binary("AP8=".to_owned()),
            DataValue::Date("2025-02-03".to_owned()),
            DataValue::Time("23:59:58.123456".to_owned()),
            DataValue::DateTime("2025-02-03T04:05:06.700800Z".to_owned()),
            DataValue::Uuid(id),
            DataValue::Json(serde_json::json!({"exact": "9007199254740993"})),
            DataValue::Bson(serde_json::json!({"$numberLong": "9223372036854775807"})),
        ];
        let insert = service
            .prepare_mutation(
                DEMO_PROFILE_ID,
                "INSERT INTO scalar_values VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                    .to_owned(),
                parameters.clone(),
            )
            .await
            .unwrap();
        service.apply_mutation(insert).await.unwrap();

        let result = service
            .execute_query(query("SELECT * FROM scalar_values"))
            .await
            .unwrap();
        assert_eq!(
            result.rows,
            vec![vec![
                DataValue::Null,
                DataValue::Boolean(true),
                DataValue::Integer("-9223372036854775808".to_owned()),
                DataValue::Float(1.25),
                DataValue::Text("12345678901234567890.0012300".to_owned()),
                DataValue::Text("hello".to_owned()),
                DataValue::Binary("AP8=".to_owned()),
                DataValue::Text("2025-02-03".to_owned()),
                DataValue::Text("23:59:58.123456".to_owned()),
                DataValue::Text("2025-02-03T04:05:06.700800Z".to_owned()),
                DataValue::Text(id.to_string()),
                DataValue::Text("{\"exact\":\"9007199254740993\"}".to_owned()),
                DataValue::Text("{\"$numberLong\":\"9223372036854775807\"}".to_owned(),),
            ]]
        );

        let metadata = service
            .load_metadata(MetadataRequest {
                connection_id: DEMO_PROFILE_ID,
                parent_id: Some("table:scalar_values".to_owned()),
            })
            .await
            .unwrap();
        assert_eq!(metadata.len(), 13);
        assert_eq!(metadata[0].name, "nullable");
        assert_eq!(metadata[0].nullable, Some(true));
    }

    #[tokio::test]
    async fn sqlite_dynamic_expression_storage_classes_decode_without_declared_types() {
        let service = service();
        let result = service
            .execute_query(query("SELECT NULL, -7, 1.25, X'00FF', 'hello'"))
            .await
            .unwrap();
        assert_eq!(
            result.rows[0],
            vec![
                DataValue::Null,
                DataValue::Integer("-7".to_owned()),
                DataValue::Float(1.25),
                DataValue::Binary("AP8=".to_owned()),
                DataValue::Text("hello".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn demo_profile_metadata_and_query_work_on_first_launch() {
        let service = service();
        let profiles = service.list_profiles().await;
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, DEMO_PROFILE_ID);
        let metadata = service
            .load_metadata(MetadataRequest {
                connection_id: DEMO_PROFILE_ID,
                parent_id: None,
            })
            .await
            .unwrap();
        assert!(metadata.iter().any(|node| node.name == "customers"));
        assert!(metadata.iter().any(|node| node.name == "orders"));
        let result = service
            .execute_query(query("SELECT id, name FROM customers ORDER BY id"))
            .await
            .unwrap();
        assert_eq!(result.rows.len(), 4);
        assert_eq!(
            result.rows[0][1],
            DataValue::Text("Ada Lovelace".to_owned())
        );
    }

    #[tokio::test]
    async fn demo_query_limit_is_enforced() {
        let service = service();
        let mut request = query("SELECT id FROM orders ORDER BY id");
        request.limit = 2;
        let result = service.execute_query(request).await.unwrap();
        assert_eq!(result.rows.len(), 2);
        assert!(result.stats.truncated);
        assert_eq!(result.next_offset, Some(2));
    }

    #[tokio::test]
    async fn mutations_require_exact_single_use_approval() {
        let service = service();
        let statement = "UPDATE customers SET city = 'Paris' WHERE id = 1".to_owned();
        let mut plan = service
            .prepare_mutation(DEMO_PROFILE_ID, statement, Vec::new())
            .await
            .unwrap();
        let original = plan.clone();
        plan.statement.push_str(" OR id = 2");
        assert!(matches!(
            service.apply_mutation(plan).await,
            Err(DataError::InvalidApproval)
        ));
        assert!(matches!(
            service.apply_mutation(original).await,
            Err(DataError::InvalidApproval)
        ));

        let plan = service
            .prepare_mutation(
                DEMO_PROFILE_ID,
                "UPDATE customers SET city = 'Paris' WHERE id = 1".to_owned(),
                Vec::new(),
            )
            .await
            .unwrap();
        let result = service.apply_mutation(plan.clone()).await.unwrap();
        assert_eq!(result.rows_affected, 1);
        assert!(matches!(
            service.apply_mutation(plan).await,
            Err(DataError::InvalidApproval)
        ));
        let rows = service
            .execute_query(query("SELECT city FROM customers WHERE id = 1"))
            .await
            .unwrap();
        assert_eq!(rows.rows[0][0], DataValue::Text("Paris".to_owned()));
    }

    #[tokio::test]
    async fn execute_query_rejects_sqlite_mutation_bypasses_without_changing_data() {
        let service = service();
        let baseline = service
            .execute_query(query(
                "SELECT id, name, email, city FROM customers ORDER BY id",
            ))
            .await
            .unwrap()
            .rows;
        assert_eq!(baseline.len(), 4);

        for sql in [
            "DELETE FROM customers",
            "PRAGMA incremental_vacuum",
            "PRAGMA optimize",
            "PRAGMA wal_checkpoint",
            "PRAGMA shrink_memory",
            "SELECT * FROM customers INTO OUTFILE '/tmp/redrob-must-not-exist.csv'",
            "SELECT name INTO /* server file */ DUMPFILE '/tmp/redrob-must-not-exist.bin' FROM customers",
            "SELECT name INTO # server file follows\n OUTFILE '/tmp/redrob-must-not-exist.csv' FROM customers",
            "SELECT name FROM customers /*!50000 INTO DUMPFILE '/tmp/redrob-must-not-exist.bin' */",
            "SELECT name FROM customers INTO /*!50000 OUTFILE */ '/tmp/redrob-must-not-exist.csv'",
            "SELECT name FROM customers InTo /* separator */ /*!50000\nDuMpFiLe */ '/tmp/redrob-must-not-exist.bin'",
            "SELECT 1; SELECT 2",
            r"SELECT '\'; DELETE FROM customers; -- '",
        ] {
            assert!(
                matches!(
                    service.execute_query(query(sql)).await,
                    Err(DataError::ReadOnlyViolation(_))
                ),
                "query should be rejected: {sql}"
            );
        }

        let quoted = service
            .execute_query(query("SELECT 'INTO /*!50000 OUTFILE */'"))
            .await
            .unwrap();
        assert_eq!(
            quoted.rows[0][0],
            DataValue::Text("INTO /*!50000 OUTFILE */".to_owned())
        );

        let customers = service
            .execute_query(query(
                "SELECT id, name, email, city FROM customers ORDER BY id;",
            ))
            .await
            .unwrap();
        assert_eq!(customers.rows, baseline);
    }

    fn sqlite_profile(id: Uuid, name: &str) -> ConnectionProfile {
        ConnectionProfile {
            id,
            name: name.to_owned(),
            kind: DatabaseKind::SQLite,
            config: ConnectionConfig {
                file_path: Some(":memory:".to_owned()),
                ..ConnectionConfig::default()
            },
            read_only: false,
            built_in: false,
            capabilities: DatabaseKind::SQLite.capabilities(),
        }
    }

    async fn crash_save_and_recover(phase: PersistencePhase) {
        let repository = Arc::new(MemoryFaultProfileRepository::default());
        let secrets = Arc::new(MemorySecretStore::default());
        let id = Uuid::new_v4();
        let service = Arc::new(DataService::persistent_with_repository(
            repository.clone(),
            secrets.clone(),
        ));
        service
            .save_profile_with_secret(sqlite_profile(id, "Original"), Some("old-secret"))
            .await
            .unwrap();

        repository.crash_after(phase);
        let interrupted_service = Arc::clone(&service);
        let interrupted = tokio::spawn(async move {
            interrupted_service
                .save_profile_with_secret(sqlite_profile(id, "Target"), Some("new-secret"))
                .await
        })
        .await;
        assert!(interrupted.unwrap_err().is_panic());
        drop(service);

        let pending = repository.journal();
        let staging_id = pending.as_ref().and_then(ProfileJournal::staging_id);
        if let Some(bytes) = repository.journal_bytes() {
            let json = String::from_utf8(bytes).unwrap();
            assert!(!json.contains("old-secret"));
            assert!(!json.contains("new-secret"));
        }

        repository.disarm();
        let recovered =
            DataService::persistent_with_repository(repository.clone(), secrets.clone());
        assert!(recovered.profile_load_warnings().is_empty());
        let expect_target = phase != PersistencePhase::JournalPrepared;
        assert_eq!(
            recovered.profile(id).await.unwrap().name,
            if expect_target { "Target" } else { "Original" }
        );
        assert_eq!(
            secrets.get(id).unwrap().unwrap().expose_secret(),
            if expect_target {
                "new-secret"
            } else {
                "old-secret"
            }
        );
        if let Some(staging_id) = staging_id {
            assert!(secrets.get(staging_id).unwrap().is_none());
        }
        assert!(repository.journal().is_none());

        drop(recovered);
        let recovered_again = DataService::persistent_with_repository(repository, secrets);
        assert_eq!(
            recovered_again.profile(id).await.unwrap().name,
            if expect_target { "Target" } else { "Original" }
        );
        assert!(recovered_again.profile_load_warnings().is_empty());
    }

    #[tokio::test]
    async fn secret_save_recovers_consistently_after_every_durable_phase() {
        for phase in [
            PersistencePhase::JournalPrepared,
            PersistencePhase::SecretStaged,
            PersistencePhase::ProfilesReplaced,
            PersistencePhase::SecretPromoted,
            PersistencePhase::JournalApplied,
            PersistencePhase::StagingDeleted,
            PersistencePhase::JournalRemoved,
        ] {
            crash_save_and_recover(phase).await;
        }
    }

    async fn crash_save_rollback_and_recover(phase: PersistencePhase) {
        let repository = Arc::new(MemoryFaultProfileRepository::default());
        let secrets = Arc::new(MemorySecretStore::default());
        let id = Uuid::new_v4();
        let service = Arc::new(DataService::persistent_with_repository(
            repository.clone(),
            secrets.clone(),
        ));
        service
            .save_profile_with_secret(sqlite_profile(id, "Original"), Some("old-secret"))
            .await
            .unwrap();
        let original_profiles = service.profiles.read().await.clone();
        let mut target_map = original_profiles.clone();
        target_map.insert(id, sqlite_profile(id, "Target"));
        let target_profiles = profiles_from_map(&target_map);
        let staging_id = Uuid::new_v4();
        let mut journal = ProfileJournal::save(id, Some(staging_id), target_profiles.clone());
        journal.phase = JournalPhase::ProfilesReplaced;
        replace_target_profiles(repository.as_ref(), &target_profiles).unwrap();
        replace_journal(repository.as_ref(), &journal).unwrap();
        secrets.set(staging_id, "new-secret").unwrap();
        secrets.set(id, "new-secret").unwrap();

        repository.crash_after(phase);
        let interrupted_service = Arc::clone(&service);
        let rollback_repository = Arc::clone(&repository);
        let interrupted = tokio::spawn(async move {
            let previous = SecretString::from("old-secret".to_owned());
            interrupted_service.abort_prepared_save(
                rollback_repository.as_ref(),
                &journal,
                &original_profiles,
                Some(&previous),
                true,
            )
        })
        .await;
        assert!(interrupted.unwrap_err().is_panic());
        drop(service);

        repository.disarm();
        let recovered =
            DataService::persistent_with_repository(repository.clone(), secrets.clone());
        let rolled_forward = matches!(
            phase,
            PersistencePhase::ProfilesRestored | PersistencePhase::SecretRestored
        );
        assert_eq!(
            recovered.profile(id).await.unwrap().name,
            if rolled_forward { "Target" } else { "Original" }
        );
        assert_eq!(
            secrets.get(id).unwrap().unwrap().expose_secret(),
            if rolled_forward {
                "new-secret"
            } else {
                "old-secret"
            }
        );
        assert!(secrets.get(staging_id).unwrap().is_none());
        assert!(repository.journal().is_none());
    }

    #[tokio::test]
    async fn save_rollback_recovers_consistently_after_every_cleanup_phase() {
        for phase in [
            PersistencePhase::ProfilesRestored,
            PersistencePhase::SecretRestored,
            PersistencePhase::JournalRolledBack,
            PersistencePhase::StagingDeleted,
            PersistencePhase::JournalRemoved,
        ] {
            crash_save_rollback_and_recover(phase).await;
        }
    }

    async fn crash_delete_and_recover(phase: PersistencePhase) {
        let repository = Arc::new(MemoryFaultProfileRepository::default());
        let secrets = Arc::new(MemorySecretStore::default());
        let id = Uuid::new_v4();
        let service = Arc::new(DataService::persistent_with_repository(
            repository.clone(),
            secrets.clone(),
        ));
        service
            .save_profile_with_secret(sqlite_profile(id, "Delete target"), Some("real-secret"))
            .await
            .unwrap();

        repository.crash_after(phase);
        let interrupted_service = Arc::clone(&service);
        let interrupted =
            tokio::spawn(async move { interrupted_service.remove_profile(id).await }).await;
        assert!(interrupted.unwrap_err().is_panic());
        drop(service);

        if let Some(bytes) = repository.journal_bytes() {
            assert!(!String::from_utf8(bytes).unwrap().contains("real-secret"));
        }
        repository.disarm();
        let recovered =
            DataService::persistent_with_repository(repository.clone(), secrets.clone());
        assert!(recovered.profile(id).await.is_err());
        assert!(secrets.get(id).unwrap().is_none());
        assert!(repository.journal().is_none());

        drop(recovered);
        let recovered_again = DataService::persistent_with_repository(repository, secrets);
        assert!(recovered_again.profile(id).await.is_err());
        assert!(recovered_again.profile_load_warnings().is_empty());
    }

    #[tokio::test]
    async fn delete_recovers_forward_after_every_durable_phase() {
        for phase in [
            PersistencePhase::JournalPrepared,
            PersistencePhase::ProfilesReplaced,
            PersistencePhase::SecretDeleted,
            PersistencePhase::JournalRemoved,
        ] {
            crash_delete_and_recover(phase).await;
        }
    }

    #[tokio::test]
    async fn invalid_journal_is_bounded_to_a_sanitized_warning_and_blocks_mutation() {
        let repository = Arc::new(MemoryFaultProfileRepository::default());
        repository
            .replace_journal(br#"{"version":99,"secret":"must-not-appear"}"#)
            .unwrap();
        let service = DataService::persistent_with_repository(
            repository,
            Arc::new(MemorySecretStore::default()),
        );

        let warnings = service.profile_load_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(!warnings[0].contains("must-not-appear"));
        assert!(!warnings[0].contains("version"));
        let profiles = service.list_profiles().await;
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, DEMO_PROFILE_ID);
        assert!(matches!(
            service
                .save_profile(sqlite_profile(Uuid::new_v4(), "Blocked"))
                .await,
            Err(DataError::PersistenceConsistency)
        ));
    }

    #[tokio::test]
    async fn nil_profile_identifier_is_rejected_before_persistence() {
        let repository = Arc::new(MemoryFaultProfileRepository::default());
        let service = DataService::persistent_with_repository(
            repository.clone(),
            Arc::new(MemorySecretStore::default()),
        );

        assert!(matches!(
            service
                .save_profile(sqlite_profile(Uuid::nil(), "Nil"))
                .await,
            Err(DataError::InvalidProfile(_))
        ));
        assert!(repository.journal().is_none());
        assert!(repository.read_profiles().unwrap().is_none());
    }

    #[tokio::test]
    async fn missing_post_stage_secret_fails_closed_without_publishing_profiles() {
        let repository = Arc::new(MemoryFaultProfileRepository::default());
        let secrets = Arc::new(MemorySecretStore::default());
        let id = Uuid::new_v4();
        let initial = DataService::persistent_with_repository(repository.clone(), secrets.clone());
        initial
            .save_profile_with_secret(sqlite_profile(id, "Original"), Some("old-secret"))
            .await
            .unwrap();
        drop(initial);

        let target_profiles = vec![sqlite_profile(id, "Unrecoverable target")];
        let mut journal = ProfileJournal::save(id, Some(Uuid::new_v4()), target_profiles.clone());
        journal.phase = JournalPhase::Staged;
        replace_target_profiles(repository.as_ref(), &target_profiles).unwrap();
        replace_journal(repository.as_ref(), &journal).unwrap();

        let recovered =
            DataService::persistent_with_repository(repository.clone(), secrets.clone());
        let warnings = recovered.profile_load_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(!warnings[0].contains("old-secret"));
        assert!(recovered.profile(id).await.is_err());
        assert!(matches!(
            recovered.connect(id).await,
            Err(DataError::ProfileNotFound)
        ));
        assert_eq!(recovered.list_profiles().await.len(), 1);
        assert_eq!(
            secrets.get(id).unwrap().unwrap().expose_secret(),
            "old-secret"
        );
        assert!(repository.journal().is_some());
    }

    #[tokio::test]
    async fn atomic_save_rolls_back_secret_and_profile_when_secret_write_fails() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        let id = Uuid::new_v4();
        let secrets = Arc::new(FaultInjectingSecretStore::default());
        secrets.fail_next_set_after_write();
        let service = DataService::persistent_with_secret_store(&path, secrets.clone());

        let error = service
            .save_profile_with_secret(sqlite_profile(id, "  Atomic  "), Some("must-not-leak"))
            .await
            .unwrap_err();

        assert!(matches!(error, DataError::KeyringUnavailable));
        assert!(service.profile(id).await.is_err());
        assert!(secrets.get(id).unwrap().is_none());
        assert!(!path.exists());
        assert!(!error.to_string().contains("must-not-leak"));
    }

    #[tokio::test]
    async fn atomic_update_preserves_previous_profile_and_secret_on_secret_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        let id = Uuid::new_v4();
        let secrets = Arc::new(FaultInjectingSecretStore::default());
        let service = DataService::persistent_with_secret_store(&path, secrets.clone());
        let saved = service
            .save_profile_with_secret(sqlite_profile(id, "Original"), Some("old-secret"))
            .await
            .unwrap();
        assert_eq!(saved.name, "Original");

        secrets.fail_next_set_after_write();
        let error = service
            .save_profile_with_secret(sqlite_profile(id, "  Replacement  "), Some("new-secret"))
            .await
            .unwrap_err();

        assert!(matches!(error, DataError::KeyringUnavailable));
        assert_eq!(service.profile(id).await.unwrap().name, "Original");
        assert_eq!(
            secrets.get(id).unwrap().unwrap().expose_secret(),
            "old-secret"
        );
        drop(service);
        let reloaded = DataService::persistent_with_secret_store(&path, secrets);
        assert_eq!(reloaded.profile(id).await.unwrap().name, "Original");
    }

    #[tokio::test]
    async fn atomic_save_compensates_secret_when_profile_persistence_fails() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        fs::create_dir(&path).unwrap();
        let id = Uuid::new_v4();
        let secrets = Arc::new(MemorySecretStore::default());
        let service = DataService::persistent_with_secret_store(&path, secrets.clone());

        let error = service
            .save_profile_with_secret(sqlite_profile(id, "Not persisted"), Some("temporary"))
            .await
            .unwrap_err();

        assert!(matches!(error, DataError::ProfileStorage(_)));
        assert!(service.profile(id).await.is_err());
        assert!(secrets.get(id).unwrap().is_none());
        assert!(!error.to_string().contains(path.to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn remove_restores_secret_when_profile_persistence_fails() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        let id = Uuid::new_v4();
        let secrets = Arc::new(MemorySecretStore::default());
        let service = DataService::persistent_with_secret_store(&path, secrets.clone());
        service
            .save_profile_with_secret(sqlite_profile(id, "Keep me"), Some("still-present"))
            .await
            .unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();

        let error = service.remove_profile(id).await.unwrap_err();

        assert!(matches!(error, DataError::ProfileStorage(_)));
        assert_eq!(service.profile(id).await.unwrap().name, "Keep me");
        assert_eq!(
            secrets.get(id).unwrap().unwrap().expose_secret(),
            "still-present"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn compatibility_secret_write_cannot_race_successful_removal() {
        let id = Uuid::new_v4();
        let (secret_store, delete_started, allow_delete) = CoordinatedDeleteSecretStore::new();
        let secrets = Arc::new(secret_store);
        let service = Arc::new(DataService::with_secret_store(secrets.clone()));
        service
            .save_profile_with_secret(sqlite_profile(id, "Remove me"), Some("original"))
            .await
            .unwrap();

        let remove_service = Arc::clone(&service);
        let remove = tokio::spawn(async move { remove_service.remove_profile(id).await });
        tokio::task::spawn_blocking(move || delete_started.wait())
            .await
            .unwrap();

        let save_service = Arc::clone(&service);
        let save = tokio::spawn(async move {
            save_service
                .save_connection_secret(id, "must-not-be-orphaned")
                .await
        });
        tokio::task::yield_now().await;
        tokio::task::spawn_blocking(move || allow_delete.wait())
            .await
            .unwrap();

        remove.await.unwrap().unwrap();
        assert!(matches!(
            save.await.unwrap(),
            Err(DataError::ProfileNotFound)
        ));
        assert!(secrets.get(id).unwrap().is_none());
    }

    #[tokio::test]
    async fn passwordless_sqlite_connect_does_not_read_the_secret_store() {
        let id = Uuid::new_v4();
        let service = DataService::with_secret_store(Arc::new(UnavailableReadSecretStore));
        service
            .save_profile(sqlite_profile(id, "Passwordless"))
            .await
            .unwrap();

        let status = service.connect(id).await.unwrap();

        assert_eq!(status.state, ConnectionState::Connected);
    }

    fn relational_server_profile(kind: DatabaseKind, tls: bool) -> ConnectionProfile {
        assert!(matches!(
            kind,
            DatabaseKind::PostgreSql | DatabaseKind::MySql
        ));
        ConnectionProfile {
            id: Uuid::new_v4(),
            name: "Relational server".to_owned(),
            kind,
            config: ConnectionConfig {
                host: Some("database.example.com".to_owned()),
                port: kind.default_port(),
                database: Some("analytics".to_owned()),
                username: Some("reader".to_owned()),
                tls,
                ..ConnectionConfig::default()
            },
            read_only: false,
            built_in: false,
            capabilities: kind.capabilities(),
        }
    }

    fn mongo_profile(srv: bool) -> ConnectionProfile {
        ConnectionProfile {
            id: Uuid::new_v4(),
            name: "Mongo".to_owned(),
            kind: DatabaseKind::MongoDb,
            config: ConnectionConfig {
                host: Some(if srv {
                    "cluster.example.mongodb.net".to_owned()
                } else {
                    "localhost".to_owned()
                }),
                port: Some(27_018),
                database: Some("analytics".to_owned()),
                username: Some("db user".to_owned()),
                srv,
                tls: true,
                ..ConnectionConfig::default()
            },
            read_only: false,
            built_in: false,
            capabilities: DatabaseKind::MongoDb.capabilities(),
        }
    }

    #[tokio::test]
    async fn profile_storage_lock_fails_closed_until_owner_drops() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        let id = Uuid::new_v4();
        let first_secrets = Arc::new(MemorySecretStore::default());
        let first = DataService::persistent_with_secret_store(&path, first_secrets.clone());
        first
            .save_profile_with_secret(sqlite_profile(id, "First owner"), Some("first-secret"))
            .await
            .unwrap();

        let second_secrets = Arc::new(MemorySecretStore::default());
        let second = DataService::persistent_with_secret_store(&path, second_secrets.clone());
        assert_eq!(
            second.profile_load_warning(),
            Some(PROFILE_STORAGE_OWNED_WARNING)
        );
        let second_profiles = second.list_profiles().await;
        assert_eq!(second_profiles.len(), 1);
        assert_eq!(second_profiles[0].id, DEMO_PROFILE_ID);
        assert!(matches!(
            second
                .save_profile_with_secret(
                    sqlite_profile(Uuid::new_v4(), "Blocked"),
                    Some("must-not-write"),
                )
                .await,
            Err(DataError::PersistenceConsistency)
        ));
        assert!(matches!(
            second.save_connection_secret(id, "must-not-write").await,
            Err(DataError::PersistenceConsistency)
        ));
        assert!(matches!(
            second.save_ai_secret("must-not-write"),
            Err(DataError::PersistenceConsistency)
        ));
        assert!(matches!(
            second.test_connection(&demo_profile(), None).await,
            Err(DataError::PersistenceConsistency)
        ));
        assert!(matches!(
            second.connect(DEMO_PROFILE_ID).await,
            Err(DataError::PersistenceConsistency)
        ));
        assert!(second_secrets.get(id).unwrap().is_none());
        assert!(second_secrets.get(AI_SECRET_ID).unwrap().is_none());

        first
            .save_profile(sqlite_profile(id, "Consistent after release"))
            .await
            .unwrap();
        drop(first);

        let successor = DataService::persistent_with_secret_store(&path, first_secrets.clone());
        assert!(successor.profile_load_warning().is_none());
        assert_eq!(
            successor.profile(id).await.unwrap().name,
            "Consistent after release"
        );
        assert_eq!(
            first_secrets.get(id).unwrap().unwrap().expose_secret(),
            "first-secret"
        );
        assert!(
            !fs::read_to_string(&path)
                .unwrap()
                .contains("must-not-write")
        );
    }

    #[tokio::test]
    async fn profiles_persist_across_restarts_and_removal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        let id = Uuid::new_v4();
        let secrets = Arc::new(MemorySecretStore::default());
        let service = DataService::persistent_with_secret_store(&path, secrets.clone());
        service
            .save_profile(sqlite_profile(id, "Persistent"))
            .await
            .unwrap();
        drop(service);

        let reloaded = DataService::persistent_with_secret_store(&path, secrets.clone());
        assert!(reloaded.profile_load_warning().is_none());
        assert_eq!(reloaded.list_profiles().await.len(), 2);
        assert_eq!(reloaded.profile(id).await.unwrap().name, "Persistent");
        reloaded.remove_profile(id).await.unwrap();
        drop(reloaded);

        let after_removal = DataService::persistent_with_secret_store(&path, secrets);
        let profiles = after_removal.list_profiles().await;
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, DEMO_PROFILE_ID);
    }

    #[tokio::test]
    async fn corrupt_profile_file_preserves_demo_and_records_safe_warning() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        fs::write(&path, b"{ definitely not profile JSON").unwrap();

        let service = DataService::persistent_with_secret_store(
            &path,
            Arc::new(MemorySecretStore::default()),
        );
        let warning = service.profile_load_warning().unwrap();
        let warnings = service.profile_load_warnings();
        assert_eq!(warnings, [warning]);
        assert!(!warning.contains(path.to_string_lossy().as_ref()));
        assert!(!warning.contains("definitely"));
        assert!(!warnings[0].contains(path.to_string_lossy().as_ref()));
        assert!(!warnings[0].contains("definitely"));
        let profiles = service.list_profiles().await;
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, DEMO_PROFILE_ID);
    }

    #[tokio::test]
    async fn persisted_profiles_never_include_connection_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        let id = Uuid::new_v4();
        let service = DataService::persistent_with_secret_store(
            &path,
            Arc::new(MemorySecretStore::default()),
        );
        service
            .save_profile(sqlite_profile(id, "Secret boundary"))
            .await
            .unwrap();
        service
            .save_connection_secret(id, "credential-that-must-not-leak")
            .await
            .unwrap();

        for (alias, value) in [
            (
                "authMechanismProperties",
                "AWS_SESSION_TOKEN:session-secret",
            ),
            ("pwd", "alias-secret"),
            ("apiKey", "api-secret"),
            ("credential", "credential-secret"),
        ] {
            let mut unsafe_profile = mongo_profile(true);
            unsafe_profile
                .config
                .options
                .insert(alias.to_owned(), value.to_owned());
            assert!(matches!(
                service.save_profile(unsafe_profile).await,
                Err(DataError::InvalidProfile(_))
            ));
        }

        let json = fs::read_to_string(&path).unwrap();
        assert!(!json.contains("credential-that-must-not-leak"));
        assert!(!json.contains("session-secret"));
        assert!(!json.contains("alias-secret"));
        assert!(!json.contains("api-secret"));
        assert!(!json.contains("credential-secret"));
        assert!(!json.to_ascii_lowercase().contains("password"));
        assert!(!path.with_extension("json.journal").exists());
    }

    #[test]
    fn relational_tls_urls_verify_server_identity_and_false_adds_no_mode() {
        for (kind, key, expected) in [
            (DatabaseKind::PostgreSql, "sslmode", "verify-full"),
            (DatabaseKind::MySql, "ssl-mode", "VERIFY_IDENTITY"),
        ] {
            let profile = relational_server_profile(kind, true);
            let url = server_url(&profile, None).unwrap();
            assert_eq!(
                url.query_pairs()
                    .find(|(candidate, _)| candidate == key)
                    .map(|(_, value)| value.into_owned()),
                Some(expected.to_owned())
            );

            let disabled = relational_server_profile(kind, false);
            let url = server_url(&disabled, None).unwrap();
            assert!(url.query_pairs().all(|(candidate, _)| candidate != key));
        }
    }

    #[test]
    fn mongodb_standard_and_srv_urls_are_built_safely() {
        let standard = server_url(&mongo_profile(false), Some("p@ss/word")).unwrap();
        assert_eq!(standard.scheme(), "mongodb");
        assert_eq!(standard.host_str(), Some("localhost"));
        assert_eq!(standard.port(), Some(27_018));
        assert_eq!(
            standard
                .query_pairs()
                .find(|(key, _)| key == "tls")
                .unwrap()
                .1,
            "true"
        );
        assert!(standard.as_str().contains("p%40ss%2Fword"));

        let mut without_tls = mongo_profile(false);
        without_tls.config.tls = false;
        let without_tls = server_url(&without_tls, None).unwrap();
        assert!(without_tls.query_pairs().all(|(key, _)| key != "tls"));

        let srv = server_url(&mongo_profile(true), None).unwrap();
        assert_eq!(srv.scheme(), "mongodb+srv");
        assert_eq!(srv.host_str(), Some("cluster.example.mongodb.net"));
        assert_eq!(srv.port(), None);
        assert_eq!(
            srv.query_pairs().find(|(key, _)| key == "tls").unwrap().1,
            "true"
        );
        assert!(!srv.as_str().contains("27018"));
    }

    #[test]
    fn mongodb_srv_rejects_invalid_hosts_and_embedded_credentials() {
        for host in [
            "localhost",
            "127.0.0.1",
            "mongodb+srv://cluster.example.test",
            "user:secret@cluster.example.test",
            "cluster.example.test/path",
        ] {
            let mut profile = mongo_profile(true);
            profile.config.host = Some(host.to_owned());
            assert!(
                profile.validate().is_err(),
                "host should be rejected: {host}"
            );
            assert!(server_url(&profile, None).is_err());
        }
    }

    fn invalid_mongo_query(query: &str, request_limit: usize) -> String {
        match parse_mongo_read_query(query, request_limit) {
            Err(DataError::InvalidQuery(message)) => message,
            other => panic!("expected invalid MongoDB query, got {other:?}"),
        }
    }

    #[test]
    fn mongo_read_parser_accepts_legacy_and_explicit_find() {
        let legacy = parse_mongo_read_query(
            r#"{"collection":"events","filter":{"active":true},"projection":{"name":1},"sort":{"name":1}}"#,
            25,
        )
        .unwrap();
        let MongoReadQuery::Find(legacy) = legacy else {
            panic!("legacy query should default to find");
        };
        assert_eq!(legacy.collection, "events");
        assert_eq!(legacy.filter, doc! { "active": true });
        assert_eq!(legacy.projection, Some(doc! { "name": 1 }));
        assert_eq!(legacy.sort, Some(doc! { "name": 1 }));
        assert_eq!(legacy.limit, 25);
        assert_eq!(legacy.explain, None);

        let explicit = parse_mongo_read_query(
            r#"{"operation":"find","collection":"events","limit":4,"explain":"execution_stats"}"#,
            10,
        )
        .unwrap();
        let MongoReadQuery::Find(explicit) = explicit else {
            panic!("explicit find should parse as find");
        };
        assert!(explicit.filter.is_empty());
        assert_eq!(explicit.limit, 4);
        assert_eq!(
            explicit.explain,
            Some(MongoExplainVerbosity::ExecutionStats)
        );
    }

    #[test]
    fn mongo_read_parser_enforces_limits_and_query_size() {
        assert!(
            invalid_mongo_query(r#"{"collection":"events","limit":0}"#, 10)
                .contains("positive integer")
        );
        assert!(
            invalid_mongo_query(r#"{"collection":"events","limit":11}"#, 10)
                .contains("request limit")
        );

        let oversized = format!(
            r#"{{"collection":"events","filter":{{"value":"{}"}}}}"#,
            "x".repeat(MAX_MONGO_QUERY_BYTES)
        );
        assert!(invalid_mongo_query(&oversized, 10).contains("exceeds"));
    }

    #[test]
    fn mongo_read_parser_rejects_unknown_incompatible_and_invalid_fields() {
        assert!(
            invalid_mongo_query(r#"{"collection":"events","unknown":true}"#, 10)
                .contains("field 'unknown'")
        );
        assert!(
            invalid_mongo_query(
                r#"{"operation":"find","collection":"events","pipeline":[]}"#,
                10,
            )
            .contains("field 'pipeline'")
        );
        assert!(
            invalid_mongo_query(
                r#"{"operation":"aggregate","collection":"events","pipeline":[],"filter":{}}"#,
                10,
            )
            .contains("field 'filter'")
        );
        assert!(invalid_mongo_query(r#"{"collection":"   "}"#, 10).contains("blank"));
        assert!(
            invalid_mongo_query(r#"{"collection":"events","filter":[]}"#, 10).contains("filter")
        );
        assert!(
            invalid_mongo_query(r#"{"collection":"events","projection":1}"#, 10)
                .contains("projection")
        );
        assert!(
            invalid_mongo_query(r#"{"collection":"events","sort":"name"}"#, 10).contains("sort")
        );
    }

    #[test]
    fn mongo_read_parser_accepts_safe_aggregate_pipeline() {
        let parsed = parse_mongo_read_query(
            r#"{"operation":"aggregate","collection":"events","pipeline":[{"$match":{"active":true}},{"$group":{"_id":"$kind","count":{"$sum":1}}}],"limit":7}"#,
            20,
        )
        .unwrap();
        let MongoReadQuery::Aggregate(aggregate) = parsed else {
            panic!("aggregate query should parse as aggregate");
        };
        assert_eq!(aggregate.collection, "events");
        assert_eq!(aggregate.pipeline.len(), 2);
        assert_eq!(aggregate.pipeline[0], doc! { "$match": { "active": true } });
        assert_eq!(aggregate.limit, 7);
    }

    #[test]
    fn mongo_read_parser_rejects_aggregate_write_stages() {
        for stage in [
            r#"{"$out":"archive"}"#,
            r#"{"$merge":"archive"}"#,
            r#"{"$unionWith":{"coll":"archive","pipeline":[{"$merge":"events"}]}}"#,
        ] {
            let query = format!(
                r#"{{"operation":"aggregate","collection":"events","pipeline":[{stage}]}}"#
            );
            assert!(matches!(
                parse_mongo_read_query(&query, 10),
                Err(DataError::ReadOnlyViolation(_))
            ));
        }
    }

    #[test]
    fn mongo_read_parser_rejects_malformed_and_overlong_pipeline_stages() {
        for pipeline in [
            r#"["$match"]"#,
            r"[{}]",
            r#"[{"match":{}}]"#,
            r#"[{"$match":{},"$sort":{}}]"#,
        ] {
            let query = format!(
                r#"{{"operation":"aggregate","collection":"events","pipeline":{pipeline}}}"#
            );
            assert!(invalid_mongo_query(&query, 10).contains("stage"));
        }

        let too_many = (0..=MAX_MONGO_PIPELINE_STAGES)
            .map(|_| serde_json::json!({"$match": {}}))
            .collect::<Vec<_>>();
        let query = serde_json::json!({
            "operation": "aggregate",
            "collection": "events",
            "pipeline": too_many,
        })
        .to_string();
        assert!(invalid_mongo_query(&query, 10).contains("stages"));

        let query = serde_json::json!({
            "operation": "aggregate",
            "collection": "events",
            "pipeline": [{"$match": {"value": "x".repeat(MAX_MONGO_STAGE_BYTES)}}],
        })
        .to_string();
        assert!(invalid_mongo_query(&query, 10).contains("stage exceeds"));
    }

    #[test]
    fn mongo_read_parser_accepts_only_supported_explain_values() {
        for (value, expected) in [
            ("query_planner", MongoExplainVerbosity::QueryPlanner),
            ("execution_stats", MongoExplainVerbosity::ExecutionStats),
            (
                "all_plans_execution",
                MongoExplainVerbosity::AllPlansExecution,
            ),
        ] {
            let query =
                format!(r#"{{"operation":"find","collection":"events","explain":"{value}"}}"#);
            let MongoReadQuery::Find(find) = parse_mongo_read_query(&query, 10).unwrap() else {
                panic!("query should parse as find");
            };
            assert_eq!(find.explain, Some(expected));
        }
        assert!(
            invalid_mongo_query(r#"{"collection":"events","explain":"verbose"}"#, 10,)
                .contains("query_planner")
        );
    }

    #[test]
    fn mongo_document_shaping_preserves_bson_and_pagination() {
        use mongodb::bson::{Binary, oid::ObjectId, spec::BinarySubtype};

        let object_id = ObjectId::new();
        let binary = Binary {
            subtype: BinarySubtype::Generic,
            bytes: vec![0, 1, 255],
        };
        let uuid_binary = Binary {
            subtype: BinarySubtype::Uuid,
            bytes: (0_u8..16).collect(),
        };
        let page = shape_mongo_documents(
            vec![
                doc! {
                    "_id": object_id,
                    "count": i64::MAX,
                    "nested": { "value": 42 },
                    "payload": binary.clone(),
                    "uuid_payload": uuid_binary.clone(),
                },
                doc! { "name": "second" },
                doc! { "name": "not returned" },
            ],
            2,
            7,
            123,
        );

        assert_eq!(
            page.columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            vec!["_id", "count", "name", "nested", "payload", "uuid_payload"]
        );
        assert_eq!(
            page.rows[0][0],
            DataValue::Bson(serde_json::json!({"$oid": object_id.to_hex()}))
        );
        assert_eq!(page.rows[0][1], DataValue::Integer(i64::MAX.to_string()));
        assert_eq!(page.rows[0][2], DataValue::Null);
        assert_eq!(
            page.rows[0][3],
            DataValue::from_bson(Bson::Document(doc! { "value": 42 }))
        );
        assert_eq!(page.rows[0][4], DataValue::from_bson(Bson::Binary(binary)));
        assert_eq!(
            page.rows[0][5],
            DataValue::Bson(Bson::Binary(uuid_binary.clone()).into_canonical_extjson())
        );
        assert_eq!(
            page.rows[0][5].to_bson().unwrap(),
            Bson::Binary(uuid_binary)
        );
        assert_eq!(page.rows[1][0], DataValue::Null);
        assert_eq!(page.rows[1][2], DataValue::Text("second".to_owned()));
        assert_eq!(page.stats.elapsed_ms, 123);
        assert_eq!(page.stats.rows_returned, 2);
        assert!(page.stats.truncated);
        assert_eq!(page.next_offset, Some(9));
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "the live gate keeps setup, cleanup, and the full Mongo execution matrix together"
    )]
    async fn live_mongo_read_execution_when_configured() {
        use mongodb::bson::{Binary, DateTime as BsonDateTime, oid::ObjectId, spec::BinarySubtype};

        let Ok(url) = std::env::var("REDROB_TEST_MONGO_URL") else {
            return;
        };
        let database = std::env::var("REDROB_TEST_MONGO_DATABASE")
            .unwrap_or_else(|_| "redrob_test".to_owned());
        let client = MongoClient::with_uri_str(&url).await.unwrap();
        let collection_name = format!("redrob_live_{}", Uuid::new_v4().simple());
        let collection = client
            .database(&database)
            .collection::<Document>(&collection_name);
        let object_id = ObjectId::new();
        let binary = Binary {
            subtype: BinarySubtype::Generic,
            bytes: vec![0, 1, 255],
        };
        let uuid_binary = Binary {
            subtype: BinarySubtype::Uuid,
            bytes: (0_u8..16).collect(),
        };
        let date_time = BsonDateTime::from_millis(1_738_554_906_700);

        let operation: Result<_> = async {
            collection
                .insert_many([
                    doc! {
                        "_id": object_id,
                        "active": true,
                        "binary": binary.clone(),
                        "boolean": true,
                        "date_time": date_time,
                        "double": 1.5,
                        "int32": 42_i32,
                        "int64": i64::MAX,
                        "nested": { "value": 7_i32 },
                        "rank": 1_i32,
                        "text": "first",
                        "title": "alpha",
                        "uuid_binary": uuid_binary.clone(),
                    },
                    doc! { "active": true, "rank": 2_i32, "title": "beta" },
                    doc! { "active": true, "rank": 3_i32, "title": "gamma" },
                    doc! { "active": true, "rank": 4_i32, "title": "delta" },
                ])
                .await
                .map_err(|error| {
                    DataError::database("MongoDB live fixture insert failed", &error)
                })?;

            let find_page = execute_mongo_query(
                &client,
                &database,
                &mongo_query(
                    &serde_json::json!({
                        "operation": "find",
                        "collection": &collection_name,
                        "filter": {"active": true},
                        "projection": {"_id": 0, "rank": 1, "title": 1},
                        "sort": {"rank": 1},
                    }),
                    2,
                    1,
                ),
            )
            .await?;
            let aggregate_page = execute_mongo_query(
                &client,
                &database,
                &mongo_query(
                    &serde_json::json!({
                        "operation": "aggregate",
                        "collection": &collection_name,
                        "pipeline": [
                            {"$match": {"active": true}},
                            {"$sort": {"rank": 1}},
                            {"$project": {"_id": 0, "rank": 1, "title": 1}}
                        ],
                    }),
                    1,
                    2,
                ),
            )
            .await?;
            let scalar_page = execute_mongo_query(
                &client,
                &database,
                &mongo_query(
                    &serde_json::json!({
                        "operation": "find",
                        "collection": &collection_name,
                        "filter": {"rank": 1},
                    }),
                    1,
                    0,
                ),
            )
            .await?;

            let mut explain_pages = Vec::new();
            for verbosity in ["query_planner", "execution_stats", "all_plans_execution"] {
                explain_pages.push(
                    execute_mongo_query(
                        &client,
                        &database,
                        &mongo_query(
                            &serde_json::json!({
                                "operation": "find",
                                "collection": &collection_name,
                                "filter": {"active": true},
                                "explain": verbosity,
                            }),
                            10,
                            0,
                        ),
                    )
                    .await?,
                );
            }
            let aggregate_explain = execute_mongo_query(
                &client,
                &database,
                &mongo_query(
                    &serde_json::json!({
                        "operation": "aggregate",
                        "collection": &collection_name,
                        "pipeline": [{"$match": {"active": true}}],
                        "explain": "execution_stats",
                    }),
                    10,
                    0,
                ),
            )
            .await?;

            Ok((
                find_page,
                aggregate_page,
                scalar_page,
                explain_pages,
                aggregate_explain,
            ))
        }
        .await;
        let cleanup = collection.drop().await;
        let (find_page, aggregate_page, scalar_page, explain_pages, aggregate_explain) = match (
            operation, cleanup,
        ) {
            (Ok(pages), Ok(())) => pages,
            (Err(error), Ok(())) => panic!("MongoDB live execution failed: {error}"),
            (Ok(_), Err(error)) => panic!("MongoDB live cleanup failed: {error}"),
            (Err(operation_error), Err(cleanup_error)) => panic!(
                "MongoDB live execution failed: {operation_error}; cleanup also failed: {cleanup_error}"
            ),
        };

        assert_eq!(find_page.stats.rows_returned, 2);
        assert!(find_page.stats.truncated);
        assert_eq!(find_page.next_offset, Some(3));
        assert_eq!(
            find_page
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            vec!["rank", "title"]
        );
        assert_eq!(
            find_page.rows,
            vec![
                vec![
                    DataValue::Integer("2".to_owned()),
                    DataValue::Text("beta".to_owned()),
                ],
                vec![
                    DataValue::Integer("3".to_owned()),
                    DataValue::Text("gamma".to_owned()),
                ],
            ]
        );

        assert_eq!(aggregate_page.stats.rows_returned, 1);
        assert!(aggregate_page.stats.truncated);
        assert_eq!(aggregate_page.next_offset, Some(3));
        assert_eq!(
            aggregate_page.rows[0],
            vec![
                DataValue::Integer("3".to_owned()),
                DataValue::Text("gamma".to_owned()),
            ]
        );

        assert_eq!(scalar_page.stats.rows_returned, 1);
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "_id"),
            &DataValue::from_bson(Bson::ObjectId(object_id))
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "binary"),
            &DataValue::from_bson(Bson::Binary(binary))
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "boolean"),
            &DataValue::Boolean(true)
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "date_time"),
            &DataValue::from_bson(Bson::DateTime(date_time))
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "double"),
            &DataValue::Float(1.5)
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "int32"),
            &DataValue::Integer("42".to_owned())
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "int64"),
            &DataValue::Integer(i64::MAX.to_string())
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "nested"),
            &DataValue::from_bson(Bson::Document(doc! { "value": 7_i32 }))
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "text"),
            &DataValue::Text("first".to_owned())
        );
        assert_eq!(
            mongo_result_value(&scalar_page, 0, "uuid_binary"),
            &DataValue::Bson(Bson::Binary(uuid_binary).into_canonical_extjson())
        );

        assert_eq!(explain_pages.len(), 3);
        assert!(explain_pages.iter().all(|page| {
            page.stats.rows_returned == 1 && !page.columns.is_empty() && !page.rows[0].is_empty()
        }));
        assert_eq!(aggregate_explain.stats.rows_returned, 1);
        assert!(!aggregate_explain.columns.is_empty());
        assert!(!aggregate_explain.rows[0].is_empty());
    }

    #[tokio::test]
    async fn query_language_mismatch_fails_before_mongodb_connection() {
        let service = service();
        let profile = mongo_profile(false);
        let id = profile.id;
        service.save_profile(profile).await.unwrap();
        let request = QueryRequest {
            connection_id: id,
            query: "SELECT 1".to_owned(),
            language: QueryLanguage::Sql,
            parameters: Vec::new(),
            limit: 10,
            offset: 0,
            timeout_ms: 100,
        };
        assert!(matches!(
            service.execute_query(request).await,
            Err(DataError::InvalidQuery(message)) if message == "SQL cannot run on MongoDB"
        ));
        assert!(!service.active.read().await.contains_key(&id));
    }

    #[test]
    fn ai_request_debug_does_not_include_content() {
        let request = AiChatRequest {
            model: "m".to_owned(),
            messages: vec![AiMessage {
                role: AiRole::User,
                content: "sensitive schema".to_owned(),
            }],
            temperature: None,
            max_tokens: None,
        };
        assert!(!format!("{request:?}").contains("sensitive schema"));
    }
}
