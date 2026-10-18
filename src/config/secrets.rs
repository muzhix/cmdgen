use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{missing_api_key, Error};
use crate::fsutil;
use crate::i18n::{self, fill};

pub const SERVICE_NAME: &str = "cmdgen";

#[derive(Clone)]
pub struct EnvMap {
    values: HashMap<String, String>,
}

impl EnvMap {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.values.insert(key.into(), value.into());
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    /// 把进程环境里的某一个变量补进映射。已有同名项时不覆盖。
    pub fn include_process_var(&mut self, key: &str) {
        if self.values.contains_key(key) {
            return;
        }
        if let Ok(value) = std::env::var(key) {
            self.set(key, value);
        }
    }

    pub fn from_process() -> Self {
        const KEYS: &[&str] = &[
            "HOME",
            "XDG_CONFIG_HOME",
            "SHELL",
            "CMDGEN_API_KEY",
            "DEEPSEEK_API_KEY",
            "OPENAI_API_KEY",
            "DASHSCOPE_API_KEY",
            "CMDGEN_BASE_URL",
            "CMDGEN_MODEL",
            "CMDGEN_PROVIDER",
            "CMDGEN_SHELL",
            "CMDGEN_TIMEOUT",
            "CMDGEN_LANG",
        ];
        let mut env = Self::new();
        for key in KEYS {
            if let Ok(value) = std::env::var(key) {
                env.set(*key, value);
            }
        }
        env
    }
}

impl Default for EnvMap {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EnvMap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("EnvMap([redacted])")
    }
}

/// 已解析的 API Key。不要把 `secret` 写入日志、错误或配置文件。
pub struct ResolvedKey {
    pub secret: String,
    pub source_label: String,
}

pub trait SecretStore {
    fn get(&self, account: &str) -> Result<Option<String>, Error>;
    fn set(&self, account: &str, secret: &str) -> Result<(), Error>;
    fn delete(&self, account: &str) -> Result<bool, Error>;
}

pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, Error> {
        let entry = open_entry(account)?;
        match entry.get_password() {
            Ok(secret) => Ok(blank_to_none(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(map_keyring_err(err)),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), Error> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(Error::Usage(i18n::t().err_api_key_empty.into()));
        }
        let entry = open_entry(account)?;
        entry.set_password(secret).map_err(map_keyring_err)
    }

    fn delete(&self, account: &str) -> Result<bool, Error> {
        let entry = open_entry(account)?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(err) => Err(map_keyring_err(err)),
        }
    }
}

fn open_entry(account: &str) -> Result<keyring::Entry, Error> {
    keyring::Entry::new(SERVICE_NAME, account).map_err(map_keyring_err)
}

fn map_keyring_err(err: keyring::Error) -> Error {
    match err {
        keyring::Error::NoEntry => Error::CredentialStoreUnavailable {
            reason: i18n::t().err_no_entry.into(),
        },
        keyring::Error::NoStorageAccess(inner) => Error::CredentialStoreUnavailable {
            reason: fill(i18n::t().err_no_storage, &[("detail", &inner.to_string())]),
        },
        keyring::Error::PlatformFailure(inner) => Error::CredentialStoreUnavailable {
            reason: fill(
                i18n::t().err_platform_failure,
                &[("detail", &inner.to_string())],
            ),
        },
        other => Error::CredentialStoreUnavailable {
            reason: other.to_string(),
        },
    }
}

fn blank_to_none(secret: String) -> Option<String> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Default)]
pub struct MemoryStore {
    values: std::sync::Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, Error> {
        let guard = self
            .values
            .lock()
            .map_err(|_| Error::CredentialStoreUnavailable {
                reason: i18n::t().err_memory_lock.into(),
            })?;
        Ok(guard.get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), Error> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(Error::Usage(i18n::t().err_api_key_empty.into()));
        }
        let mut guard = self
            .values
            .lock()
            .map_err(|_| Error::CredentialStoreUnavailable {
                reason: i18n::t().err_memory_lock.into(),
            })?;
        guard.insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool, Error> {
        let mut guard = self
            .values
            .lock()
            .map_err(|_| Error::CredentialStoreUnavailable {
                reason: i18n::t().err_memory_lock.into(),
            })?;
        Ok(guard.remove(account).is_some())
    }
}

pub fn provider_env_var(provider: &str) -> &'static str {
    if provider.eq_ignore_ascii_case("deepseek") {
        "DEEPSEEK_API_KEY"
    } else if provider.eq_ignore_ascii_case("bailian") {
        "DASHSCOPE_API_KEY"
    } else {
        "OPENAI_API_KEY"
    }
}

/// 本地凭据文件。只在系统凭据存储不可用时由 `set-key` 写入。
pub struct CredentialFile {
    path: PathBuf,
}

