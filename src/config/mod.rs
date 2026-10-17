mod file;
pub mod secrets;

use std::path::PathBuf;

pub use file::{
    config_dir, config_path, home_dir, init_config, load_config, parse_config, FileConfig,
    InitOutcome, DEFAULT_CONFIG_TOML,
};
pub use secrets::{
    provider_env_var, resolve_api_key, EnvMap, KeyringStore, MemoryStore, ResolvedKey, SecretStore,
};

use crate::error::Error;
use crate::shell::detect::resolve_shell;
use crate::shell::integration::parse_keybinding;

/// 2026-10-17 核对 DeepSeek 文档：当前模型名为 `deepseek-flash`。
pub const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
pub const DEEPSEEK_MODEL: &str = "deepseek-flash";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const OPENAI_MODEL: &str = "gpt-4o-mini";

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub shell: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveConfig {
    pub provider_name: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
    pub timeout_secs: u64,
    pub shell_default: String,
    pub keybinding: String,
    pub include_cwd: bool,
    pub config_path: PathBuf,
    pub config_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStatus {
    pub configured: bool,
    pub source_label: String,
}

pub fn load_effective(env: &EnvMap, overrides: &Overrides) -> Result<EffectiveConfig, Error> {
    let path = config_path(env)?;
    let (file, exists) = match load_config(&path) {
        Ok(config) => (config, true),
        Err(Error::ConfigNotFound { .. }) => (FileConfig::default(), false),
        Err(err) => return Err(err),
    };
    merge(file, exists, path, env, overrides)
}

pub fn merge(
    file: FileConfig,
    exists: bool,
    path: PathBuf,
    env: &EnvMap,
    overrides: &Overrides,
) -> Result<EffectiveConfig, Error> {
    let provider_name = normalize_provider(
        &first_non_empty([
            overrides.provider.as_deref(),
            env.get("CMDGEN_PROVIDER"),
            file.provider.name.as_deref(),
            Some("deepseek"),
        ])
        .unwrap_or_else(|| "deepseek".to_string()),
    )?;

    let base_url = first_non_empty([
        overrides.base_url.as_deref(),
        env.get("CMDGEN_BASE_URL"),
        file.provider.base_url.as_deref(),
        preset_base_url(&provider_name),
    ])
    .ok_or_else(|| Error::UnsupportedProvider {
        name: provider_name.clone(),
    })?;

    let model = first_non_empty([
        overrides.model.as_deref(),
        env.get("CMDGEN_MODEL"),
        file.provider.model.as_deref(),
        preset_model(&provider_name),
    ])
    .ok_or_else(|| Error::UnsupportedProvider {
        name: provider_name.clone(),
    })?;

    let temperature = file.generation.temperature.unwrap_or(0.0);
    if !(0.0..=2.0).contains(&temperature) {
        return Err(Error::invalid(
            &path,
            "generation.temperature 必须在 0 到 2 之间",
        ));
    }

    let max_tokens = file.generation.max_tokens.unwrap_or(256);
    if !(1..=8192).contains(&max_tokens) {
        return Err(Error::invalid(
            &path,
            "generation.max_tokens 必须在 1 到 8192 之间",
        ));
    }

    let timeout_secs = if let Some(value) = overrides.timeout_secs {
        value
    } else if let Some(value) = env.get("CMDGEN_TIMEOUT") {
        parse_timeout(value)?
    } else {
        file.generation.timeout_secs.unwrap_or(20)
    };
    if !(1..=600).contains(&timeout_secs) {
        return Err(Error::invalid(
            &path,
            "generation.timeout_secs 必须在 1 到 600 之间",
        ));
    }

    let shell_default = file.shell.default.unwrap_or_else(|| "auto".to_string());
    if !shell_default.eq_ignore_ascii_case("auto") {
        resolve_shell(Some(&shell_default), "auto", None, None)?;
    }

    let keybinding = file
        .shell
        .keybinding
        .unwrap_or_else(|| "ctrl-g".to_string());
    parse_keybinding(&keybinding)?;

    Ok(EffectiveConfig {
        provider_name,
        base_url,
        model,
        temperature,
        max_tokens,
        timeout_secs,
        shell_default,
        keybinding,
        include_cwd: file.shell.include_cwd.unwrap_or(false),
        config_path: path,
        config_exists: exists,
    })
}

pub fn render_show(config: &EffectiveConfig, shell: &str, key: &KeyStatus) -> String {
    let file_state = if config.config_exists {
        "已存在"
    } else {
        "不存在，当前使用默认值"
    };
    let credentials = if key.configured {
        format!("{}（已脱敏）", key.source_label)
    } else {
        "未配置".to_string()
    };
    format!(
        "\
配置文件: {path}（{file_state}）
provider.name: {provider}
provider.base_url: {base_url}
provider.model: {model}
generation.temperature: {temperature}
generation.max_tokens: {max_tokens}
generation.timeout_secs: {timeout}
shell.default: {shell_default}
shell.resolved: {shell}
shell.keybinding: {keybinding}
shell.include_cwd: {include_cwd}
credentials: {credentials}
",
        path = config.config_path.display(),
        provider = config.provider_name,
        base_url = config.base_url,
        model = config.model,
        temperature = format_number(config.temperature),
        max_tokens = config.max_tokens,
        timeout = config.timeout_secs,
        shell_default = config.shell_default,
        keybinding = config.keybinding,
        include_cwd = config.include_cwd,
    )
}

pub fn key_status_from(result: Result<&ResolvedKey, &Error>) -> KeyStatus {
    match result {
        Ok(key) => KeyStatus {
            configured: true,
            source_label: key.source_label.clone(),
        },
        Err(Error::MissingApiKey { .. }) => KeyStatus {
            configured: false,
            source_label: "未配置".into(),
        },
        Err(Error::CredentialStoreUnavailable { reason }) => KeyStatus {
            configured: false,
            source_label: format!("未配置（系统凭据存储不可用: {reason}）"),
        },
        Err(err) => KeyStatus {
            configured: false,
            source_label: format!("未配置（{err}）"),
        },
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1_000_000.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_provider(name: &str) -> Result<String, Error> {
    let name = name.trim().to_ascii_lowercase();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'));
    if !valid {
        return Err(Error::InvalidConfig {
            path: "provider.name".into(),
            reason: format!("provider 名称无效: {name}"),
        });
    }
    Ok(name)
}

fn preset_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "deepseek" => Some(DEEPSEEK_BASE_URL),
        "openai" => Some(OPENAI_BASE_URL),
        _ => None,
    }
}

fn preset_model(provider: &str) -> Option<&'static str> {
    match provider {
        "deepseek" => Some(DEEPSEEK_MODEL),
        "openai" => Some(OPENAI_MODEL),
        _ => None,
    }
}

fn parse_timeout(value: &str) -> Result<u64, Error> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| Error::InvalidConfig {
            path: "CMDGEN_TIMEOUT".into(),
            reason: format!("必须是正整数秒，收到 {value:?}"),
        })
}
