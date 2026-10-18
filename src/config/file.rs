use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::error::Error;
use crate::fsutil;
use crate::i18n::{self, fill, Lang};

use super::EnvMap;

pub const DEFAULT_CONFIG_TOML: &str = r#"# cmdgen configuration. Do not put an API key in this file.
# Switch between deepseek and bailian with `cmdgen config use <name>`.
language = "en"
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
    pub language: Option<String>,
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
        .ok_or_else(|| Error::Usage(i18n::t().err_no_home.into()))
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
            fill(i18n::t().err_secret_in_config, &[("key", key)]),
        ));
    }
    let file: FileConfig =
        toml::from_str(text).map_err(|err| Error::invalid(path, err.to_string()))?;
    apply_parsed_language(&file, path)?;
    validate_config(&file, path)?;
    Ok(file)
}

/// Read `language` after the file parses. Invalid codes are reported in English.
/// A missing key selects `en`. An explicit `--lang` or `CMDGEN_LANG` is left in place.
pub fn apply_parsed_language(file: &FileConfig, path: &Path) -> Result<Lang, Error> {
    let Some(raw) = trimmed(&file.language) else {
        i18n::apply_config_language(Lang::En);
        return Ok(Lang::En);
    };
    match i18n::parse_lang_code(raw) {
        Some(lang) => {
            i18n::apply_config_language(lang);
            Ok(lang)
        }
        None => Err(Error::Usage(format!(
            "invalid config ({}): {}",
            path.display(),
            i18n::unknown_language_message(raw)
        ))),
    }
}

pub fn validate_config(file: &FileConfig, path: &Path) -> Result<(), Error> {
    let legacy = provider_file_has_content(&file.provider);
    let mut names = BTreeMap::<String, String>::new();
    for (raw_name, profile) in &file.providers {
        let name = normalize_in_file(raw_name, path)?;
        if names.insert(name.clone(), raw_name.clone()).is_some() {
            return Err(Error::invalid(
                path,
                fill(i18n::t().err_provider_duplicate, &[("name", &name)]),
            ));
        }
        if let Some(var) = trimmed(&profile.api_key_env) {
            if !is_env_var_name(var) {
                return Err(Error::invalid(
                    path,
                    fill(i18n::t().err_api_key_env, &[("name", &name)]),
                ));
            }
        }
        if !super::is_preset(&name)
            && (trimmed(&profile.base_url).is_none() || trimmed(&profile.model).is_none())
        {
            return Err(Error::invalid(
                path,
                fill(i18n::t().err_profile_needs_endpoint, &[("name", &name)]),
            ));
        }
    }

    if !names.is_empty() && legacy {
        return Err(Error::invalid(path, i18n::t().err_legacy_and_providers));
    }

    if let Some(active) = trimmed(&file.active) {
        let active = normalize_in_file(active, path)?;
        let known = names.contains_key(&active) || super::is_preset(&active);
        if !names.is_empty() && !known {
            return Err(Error::invalid(
                path,
                fill(i18n::t().err_active_unknown, &[("active", &active)]),
            ));
        }
        if names.is_empty() && legacy {
            return Err(Error::invalid(path, i18n::t().err_active_needs_providers));
        }
        if names.is_empty() && !legacy && !super::is_preset(&active) {
            return Err(Error::invalid(
                path,
                fill(i18n::t().err_active_unknown_preset, &[("active", &active)]),
            ));
        }
    } else if !names.is_empty() {
        return Err(Error::invalid(path, i18n::t().err_providers_need_active));
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
        return Err(Error::invalid(path, i18n::t().err_legacy_only_switch));
    }
    let name = normalize_in_file(name, path)?;
    let known = file
        .providers
        .keys()
        .any(|key| super::normalize_provider(key).ok().as_deref() == Some(name.as_str()));
    if !known && !super::is_preset(&name) {
        return Err(Error::invalid(
            path,
            fill(i18n::t().err_unknown_provider, &[("name", &name)]),
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
        return Err(Error::Usage(i18n::t().err_set_needs_field.into()));
    }
    if !path.exists() {
        return Err(Error::ConfigNotFound {
            path: path.display().to_string(),
        });
    }
    let text = fsutil::read_to_string(path)?;
    let file = parse_config(&text, path)?;
    if provider_file_has_content(&file.provider) && file.providers.is_empty() {
        return Err(Error::invalid(path, i18n::t().err_legacy_only_edit));
    }

    let name = match name {
        Some(name) => normalize_in_file(name, path)?,
        None => match trimmed(&file.active) {
            Some(active) => normalize_in_file(active, path)?,
            None => {
                return Err(Error::Usage(i18n::t().err_no_active_for_set.into()));
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
            fill(i18n::t().err_new_provider_needs_both, &[("name", &name)]),
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
        return Err(Error::invalid(path, i18n::t().err_model_empty));
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
        return Err(fill(i18n::t().err_profile_must_be_table, &[("key", key)]));
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
        _ => Err(i18n::t().err_providers_must_be_table.into()),
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
        Some(_) => Err(fill(i18n::t().err_key_must_be_string, &[("key", key)])),
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
        Some(_) => Err(i18n::t().err_active_must_be_string.into()),
        None => {
            doc.insert("active", toml_edit::value(name));
            Ok(())
        }
    }
}

pub fn render_default_config(language: Lang) -> String {
    DEFAULT_CONFIG_TOML.replacen(
        "language = \"en\"",
        &format!("language = \"{}\"", language.as_str()),
        1,
    )
}

pub fn init_config(path: &Path, language: Lang) -> Result<InitOutcome, Error> {
    if path.exists() {
        return Ok(InitOutcome::AlreadyExists(path.to_path_buf()));
    }
    if let Some(parent) = path.parent() {
        fsutil::ensure_dir(parent, 0o700)?;
    }
    fsutil::atomic_write(path, &render_default_config(language), 0o600)?;
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
            formatter.write_str(i18n::t().expect_number)
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
            formatter.write_str(i18n::t().expect_integer)
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
