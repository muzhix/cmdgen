use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::error::Error;
use crate::fsutil;

use super::EnvMap;

pub const DEFAULT_CONFIG_TOML: &str = r#"# cmdgen 配置。不要在此文件中写入 API Key。
# 用 `cmdgen config use <name>` 在 deepseek 和 bailian 之间切换。
active = "deepseek"

[providers.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-flash"

[providers.bailian]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen3.8-flash"
api_key_env = "DASHSCOPE_API_KEY"

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
    pub active: Option<String>,
    pub provider: ProviderFile,
    pub providers: BTreeMap<String, ProviderProfile>,
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
pub struct ProviderProfile {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
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

/// `update_provider` 实际写入配置文件的字段。未给出的字段为 `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderUpdate {
    pub name: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
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
    let file: FileConfig =
        toml::from_str(text).map_err(|err| Error::invalid(path, err.to_string()))?;
    validate_config(&file, path)?;
    Ok(file)
}

pub fn validate_config(file: &FileConfig, path: &Path) -> Result<(), Error> {
    let legacy = provider_file_has_content(&file.provider);
    let mut names = BTreeMap::<String, String>::new();
    for (raw_name, profile) in &file.providers {
        let name = normalize_in_file(raw_name, path)?;
        if names.insert(name.clone(), raw_name.clone()).is_some() {
            return Err(Error::invalid(path, format!("provider 名称重复: {name}")));
        }
        if let Some(var) = trimmed(&profile.api_key_env) {
            if !is_env_var_name(var) {
                return Err(Error::invalid(
                    path,
                    format!(
                        "providers.{name}.api_key_env 必须是环境变量名，例如 DASHSCOPE_API_KEY。"
                    ),
                ));
            }
        }
        if !super::is_preset(&name)
            && (trimmed(&profile.base_url).is_none() || trimmed(&profile.model).is_none())
        {
            return Err(Error::invalid(
                path,
                format!(
                    "[providers.{name}] 必须设置 base_url 和 model。预设 deepseek / bailian 可以省略这两项。"
                ),
            ));
        }
    }

    if !names.is_empty() && legacy {
        return Err(Error::invalid(
            path,
            "不能同时使用 [provider] 和 [providers]。请只保留 [providers.<name>]，并用 active 选择当前项。",
        ));
    }

    if let Some(active) = trimmed(&file.active) {
        let active = normalize_in_file(active, path)?;
        let known = names.contains_key(&active) || super::is_preset(&active);
        if !names.is_empty() && !known {
            return Err(Error::invalid(
                path,
                format!(
                    "active 指向未知 provider: {active}。请改成 [providers] 中的名称，或预设 deepseek / bailian。"
                ),
            ));
        }
        if names.is_empty() && legacy {
            return Err(Error::invalid(
                path,
                "active 需要配合 [providers.<name>] 使用。当前文件仍是单个 [provider]。",
            ));
        }
        if names.is_empty() && !legacy && !super::is_preset(&active) {
            return Err(Error::invalid(
                path,
                format!(
                    "active 指向未知 provider: {active}。请改成预设 deepseek / bailian，或先添加 [providers.{active}]。"
                ),
            ));
        }
    } else if !names.is_empty() {
        return Err(Error::invalid(
            path,
            "配置了 [providers] 但没有 active。请设置 active，或运行 `cmdgen config use <name>`。",
        ));
    }
    Ok(())
}

/// 把 `active` 写成 `name`。只改这一项，保留注释和其余配置。
pub fn activate_provider(path: &Path, name: &str) -> Result<String, Error> {
    if !path.exists() {
        return Err(Error::ConfigNotFound {
            path: path.display().to_string(),
        });
    }
    let text = fsutil::read_to_string(path)?;
    let file = parse_config(&text, path)?;
    if provider_file_has_content(&file.provider) && file.providers.is_empty() {
        return Err(Error::invalid(
            path,
            "当前配置只有单个 [provider]。请改成 [providers.<name>] 后再切换。",
        ));
    }
    let name = normalize_in_file(name, path)?;
    let known = file
        .providers
        .keys()
        .any(|key| super::normalize_provider(key).ok().as_deref() == Some(name.as_str()));
    if !known && !super::is_preset(&name) {
        return Err(Error::invalid(
            path,
            format!(
                "未知的 provider: {name}。请先在配置中加入 [providers.{name}]，或改用预设 deepseek / bailian。"
            ),
        ));
    }
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|err| Error::invalid(path, err.to_string()))?;
    set_active(&mut doc, &name).map_err(|reason| Error::invalid(path, reason))?;
    let updated = doc.to_string();
    fsutil::atomic_write(path, &updated, 0o600)?;
    Ok(name)
}

