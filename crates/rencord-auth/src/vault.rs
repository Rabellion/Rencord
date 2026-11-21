//! Account list and tokens.
//!
//! The index is JSON with no secrets. Each token lives in its own keychain
//! item. Tests use an in-memory backend with the same rules. The desktop
//! build uses the OS keychain (Secret Service, Keychain, or Credential
//! Manager) and does not fall back to a file.

use std::collections::HashMap;
use std::sync::Mutex;

use rencord_model::{Token, User};
use serde::{Deserialize, Serialize};

use crate::AuthError;

const INDEX_ACCOUNT: &str = "accounts";
const ACTIVE_ACCOUNT: &str = "active";
pub const KEYCHAIN_SERVICE: &str = "rencord";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRecord {
    pub user_id: String,
    pub username: String,
    pub global_name: Option<String>,
    pub discriminator: Option<String>,
    pub avatar: Option<String>,
}

impl AccountRecord {
    pub fn from_user(user: &User) -> Self {
        Self {
            user_id: user.id.to_string(),
            username: user.username.clone(),
            global_name: user.global_name.clone(),
            discriminator: user.discriminator.clone(),
            avatar: user.avatar.clone(),
        }
    }

    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    pub fn initial(&self) -> String {
        self.display_name()
            .chars()
            .next()
            .map(|ch| ch.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into())
    }
}

trait SecretBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, AuthError>;
    fn set(&self, account: &str, value: &str) -> Result<(), AuthError>;
    fn delete(&self, account: &str) -> Result<(), AuthError>;
}

struct MemoryBackend {
    values: Mutex<HashMap<String, String>>,
}

