use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Error;
use crate::fsutil;

use super::EnvMap;

pub const DEFAULT_CONFIG_TOML: &str = r#"# cmdgen 配置。不要在此文件中写入 API Key。
[provider]
name = "deepseek"
base_url = "https://api.deepseek.com"
model = "deepseek-flash"

[generation]
temperature = 0.0
max_tokens = 256
timeout_secs = 20

[shell]
default = "auto"
keybinding = "ctrl-g"
include_cwd = false
"#;

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub provider: ProviderFile,
    pub generation: GenerationFile,
    pub shell: ShellFile,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderFile {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GenerationFile {
    #[serde(deserialize_with = "de_opt_f64")]
    pub temperature: Option<f64>,
    #[serde(deserialize_with = "de_opt_u32")]
    pub max_tokens: Option<u32>,
    #[serde(deserialize_with = "de_opt_u64")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShellFile {
    pub default: Option<String>,
    pub keybinding: Option<String>,
    pub include_cwd: Option<bool>,
}

pub enum InitOutcome {
    Created(PathBuf),
    AlreadyExists(PathBuf),
}

pub fn home_dir(env: &EnvMap) -> Result<PathBuf, Error> {
    if let Some(home) = env.get("HOME") {
        return Ok(PathBuf::from(home));
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| Error::Usage("无法确定用户主目录。请设置 HOME。".into()))
}

pub fn config_dir(env: &EnvMap) -> Result<PathBuf, Error> {
    if let Some(xdg) = env.get("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("cmdgen"));
    }
    Ok(home_dir(env)?.join(".config").join("cmdgen"))
}

pub fn config_path(env: &EnvMap) -> Result<PathBuf, Error> {
    Ok(config_dir(env)?.join("config.toml"))
}

pub fn load_config(path: &Path) -> Result<FileConfig, Error> {
    if !path.exists() {
        return Err(Error::ConfigNotFound {
            path: path.display().to_string(),
        });
    }
    let text = fsutil::read_to_string(path)?;
    parse_config(&text, path)
}

pub fn parse_config(text: &str, path: &Path) -> Result<FileConfig, Error> {
    if let Some(key) = forbidden_secret_key(text) {
        return Err(Error::invalid(
            path,
            format!("配置文件不能包含 {key}。API Key 请使用环境变量或 `cmdgen config set-key`。"),
        ));
    }
    toml::from_str(text).map_err(|err| Error::invalid(path, err.to_string()))
}

pub fn init_config(path: &Path) -> Result<InitOutcome, Error> {
    if path.exists() {
        return Ok(InitOutcome::AlreadyExists(path.to_path_buf()));
    }
    if let Some(parent) = path.parent() {
        fsutil::ensure_dir(parent, 0o700)?;
    }
    fsutil::atomic_write(path, DEFAULT_CONFIG_TOML, 0o600)?;
    Ok(InitOutcome::Created(path.to_path_buf()))
}

fn forbidden_secret_key(text: &str) -> Option<&'static str> {
    const KEYS: &[&str] = &["api_key", "apikey", "token", "secret", "password"];
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        let Some((key, _)) = code.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"').trim().to_ascii_lowercase();
        if let Some(found) = KEYS.iter().copied().find(|item| *item == key) {
            return Some(found);
        }
    }
    None
}

fn de_opt_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_f64(deserializer).map(Some)
}

fn de_opt_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = de_u64(deserializer)?;
    u32::try_from(value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

fn de_opt_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_u64(deserializer).map(Some)
}

fn de_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;
    impl serde::de::Visitor<'_> for Visitor {
        type Value = f64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("数字")
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<f64, E> {
            Ok(value as f64)
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<f64, E> {
            Ok(value as f64)
        }

        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<f64, E> {
            Ok(value)
        }
    }
    deserializer.deserialize_any(Visitor)
}

fn de_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;
    impl serde::de::Visitor<'_> for Visitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("整数")
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<u64, E> {
            u64::try_from(value).map_err(E::custom)
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<u64, E> {
            Ok(value)
        }
    }
    deserializer.deserialize_any(Visitor)
}
