//! Durable, secret-free profile transaction journal and atomic JSON storage.

use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ConnectionProfile, DEMO_PROFILE_ID, DataError, Result,
    ai::AI_SECRET_ID,
    secret::{SecretStore, exposed},
};

const JOURNAL_VERSION: u8 = 1;
const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;
const MAX_PROFILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_JOURNAL_PROFILES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PersistencePhase {
    JournalPrepared,
    SecretStaged,
    ProfilesReplaced,
    SecretPromoted,
    JournalApplied,
    ProfilesRestored,
    SecretRestored,
    JournalRolledBack,
    StagingDeleted,
    JournalRemoved,
    SecretDeleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum JournalPhase {
    Prepared,
    Staged,
    ProfilesReplaced,
    Applied,
    RolledBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub(crate) enum JournalOperation {
    Save {
        profile_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        staging_id: Option<Uuid>,
    },
    Delete {
        profile_id: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProfileJournal {
    version: u8,
    operation_id: Uuid,
    pub(crate) phase: JournalPhase,
    pub(crate) operation: JournalOperation,
    pub(crate) target_profiles: Vec<ConnectionProfile>,
}

impl ProfileJournal {
    pub(crate) fn save(
        profile_id: Uuid,
        staging_id: Option<Uuid>,
        target_profiles: Vec<ConnectionProfile>,
    ) -> Self {
        Self {
            version: JOURNAL_VERSION,
            operation_id: Uuid::new_v4(),
            phase: JournalPhase::Prepared,
            operation: JournalOperation::Save {
                profile_id,
                staging_id,
            },
            target_profiles,
        }
    }

    pub(crate) fn delete(profile_id: Uuid, target_profiles: Vec<ConnectionProfile>) -> Self {
        Self {
            version: JOURNAL_VERSION,
            operation_id: Uuid::new_v4(),
            phase: JournalPhase::Prepared,
            operation: JournalOperation::Delete { profile_id },
            target_profiles,
        }
    }

    pub(crate) fn staging_id(&self) -> Option<Uuid> {
        match self.operation {
            JournalOperation::Save { staging_id, .. } => staging_id,
            JournalOperation::Delete { .. } => None,
        }
    }

    pub(crate) fn operation_id(&self) -> Uuid {
        self.operation_id
    }
}

pub(crate) trait ProfileRepository: Send + Sync {
    fn read_profiles(&self) -> Result<Option<Vec<u8>>>;
    fn replace_profiles(&self, bytes: &[u8]) -> Result<()>;
    fn read_journal(&self) -> Result<Option<Vec<u8>>>;
    fn replace_journal(&self, bytes: &[u8]) -> Result<()>;
    fn remove_journal(&self) -> Result<()>;

    fn checkpoint(&self, _phase: PersistencePhase) {}
}

pub(crate) struct FileProfileRepository {
    profile_path: PathBuf,
    journal_path: PathBuf,
    // Retaining this descriptor retains the OS lock for the repository lifetime.
    // The file is intentionally empty and contains no profile or credential data.
    _lock_file: File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepositoryOpenError {
    InUse,
    Unavailable,
}

impl FileProfileRepository {
    pub(crate) fn new(profile_path: PathBuf) -> std::result::Result<Self, RepositoryOpenError> {
        let storage_directory = parent(&profile_path);
        fs::create_dir_all(storage_directory).map_err(|_| RepositoryOpenError::Unavailable)?;

        let mut lock_name = OsString::from(profile_path.as_os_str());
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|_| RepositoryOpenError::Unavailable)?;
        fs2::FileExt::try_lock_exclusive(&lock_file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                RepositoryOpenError::InUse
            } else {
                RepositoryOpenError::Unavailable
            }
        })?;
        lock_file
            .set_len(0)
            .and_then(|()| lock_file.sync_all())
            .map_err(|_| RepositoryOpenError::Unavailable)?;
        sync_parent(&lock_path, "profile storage lock")
            .map_err(|_| RepositoryOpenError::Unavailable)?;

        let mut journal_name = OsString::from(profile_path.as_os_str());
        journal_name.push(".journal");
        Ok(Self {
            profile_path,
            journal_path: PathBuf::from(journal_name),
            _lock_file: lock_file,
        })
    }
}

impl ProfileRepository for FileProfileRepository {
    fn read_profiles(&self) -> Result<Option<Vec<u8>>> {
        read_bounded(&self.profile_path, MAX_PROFILE_BYTES, "connection profiles")
    }

    fn replace_profiles(&self, bytes: &[u8]) -> Result<()> {
        atomic_replace(&self.profile_path, bytes, "connection profiles")
    }

    fn read_journal(&self) -> Result<Option<Vec<u8>>> {
        read_bounded(
            &self.journal_path,
            MAX_JOURNAL_BYTES,
            "profile transaction journal",
        )
    }

    fn replace_journal(&self, bytes: &[u8]) -> Result<()> {
        atomic_replace(&self.journal_path, bytes, "profile transaction journal")
    }

    fn remove_journal(&self) -> Result<()> {
        match fs::remove_file(&self.journal_path) {
            Ok(()) => sync_parent(&self.journal_path, "profile transaction journal"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(storage_error(
                "could not remove the profile transaction journal",
            )),
        }
    }
}

pub(crate) fn profiles_from_map(
    profiles: &HashMap<Uuid, ConnectionProfile>,
) -> Vec<ConnectionProfile> {
    let mut persisted: Vec<_> = profiles
        .values()
        .filter(|profile| !profile.built_in && profile.id != DEMO_PROFILE_ID)
        .cloned()
        .collect();
    persisted.sort_by_key(|profile| profile.id);
    persisted
}

pub(crate) fn serialize_profiles(profiles: &[ConnectionProfile]) -> Result<Vec<u8>> {
    serialize_json(profiles, MAX_PROFILE_BYTES, "connection profiles")
}

pub(crate) fn serialize_journal(journal: &ProfileJournal) -> Result<Vec<u8>> {
    let bytes = serialize_json(journal, MAX_JOURNAL_BYTES, "profile transaction journal")?;
    // Defense in depth: the journal model is intentionally composed only of non-secret profiles.
    validate_journal(journal.clone())?;
    Ok(bytes)
}

pub(crate) fn deserialize_journal(bytes: &[u8]) -> Result<ProfileJournal> {
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(DataError::PersistenceConsistency);
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| DataError::PersistenceConsistency)?;
    let target_profiles = value
        .get("targetProfiles")
        .and_then(serde_json::Value::as_array)
        .ok_or(DataError::PersistenceConsistency)?;
    if target_profiles.iter().any(|profile| {
        profile
            .get("id")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .is_none_or(|id| id.is_nil())
    }) {
        return Err(DataError::PersistenceConsistency);
    }
    let journal: ProfileJournal =
        serde_json::from_value(value).map_err(|_| DataError::PersistenceConsistency)?;
    validate_journal(journal)
}

pub(crate) fn reconcile(
    repository: &dyn ProfileRepository,
    secrets: &dyn SecretStore,
) -> Result<()> {
    let Some(bytes) = repository.read_journal()? else {
        return Ok(());
    };
    let mut journal = deserialize_journal(&bytes)?;
    let operation_id = journal.operation_id();

    match journal.operation {
        JournalOperation::Save {
            profile_id,
            staging_id,
        } => reconcile_save(repository, secrets, &mut journal, profile_id, staging_id),
        JournalOperation::Delete { profile_id } => {
            replace_target_profiles(repository, &journal.target_profiles)?;
            repository.checkpoint(PersistencePhase::ProfilesReplaced);
            secrets.delete(profile_id)?;
            repository.checkpoint(PersistencePhase::SecretDeleted);
            remove_journal(repository, operation_id)?;
            repository.checkpoint(PersistencePhase::JournalRemoved);
            Ok(())
        }
    }
}

fn reconcile_save(
    repository: &dyn ProfileRepository,
    secrets: &dyn SecretStore,
    journal: &mut ProfileJournal,
    profile_id: Uuid,
    staging_id: Option<Uuid>,
) -> Result<()> {
    if journal.phase == JournalPhase::RolledBack {
        if let Some(staging_id) = staging_id {
            secrets.delete(staging_id)?;
            repository.checkpoint(PersistencePhase::StagingDeleted);
        }
        remove_journal(repository, journal.operation_id())?;
        repository.checkpoint(PersistencePhase::JournalRemoved);
        return Ok(());
    }

    if journal.phase == JournalPhase::Applied {
        // Replacing the target again is idempotent and closes a durability gap if the
        // profile rename reached disk but its directory metadata was not persisted.
        replace_target_profiles(repository, &journal.target_profiles)?;
        repository.checkpoint(PersistencePhase::ProfilesReplaced);
        if let Some(staging_id) = staging_id
            && let Some(secret) = secrets.get(staging_id)?
        {
            secrets.set(profile_id, exposed(&secret))?;
            repository.checkpoint(PersistencePhase::SecretPromoted);
        }
    } else {
        let staged = match staging_id {
            Some(id) => secrets.get(id)?,
            None => None,
        };
        if staging_id.is_some() && staged.is_none() {
            if journal.phase == JournalPhase::Prepared {
                remove_journal(repository, journal.operation_id())?;
                repository.checkpoint(PersistencePhase::JournalRemoved);
                return Ok(());
            }
            return Err(DataError::PersistenceConsistency);
        }

        replace_target_profiles(repository, &journal.target_profiles)?;
        repository.checkpoint(PersistencePhase::ProfilesReplaced);
        journal.phase = JournalPhase::ProfilesReplaced;
        replace_journal(repository, journal)?;
        if let Some(secret) = staged.as_ref() {
            secrets.set(profile_id, exposed(secret))?;
            repository.checkpoint(PersistencePhase::SecretPromoted);
        }
        journal.phase = JournalPhase::Applied;
        replace_journal(repository, journal)?;
        repository.checkpoint(PersistencePhase::JournalApplied);
    }

    if let Some(staging_id) = staging_id {
        secrets.delete(staging_id)?;
        repository.checkpoint(PersistencePhase::StagingDeleted);
    }
    remove_journal(repository, journal.operation_id())?;
    repository.checkpoint(PersistencePhase::JournalRemoved);
    Ok(())
}

pub(crate) fn replace_target_profiles(
    repository: &dyn ProfileRepository,
    profiles: &[ConnectionProfile],
) -> Result<()> {
    repository.replace_profiles(&serialize_profiles(profiles)?)
}

pub(crate) fn replace_journal(
    repository: &dyn ProfileRepository,
    journal: &ProfileJournal,
) -> Result<()> {
    if let Some(bytes) = repository.read_journal()? {
        let stored = deserialize_journal(&bytes)?;
        if stored.operation_id != journal.operation_id {
            return Err(DataError::PersistenceConsistency);
        }
    }
    repository.replace_journal(&serialize_journal(journal)?)
}

pub(crate) fn remove_journal(repository: &dyn ProfileRepository, operation_id: Uuid) -> Result<()> {
    if let Some(bytes) = repository.read_journal()? {
        let stored = deserialize_journal(&bytes)?;
        if stored.operation_id != operation_id {
            return Err(DataError::PersistenceConsistency);
        }
        repository.remove_journal()?;
    }
    Ok(())
}

fn validate_journal(mut journal: ProfileJournal) -> Result<ProfileJournal> {
    if journal.version != JOURNAL_VERSION
        || journal.operation_id.is_nil()
        || journal.target_profiles.len() > MAX_JOURNAL_PROFILES
    {
        return Err(DataError::PersistenceConsistency);
    }

    let mut ids = HashSet::with_capacity(journal.target_profiles.len());
    for profile in &mut journal.target_profiles {
        profile.normalize();
        if profile.id.is_nil()
            || profile.id == DEMO_PROFILE_ID
            || profile.id == AI_SECRET_ID
            || profile.built_in
            || !ids.insert(profile.id)
            || profile.validate().is_err()
        {
            return Err(DataError::PersistenceConsistency);
        }
    }
    journal.target_profiles.sort_by_key(|profile| profile.id);

    match journal.operation {
        JournalOperation::Save {
            profile_id,
            staging_id,
        } => {
            if !ids.contains(&profile_id)
                || staging_id.is_some_and(|id| {
                    id.is_nil()
                        || id == profile_id
                        || id == DEMO_PROFILE_ID
                        || id == AI_SECRET_ID
                        || ids.contains(&id)
                })
            {
                return Err(DataError::PersistenceConsistency);
            }
        }
        JournalOperation::Delete { profile_id } => {
            if profile_id == DEMO_PROFILE_ID
                || profile_id == AI_SECRET_ID
                || profile_id.is_nil()
                || ids.contains(&profile_id)
            {
                return Err(DataError::PersistenceConsistency);
            }
        }
    }
    Ok(journal)
}

fn serialize_json<T: Serialize + ?Sized>(
    value: &T,
    maximum: u64,
    label: &'static str,
) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| storage_error(&format!("could not serialize {label}")))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > maximum {
        return Err(storage_error(&format!("{label} is too large")));
    }
    Ok(bytes)
}

