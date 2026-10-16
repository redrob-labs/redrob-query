//! Secret storage with an OS-keyring primary and process-memory fallback.

use std::{collections::HashMap, sync::Mutex};

use keyring::Entry;
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::{DataError, Result};

const SERVICE_NAME: &str = "ai.redrob.query.connections";

/// Secret storage boundary, injectable for deterministic tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: Uuid) -> Result<Option<SecretString>>;
    fn set(&self, id: Uuid, secret: &str) -> Result<()>;
    fn delete(&self, id: Uuid) -> Result<()>;
}

#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<Uuid, SecretString>>,
}

impl SecretStore for MemorySecretStore {
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
        Ok(())
    }
}

#[derive(Default)]
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    fn entry(id: Uuid) -> Result<Entry> {
        Entry::new(SERVICE_NAME, &id.to_string()).map_err(|_| DataError::KeyringUnavailable)
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, id: Uuid) -> Result<Option<SecretString>> {
        match Self::entry(id)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(DataError::KeyringUnavailable),
        }
    }

    fn set(&self, id: Uuid, secret: &str) -> Result<()> {
        Self::entry(id)?
            .set_password(secret)
            .map_err(|_| DataError::KeyringUnavailable)
    }

    fn delete(&self, id: Uuid) -> Result<()> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(DataError::KeyringUnavailable),
        }
    }
}

/// Uses the desktop keyring when available and falls back to memory when the
/// current desktop/session has no credential service.
pub struct FallbackSecretStore {
    keyring: KeyringSecretStore,
    memory: MemorySecretStore,
}

impl Default for FallbackSecretStore {
    fn default() -> Self {
        Self {
            keyring: KeyringSecretStore,
            memory: MemorySecretStore::default(),
        }
    }
}

impl SecretStore for FallbackSecretStore {
    fn get(&self, id: Uuid) -> Result<Option<SecretString>> {
        match self.keyring.get(id) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) | Err(DataError::KeyringUnavailable) => self.memory.get(id),
            Err(error) => Err(error),
        }
    }

    fn set(&self, id: Uuid, secret: &str) -> Result<()> {
        match self.keyring.set(id, secret) {
            Ok(()) => {
                let _ = self.memory.delete(id);
                Ok(())
            }
            Err(DataError::KeyringUnavailable) => self.memory.set(id, secret),
            Err(error) => Err(error),
        }
    }

    fn delete(&self, id: Uuid) -> Result<()> {
        let memory_result = self.memory.delete(id);
        match self.keyring.delete(id) {
            Ok(()) | Err(DataError::KeyringUnavailable) => memory_result,
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn exposed(secret: &SecretString) -> &str {
    secret.expose_secret()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn memory_store_round_trip_and_delete() {
        let store = MemorySecretStore::default();
        let id = Uuid::new_v4();
        store.set(id, "secret").unwrap();
        assert_eq!(store.get(id).unwrap().unwrap().expose_secret(), "secret");
        store.delete(id).unwrap();
        assert!(store.get(id).unwrap().is_none());
    }
}