impl CredentialFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_env(env: &EnvMap) -> Result<Self, Error> {
        Ok(Self::new(super::config_dir(env)?.join("credentials.toml")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, account: &str) -> Result<Option<String>, Error> {
        let document = self.load()?;
        Ok(document
            .keys
            .get(account)
            .and_then(|value| blank_to_none(value.clone())))
    }

    pub fn set(&self, account: &str, secret: &str) -> Result<(), Error> {
        let secret = require_secret(secret)?;
        let mut document = self.load()?;
        document.keys.insert(account.to_string(), secret);
        self.store(&document)
    }

    pub fn delete(&self, account: &str) -> Result<bool, Error> {
        if !self.path.exists() {
            return Ok(false);
        }
        let mut document = self.load()?;
        if document.keys.remove(account).is_none() {
            return Ok(false);
        }
        self.store(&document)?;
        Ok(true)
    }

    fn load(&self) -> Result<CredentialDocument, Error> {
        if !self.path.exists() {
            return Ok(CredentialDocument::default());
        }
        let text = fsutil::read_to_string(&self.path)?;
        toml::from_str(&text)
            .map_err(|_| Error::invalid(&self.path, i18n::t().err_credentials_unreadable))
    }

    fn store(&self, document: &CredentialDocument) -> Result<(), Error> {
        if document.keys.is_empty() {
            return fsutil::remove_if_exists(&self.path);
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fsutil::ensure_dir(parent, 0o700)?;
            }
        }
        let text = toml::to_string(document)
            .map_err(|_| Error::invalid(&self.path, i18n::t().err_credentials_unwritable))?;
        fsutil::atomic_write(&self.path, &text, 0o600)?;
        fsutil::set_mode(&self.path, 0o600)
    }
}

impl std::fmt::Debug for CredentialFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialFile")
            .field("path", &self.path)
            .finish()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct CredentialDocument {
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedKey {
    Keyring { removed_file_entry: bool },
    File { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletedKey {
    pub keyring: bool,
    pub file: bool,
}

/// 优先写入系统凭据存储。存储不可用时写入本地凭据文件。
/// 写入存储成功后，删除文件中的同名旧 Key。
pub fn save_api_key(
    account: &str,
    secret: &str,
    store: &dyn SecretStore,
    file: &CredentialFile,
) -> Result<SavedKey, Error> {
    let secret = require_secret(secret)?;
    match store.set(account, &secret) {
        Ok(()) => Ok(SavedKey::Keyring {
            removed_file_entry: file.delete(account)?,
        }),
        Err(Error::CredentialStoreUnavailable { .. }) => {
            file.set(account, &secret)?;
            Ok(SavedKey::File {
                path: file.path().to_path_buf(),
            })
        }
        Err(err) => Err(err),
    }
}

/// 同时删除系统凭据存储和本地凭据文件中的当前 provider。任一侧删掉即成功。
pub fn delete_api_key(
    account: &str,
    store: &dyn SecretStore,
    file: &CredentialFile,
) -> Result<DeletedKey, Error> {
    let (keyring, store_problem) = match store.delete(account) {
        Ok(removed) => (removed, None),
        Err(Error::CredentialStoreUnavailable { reason }) => (false, Some(reason)),
        Err(err) => return Err(err),
    };
    let file_removed = file.delete(account)?;
    if !keyring && !file_removed {
        let mut message = fill(i18n::t().err_nothing_to_delete, &[("account", account)]);
        if let Some(reason) = store_problem {
            message.push_str(&fill(
                i18n::t().store_unavailable_now,
                &[("reason", &reason)],
            ));
        }
        return Err(Error::Usage(message));
    }
    Ok(DeletedKey {
        keyring,
        file: file_removed,
    })
}

/// 凭据优先级：`CMDGEN_API_KEY` > `api_key_env` > 系统凭据存储 > 本地凭据文件。
/// `api_key_env` 为 `None` 时跳过 provider 环境变量，避免误用 `OPENAI_API_KEY`。
pub fn resolve_api_key(
    provider: &str,
    api_key_env: Option<&str>,
    env: &EnvMap,
    store: &dyn SecretStore,
    file: &CredentialFile,
) -> Result<ResolvedKey, Error> {
    if let Some(secret) = env.get("CMDGEN_API_KEY") {
        return Ok(ResolvedKey {
            secret: secret.trim().to_string(),
            source_label: fill(i18n::t().source_env, &[("name", "CMDGEN_API_KEY")]),
        });
    }

    if let Some(provider_var) = api_key_env.filter(|name| !name.trim().is_empty()) {
        if let Some(secret) = env.get(provider_var) {
            return Ok(ResolvedKey {
                secret: secret.trim().to_string(),
                source_label: fill(i18n::t().source_env, &[("name", provider_var)]),
            });
        }
    }

    let store_problem = match store.get(provider) {
        Ok(Some(secret)) => {
            return Ok(ResolvedKey {
                secret,
                source_label: fill(
                    i18n::t().source_keyring,
                    &[("service", SERVICE_NAME), ("account", provider)],
                ),
            });
        }
        Ok(None) => None,
        Err(Error::CredentialStoreUnavailable { reason }) => Some(reason),
        Err(err) => return Err(err),
    };

    match file.get(provider)? {
        Some(secret) => Ok(ResolvedKey {
            secret,
            source_label: fill(
                i18n::t().source_file,
                &[("path", &file.path().display().to_string())],
            ),
        }),
        None => Err(missing_api_key(provider, api_key_env, store_problem)),
    }
}

fn require_secret(secret: &str) -> Result<String, Error> {
    let secret = secret.trim();
    if secret.is_empty() {
        Err(Error::Usage(i18n::t().err_api_key_empty.into()))
    } else {
        Ok(secret.to_string())
    }
}

/// 探测凭据存储是否可查询。不会写入凭据。
pub fn keyring_status() -> Result<(), String> {
    let entry =
        keyring::Entry::new(SERVICE_NAME, "__cmdgen_probe__").map_err(|err| err.to_string())?;
    match entry.get_password() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}