fn read_bounded(path: &Path, maximum: u64, label: &'static str) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(storage_error(&format!("could not read {label}"))),
    };
    if file
        .metadata()
        .map_or(true, |metadata| metadata.len() > maximum)
    {
        return Err(storage_error(&format!("{label} is too large")));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| storage_error(&format!("could not read {label}")))?;
    if bytes.len() as u64 > maximum {
        return Err(storage_error(&format!("{label} is too large")));
    }
    Ok(Some(bytes))
}

fn atomic_replace(path: &Path, bytes: &[u8], label: &'static str) -> Result<()> {
    let parent = parent(path);
    fs::create_dir_all(parent)
        .map_err(|_| storage_error("could not create the profile storage directory"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| storage_error(&format!("could not create a temporary {label} file")))?;
    temporary
        .write_all(bytes)
        .map_err(|_| storage_error(&format!("could not write {label}")))?;
    temporary
        .flush()
        .map_err(|_| storage_error(&format!("could not flush {label}")))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| storage_error(&format!("could not sync {label}")))?;
    temporary
        .persist(path)
        .map_err(|_| storage_error(&format!("could not replace {label}")))?;
    sync_parent(path, label)
}

fn sync_parent(path: &Path, label: &'static str) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(parent(path))
            .and_then(|directory| directory.sync_all())
            .map_err(|_| storage_error(&format!("could not sync {label} storage")))?;
    }
    #[cfg(not(unix))]
    let _ = (path, label);
    Ok(())
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn storage_error(message: &str) -> DataError {
    DataError::ProfileStorage(message.to_owned())
}