impl SecretBackend for MemoryBackend {
    fn get(&self, account: &str) -> Result<Option<String>, AuthError> {
        Ok(self.values.lock().expect("vault").get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), AuthError> {
        self.values
            .lock()
            .expect("vault")
            .insert(account.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), AuthError> {
        self.values.lock().expect("vault").remove(account);
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
struct KeyringBackend;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl SecretBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<String>, AuthError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(keyring_error)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(keyring_error(err)),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(keyring_error)?;
        entry.set_password(value).map_err(keyring_error)
    }

    fn delete(&self, account: &str) -> Result<(), AuthError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(keyring_error(err)),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn keyring_error(err: keyring::Error) -> AuthError {
    AuthError::Keychain(err.to_string())
}

pub struct AccountStore {
    backend: Box<dyn SecretBackend>,
}

impl AccountStore {
    pub fn memory() -> Self {
        Self {
            backend: Box::new(MemoryBackend {
                values: Mutex::new(HashMap::new()),
            }),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    pub fn keyring() -> Self {
        Self {
            backend: Box::new(KeyringBackend),
        }
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountRecord>, AuthError> {
        Ok(self.read_index()?.unwrap_or_default())
    }

    pub fn save_account(&self, account: &AccountRecord, token: &Token) -> Result<(), AuthError> {
        let session_key = session_account(&account.user_id)?;
        if token.is_empty() {
            return Err(AuthError::Message(
                "Refusing to store an empty session.".into(),
            ));
        }
        self.backend.set(&session_key, token.expose())?;
        let mut index = self.read_index()?.unwrap_or_default();
        if let Some(existing) = index
            .iter_mut()
            .find(|item| item.user_id == account.user_id)
        {
            *existing = account.clone();
        } else {
            index.push(account.clone());
        }
        self.write_index(&index)?;
        self.backend.set(ACTIVE_ACCOUNT, &account.user_id)?;
        Ok(())
    }

    pub fn load_token(&self, user_id: &str) -> Result<Option<Token>, AuthError> {
        let session_key = session_account(user_id)?;
        Ok(self
            .backend
            .get(&session_key)?
            .filter(|value| !value.is_empty())
            .map(Token::new))
    }

    pub fn remove_account(&self, user_id: &str) -> Result<(), AuthError> {
        let session_key = session_account(user_id)?;
        self.backend.delete(&session_key)?;
        let mut index = self.read_index()?.unwrap_or_default();
        index.retain(|account| account.user_id != user_id);
        self.write_index(&index)?;
        if self.active_user()?.as_deref() == Some(user_id) {
            match index.last() {
                Some(next) => self.backend.set(ACTIVE_ACCOUNT, &next.user_id)?,
                None => self.backend.delete(ACTIVE_ACCOUNT)?,
            }
        }
        Ok(())
    }

    pub fn active_user(&self) -> Result<Option<String>, AuthError> {
        Ok(self
            .backend
            .get(ACTIVE_ACCOUNT)?
            .filter(|value| !value.is_empty()))
    }

    pub fn set_active_user(&self, user_id: Option<&str>) -> Result<(), AuthError> {
        match user_id {
            Some(user_id) => {
                let _ = session_account(user_id)?;
                self.backend.set(ACTIVE_ACCOUNT, user_id)
            }
            None => self.backend.delete(ACTIVE_ACCOUNT),
        }
    }

    /// The account index as stored. Tests use this to prove the token is absent.
    pub fn stored_index(&self) -> Result<String, AuthError> {
        Ok(self.backend.get(INDEX_ACCOUNT)?.unwrap_or_default())
    }

    fn read_index(&self) -> Result<Option<Vec<AccountRecord>>, AuthError> {
        let Some(raw) = self.backend.get(INDEX_ACCOUNT)? else {
            return Ok(None);
        };
        if raw.is_empty() {
            return Ok(Some(Vec::new()));
        }
        serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| AuthError::Message("The saved account list could not be read.".into()))
    }

    fn write_index(&self, accounts: &[AccountRecord]) -> Result<(), AuthError> {
        let raw = serde_json::to_string(accounts)
            .map_err(|_| AuthError::Message("The account list could not be saved.".into()))?;
        self.backend.set(INDEX_ACCOUNT, &raw)
    }
}

fn session_account(user_id: &str) -> Result<String, AuthError> {
    if user_id.is_empty() || user_id.len() > 32 || !user_id.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(AuthError::Message("That account id is not valid.".into()));
    }
    Ok(format!("token.{user_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rencord_model::Snowflake;

    fn record(id: &str, name: &str) -> AccountRecord {
        AccountRecord {
            user_id: id.into(),
            username: name.into(),
            global_name: Some(format!("{name} display")),
            discriminator: Some("0".into()),
            avatar: None,
        }
    }

    #[test]
    fn memory_backend_switches_accounts_without_writing_the_token_into_the_index() {
        let store = AccountStore::memory();
        let first = Token::new("first.session.secret-value");
        let second = Token::new("second.session.secret-value");
        store.save_account(&record("10", "ada"), &first).unwrap();
        store
            .save_account(&record("11", "charles"), &second)
            .unwrap();
        assert_eq!(store.list_accounts().unwrap().len(), 2);
        assert_eq!(store.active_user().unwrap().as_deref(), Some("11"));
        store.set_active_user(Some("10")).unwrap();
        assert_eq!(store.active_user().unwrap().as_deref(), Some("10"));
        let loaded = store.load_token("10").unwrap().unwrap();
        assert_eq!(loaded.expose(), first.expose());
        assert!(!format!("{loaded:?}").contains("secret"));
        let index = store.stored_index().unwrap();
        assert!(!index.contains("secret"));
        assert!(!index.contains("first.session"));
        assert!(index.contains("ada"));
        store.remove_account("10").unwrap();
        assert!(store.load_token("10").unwrap().is_none());
        assert_eq!(store.active_user().unwrap().as_deref(), Some("11"));
        store.remove_account("11").unwrap();
        assert!(store.active_user().unwrap().is_none());
        assert!(
            store.stored_index().unwrap().contains("[]")
                || store.list_accounts().unwrap().is_empty()
        );
    }

    #[test]
    fn account_record_uses_the_user_and_not_a_token() {
        let user = User {
            id: Snowflake::new(7),
            username: "ada".into(),
            global_name: None,
            discriminator: None,
            avatar: Some("hash".into()),
            bot: false,
        };
        let record = AccountRecord::from_user(&user);
        assert_eq!(record.user_id, "7");
        assert_eq!(record.initial(), "A");
        assert_eq!(record.avatar.as_deref(), Some("hash"));
        assert_eq!(KEYCHAIN_SERVICE, "rencord");
    }
}
