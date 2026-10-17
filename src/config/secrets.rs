use std::collections::HashMap;

use crate::error::{missing_api_key, Error};

pub const SERVICE_NAME: &str = "cmdgen";

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

    pub fn from_process() -> Self {
        const KEYS: &[&str] = &[
            "HOME",
            "XDG_CONFIG_HOME",
            "SHELL",
            "CMDGEN_API_KEY",
            "DEEPSEEK_API_KEY",
            "OPENAI_API_KEY",
            "CMDGEN_BASE_URL",
            "CMDGEN_MODEL",
            "CMDGEN_PROVIDER",
            "CMDGEN_SHELL",
            "CMDGEN_TIMEOUT",
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
            return Err(Error::Usage("API Key 不能为空。".into()));
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
            reason: "没有找到对应凭据".into(),
        },
        keyring::Error::NoStorageAccess(inner) => Error::CredentialStoreUnavailable {
            reason: format!("无法访问系统凭据存储: {inner}"),
        },
        keyring::Error::PlatformFailure(inner) => Error::CredentialStoreUnavailable {
            reason: format!("系统凭据存储失败: {inner}"),
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
                reason: "内存凭据存储锁已损坏".into(),
            })?;
        Ok(guard.get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), Error> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(Error::Usage("API Key 不能为空。".into()));
        }
        let mut guard = self
            .values
            .lock()
            .map_err(|_| Error::CredentialStoreUnavailable {
                reason: "内存凭据存储锁已损坏".into(),
            })?;
        guard.insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool, Error> {
        let mut guard = self
            .values
            .lock()
            .map_err(|_| Error::CredentialStoreUnavailable {
                reason: "内存凭据存储锁已损坏".into(),
            })?;
        Ok(guard.remove(account).is_some())
    }
}

pub fn provider_env_var(provider: &str) -> &'static str {
    if provider.eq_ignore_ascii_case("deepseek") {
        "DEEPSEEK_API_KEY"
    } else {
        "OPENAI_API_KEY"
    }
}

/// 凭据优先级：`CMDGEN_API_KEY` > provider 环境变量 > 系统凭据存储。
pub fn resolve_api_key(
    provider: &str,
    env: &EnvMap,
    store: &dyn SecretStore,
) -> Result<ResolvedKey, Error> {
    if let Some(secret) = env.get("CMDGEN_API_KEY") {
        return Ok(ResolvedKey {
            secret: secret.trim().to_string(),
            source_label: "环境变量 CMDGEN_API_KEY".into(),
        });
    }

    let provider_var = provider_env_var(provider);
    if let Some(secret) = env.get(provider_var) {
        return Ok(ResolvedKey {
            secret: secret.trim().to_string(),
            source_label: format!("环境变量 {provider_var}"),
        });
    }

    match store.get(provider) {
        Ok(Some(secret)) => Ok(ResolvedKey {
            secret,
            source_label: format!("系统凭据存储（服务 {SERVICE_NAME}，账号 {provider}）"),
        }),
        Ok(None) => Err(missing_api_key(provider, None)),
        Err(Error::CredentialStoreUnavailable { reason }) => {
            Err(missing_api_key(provider, Some(reason)))
        }
        Err(err) => Err(err),
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