/// 把 `base_url` 和 `model` 写入 `[providers.<name>]`。只改给出的字段。
///
/// `name` 为空时使用配置里的 `active`。不读取环境变量。档案不存在时，预设可以只写其中一项；其他名称必须同时给出这两项。
pub fn update_provider(
    path: &Path,
    name: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<ProviderUpdate, Error> {
    if base_url.is_none() && model.is_none() {
        return Err(Error::Usage(
            "请至少给出 --model 或 --base-url。示例: cmdgen config set --provider bailian --base-url https://example.com/v1"
                .into(),
        ));
    }
    if !path.exists() {
        return Err(Error::ConfigNotFound {
            path: path.display().to_string(),
        });
    }
    let text = fsutil::read_to_string(path)?;
    let file = parse_config(&text, path)?;
    if provider_file_has_content(&file.provider) && file.providers.is_empty() {
        return Err(Error::invalid(
            path,
            "当前配置只有单个 [provider]。请改成 [providers.<name>] 后再修改。",
        ));
    }

    let name = match name {
        Some(name) => normalize_in_file(name, path)?,
        None => match trimmed(&file.active) {
            Some(active) => normalize_in_file(active, path)?,
            None => {
                return Err(Error::Usage(
                    "配置里没有 active。请用 --provider 指定 provider，或先运行 `cmdgen config use <name>`。"
                        .into(),
                ));
            }
        },
    };
    let base_url = match base_url {
        Some(value) => Some(canonical_base_url(value)?),
        None => None,
    };
    let model = match model {
        Some(value) => Some(canonical_model(path, value)?),
        None => None,
    };

    let existing_key = file.providers.iter().find_map(|(key, _)| {
        (super::normalize_provider(key).ok().as_deref() == Some(name.as_str())).then(|| key.clone())
    });
    if existing_key.is_none() && !super::is_preset(&name) && (base_url.is_none() || model.is_none())
    {
        return Err(Error::invalid(
            path,
            format!("未知的 provider: {name}。新建档案时必须同时给出 --base-url 和 --model。"),
        ));
    }
    let key = existing_key.unwrap_or_else(|| name.clone());

    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|err| Error::invalid(path, err.to_string()))?;
    write_profile_fields(&mut doc, &key, base_url.as_deref(), model.as_deref())
        .map_err(|reason| Error::invalid(path, reason))?;
    let updated = doc.to_string();
    parse_config(&updated, path)?;
    fsutil::atomic_write(path, &updated, 0o600)?;
    Ok(ProviderUpdate {
        name,
        base_url,
        model,
    })
}

fn canonical_base_url(value: &str) -> Result<String, Error> {
    let trimmed = value.trim().trim_end_matches('/');
    crate::generator::client::completions_endpoint(trimmed)?;
    Ok(trimmed.to_string())
}

fn canonical_model(path: &Path, value: &str) -> Result<String, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::invalid(path, "model 不能为空。"));
    }
    Ok(trimmed.to_string())
}

fn write_profile_fields(
    doc: &mut DocumentMut,
    key: &str,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<(), String> {
    let providers = ensure_providers_table(doc)?;
    if !providers.contains_key(key) {
        providers.insert(key, Item::Table(Table::new()));
    }
    let Some(Item::Table(profile)) = providers.get_mut(key) else {
        return Err(format!("providers.{key} 必须是表。"));
    };
    if let Some(url) = base_url {
        assign_string(profile, "base_url", url)?;
    }
    if let Some(model) = model {
        assign_string(profile, "model", model)?;
    }
    Ok(())
}

fn ensure_providers_table(doc: &mut DocumentMut) -> Result<&mut Table, String> {
    if doc.get("providers").is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        doc.insert("providers", Item::Table(table));
    }
    match doc.get_mut("providers") {
        Some(Item::Table(table)) => Ok(table),
        _ => Err("providers 必须是表。".into()),
    }
}

fn assign_string(table: &mut Table, key: &str, value: &str) -> Result<(), String> {
    match table.get_mut(key) {
        Some(Item::Value(existing)) => {
            let decor = existing.decor().clone();
            let mut updated = Value::from(value);
            *updated.decor_mut() = decor;
            *existing = updated;
            Ok(())
        }
        Some(_) => Err(format!("{key} 必须是字符串。")),
        None => {
            table.insert(key, toml_edit::value(value));
            Ok(())
        }
    }
}

pub fn provider_file_has_content(provider: &ProviderFile) -> bool {
    trimmed(&provider.name).is_some()
        || trimmed(&provider.base_url).is_some()
        || trimmed(&provider.model).is_some()
}

pub fn trimmed(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn normalize_in_file(name: &str, path: &Path) -> Result<String, Error> {
    super::normalize_provider(name).map_err(|err| match err {
        Error::InvalidConfig { reason, .. } => Error::invalid(path, reason),
        other => other,
    })
}

fn is_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn set_active(doc: &mut DocumentMut, name: &str) -> Result<(), String> {
    match doc.get_mut("active") {
        Some(Item::Value(value)) => {
            let decor = value.decor().clone();
            let mut updated = Value::from(name);
            *updated.decor_mut() = decor;
            *value = updated;
            Ok(())
        }
        Some(_) => Err("active 必须是字符串。".into()),
        None => {
            doc.insert("active", toml_edit::value(name));
            Ok(())
        }
    }
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