pub(crate) fn repository(
    path: PathBuf,
) -> std::result::Result<Arc<dyn ProfileRepository>, RepositoryOpenError> {
    Ok(Arc::new(FileProfileRepository::new(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConnectionConfig, DatabaseKind};

    fn test_profile() -> ConnectionProfile {
        ConnectionProfile {
            id: Uuid::new_v4(),
            name: "Strict journal".to_owned(),
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

    #[test]
    fn journal_deserialization_is_bounded_and_strict() {
        let oversized = vec![
            b' ';
            usize::try_from(MAX_JOURNAL_BYTES + 1)
                .expect("journal byte limit fits usize")
        ];
        assert!(matches!(
            deserialize_journal(&oversized),
            Err(DataError::PersistenceConsistency)
        ));
        assert!(matches!(
            deserialize_journal(
                br#"{"version":1,"operationId":"00000000-0000-0000-0000-000000000001","phase":"prepared","operation":{"kind":"delete","profileId":"00000000-0000-0000-0000-000000000002"},"targetProfiles":[],"unexpected":true}"#,
            ),
            Err(DataError::PersistenceConsistency)
        ));

        let profile = test_profile();
        let journal = ProfileJournal::save(profile.id, None, vec![profile]);
        for path in ["profile", "capabilities"] {
            let mut value = serde_json::to_value(&journal).unwrap();
            let target = value["targetProfiles"][0]
                .as_object_mut()
                .expect("profile is an object");
            if path == "profile" {
                target.insert(
                    "plaintextSecret".to_owned(),
                    serde_json::Value::String("must-be-rejected".to_owned()),
                );
            } else {
                target["capabilities"]
                    .as_object_mut()
                    .expect("capabilities is an object")
                    .insert(
                        "plaintextSecret".to_owned(),
                        serde_json::Value::String("must-be-rejected".to_owned()),
                    );
            }
            assert!(matches!(
                deserialize_journal(&serde_json::to_vec(&value).unwrap()),
                Err(DataError::PersistenceConsistency)
            ));
        }

        for invalid_id in [None, Some(Uuid::nil().to_string())] {
            let mut value = serde_json::to_value(&journal).unwrap();
            let target = value["targetProfiles"][0]
                .as_object_mut()
                .expect("profile is an object");
            if let Some(id) = invalid_id {
                target.insert("id".to_owned(), serde_json::Value::String(id));
            } else {
                target.remove("id");
            }
            assert!(matches!(
                deserialize_journal(&serde_json::to_vec(&value).unwrap()),
                Err(DataError::PersistenceConsistency)
            ));
        }
    }

    #[test]
    fn exclusive_lock_blocks_independent_repository_and_releases_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("connections.json");
        let lock_path = PathBuf::from(format!("{}.lock", path.display()));

        let first = repository(path.clone()).unwrap();
        assert!(matches!(
            repository(path.clone()),
            Err(RepositoryOpenError::InUse)
        ));
        assert_eq!(fs::read(&lock_path).unwrap(), Vec::<u8>::new());
        assert!(!path.exists());
        assert!(!PathBuf::from(format!("{}.journal", path.display())).exists());

        drop(first);
        assert!(repository(path).is_ok());
    }

    #[test]
    fn exclusive_lock_releases_when_owner_panics_and_drops() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.json");
        let crashing_path = path.clone();

        let crashed = std::thread::spawn(move || {
            let _repository = repository(crashing_path).unwrap();
            panic!("simulated process-local crash");
        })
        .join();

        assert!(crashed.is_err());
        assert!(repository(path).is_ok());
    }

    #[test]
    fn journal_replacement_and_removal_require_operation_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path().join("connections.json")).unwrap();
        let profile = test_profile();
        let owned = ProfileJournal::save(profile.id, None, vec![profile.clone()]);
        let foreign = ProfileJournal::save(profile.id, None, vec![profile]);

        replace_journal(repository.as_ref(), &owned).unwrap();
        assert!(matches!(
            replace_journal(repository.as_ref(), &foreign),
            Err(DataError::PersistenceConsistency)
        ));
        assert!(matches!(
            remove_journal(repository.as_ref(), foreign.operation_id()),
            Err(DataError::PersistenceConsistency)
        ));
        assert!(repository.read_journal().unwrap().is_some());

        remove_journal(repository.as_ref(), owned.operation_id()).unwrap();
        assert!(repository.read_journal().unwrap().is_none());
    }
}
