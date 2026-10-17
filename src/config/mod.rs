mod file;
pub mod secrets;

use std::path::PathBuf;

pub use file::{
    activate_provider, config_dir, config_path, home_dir, init_config, load_config, parse_config,
    provider_file_has_content, update_provider, FileConfig, InitOutcome, ProviderProfile,
    ProviderUpdate, DEFAULT_CONFIG_TOML,
};
pub use secrets::{
    delete_api_key, provider_env_var, resolve_api_key, save_api_key, CredentialFile, DeletedKey,
    EnvMap, KeyringStore, MemoryStore, ResolvedKey, SavedKey, SecretStore,
};

use crate::error::Error;
use crate::shell::detect::resolve_shell;
use crate::shell::integration::parse_keybinding;

/// 2026-10-17 核对 DeepSeek 文档：当前模型名为 `deepseek-flash`。
pub const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
pub const DEEPSEEK_MODEL: &str = "deepseek-flash";
/// 2026-10-17 核对百炼文档。共用域名不需要业务空间 ID；模型为文本生成页列出的 `qwen3.8-flash`。
pub const BAILIAN_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
pub const BAILIAN_MODEL: &str = "qwen3.8-flash";

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub shell: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredProvider {
    pub name: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveConfig {
    pub provider_name: String,
    pub base_url: String,
    pub model: String,
    /// 选中 provider 在 `CMDGEN_API_KEY` 之后要读取的环境变量。没有则为 `None`。
    pub api_key_env: Option<String>,
    pub configured_providers: Vec<ConfiguredProvider>,
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
    file::validate_config(&file, &path)?;
    let selected = select_provider(&file, &path, env, overrides)?;
    let configured_providers = catalog_providers(&file)?;

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
        provider_name: selected.name,
        base_url: selected.base_url,
        model: selected.model,
        api_key_env: selected.api_key_env,
        configured_providers,
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
    let providers = if config.configured_providers.is_empty() {
        "（无）".to_string()
    } else {
        config
            .configured_providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "\
配置文件: {path}（{file_state}）
providers: {providers}
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
        providers = providers,
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

pub fn render_providers(config: &EffectiveConfig) -> String {
    let mut out = format!("当前: {}\n", config.provider_name);
    if config.configured_providers.is_empty() {
        out.push_str("（尚未在配置文件中保存 provider）\n");
    }
    let mut listed = false;
    for provider in &config.configured_providers {
        let mark = if provider.name == config.provider_name {
            listed = true;
            "*"
        } else {
            " "
        };
        out.push_str(&format!(
            "{mark} {name}  {url}  {model}\n",
            name = provider.name,
            url = provider.base_url,
            model = provider.model
        ));
    }
    if !listed && !config.configured_providers.is_empty() {
        out.push_str(&format!(
            "* {}  {}  {}  （预设或本次覆盖，未写入 [providers]）\n",
            config.provider_name, config.base_url, config.model
        ));
    }
    out
}

struct SelectedProvider {
    name: String,
    base_url: String,
    model: String,
    api_key_env: Option<String>,
}

fn select_provider(
    file: &FileConfig,
    path: &PathBuf,
    env: &EnvMap,
    overrides: &Overrides,
) -> Result<SelectedProvider, Error> {
    let explicit =
        match first_non_empty([overrides.provider.as_deref(), env.get("CMDGEN_PROVIDER")]) {
            Some(name) => Some(normalize_provider(&name)?),
            None => None,
        };
    let legacy_name = match file::trimmed(&file.provider.name) {
        Some(name) => Some(normalize_provider(name)?),
        None => None,
    };
    let active = match file::trimmed(&file.active) {
        Some(name) => Some(normalize_provider(name)?),
        None => None,
    };
    let provider_name = if let Some(name) = explicit.clone() {
        name
    } else if !file.providers.is_empty() {
        active.clone().ok_or_else(|| {
            Error::invalid(
                path,
                "配置了 [providers] 但没有 active。请设置 active，或运行 `cmdgen config use <name>`。",
            )
        })?
    } else if let Some(name) = active.clone() {
        name
    } else if let Some(name) = legacy_name.clone() {
        name
    } else {
        "deepseek".to_string()
    };

    let profile = find_profile(file, &provider_name);
    let legacy_selected = profile.is_none()
        && file.providers.is_empty()
        && match explicit.as_deref() {
            None => true,
            Some(name) => legacy_name.as_deref() == Some(name),
        };

    let (saved_base, saved_model, explicit_env, legacy_openai) = if let Some(profile) = profile {
        (
            file::trimmed(&profile.base_url).map(str::to_string),
            file::trimmed(&profile.model).map(str::to_string),
            file::trimmed(&profile.api_key_env).map(str::to_string),
            false,
        )
    } else if legacy_selected {
        (
            file::trimmed(&file.provider.base_url).map(str::to_string),
            file::trimmed(&file.provider.model).map(str::to_string),
            None,
            true,
        )
    } else {
        (None, None, None, false)
    };

    let base_url = first_non_empty([
        overrides.base_url.as_deref(),
        env.get("CMDGEN_BASE_URL"),
        saved_base.as_deref(),
        preset_base_url(&provider_name),
    ])
    .ok_or_else(|| Error::UnsupportedProvider {
        name: provider_name.clone(),
    })?;
    let model = first_non_empty([
        overrides.model.as_deref(),
        env.get("CMDGEN_MODEL"),
        saved_model.as_deref(),
        preset_model(&provider_name),
    ])
    .ok_or_else(|| Error::UnsupportedProvider {
        name: provider_name.clone(),
    })?;

    let api_key_env = if let Some(var) = explicit_env {
        Some(var)
    } else if let Some(builtin) = builtin_api_key_env(&provider_name) {
        Some(builtin.to_string())
    } else if legacy_openai {
        Some("OPENAI_API_KEY".to_string())
    } else {
        None
    };

    Ok(SelectedProvider {
        name: provider_name,
        base_url,
        model,
        api_key_env,
    })
}

fn find_profile<'a>(file: &'a FileConfig, name: &str) -> Option<&'a file::ProviderProfile> {
    file.providers.iter().find_map(|(key, profile)| {
        (normalize_provider(key).ok().as_deref() == Some(name)).then_some(profile)
    })
}

fn catalog_providers(file: &FileConfig) -> Result<Vec<ConfiguredProvider>, Error> {
    let mut providers = Vec::new();
    if !file.providers.is_empty() {
        for (key, profile) in &file.providers {
            let name = normalize_provider(key)?;
            let (base_url, model) = profile_endpoint(&name, profile)?;
            providers.push(ConfiguredProvider {
                name,
                base_url,
                model,
            });
        }
    } else if provider_file_has_content(&file.provider) {
        let name = match file::trimmed(&file.provider.name) {
            Some(name) => normalize_provider(name)?,
            None => "deepseek".to_string(),
        };
        let base_url = first_non_empty([
            file::trimmed(&file.provider.base_url),
            preset_base_url(&name),
        ])
        .ok_or_else(|| Error::UnsupportedProvider { name: name.clone() })?;
        let model = first_non_empty([file::trimmed(&file.provider.model), preset_model(&name)])
            .ok_or_else(|| Error::UnsupportedProvider { name: name.clone() })?;
        providers.push(ConfiguredProvider {
            name,
            base_url,
            model,
        });
    }
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(providers)
}

fn profile_endpoint(
    name: &str,
    profile: &file::ProviderProfile,
) -> Result<(String, String), Error> {
    let base_url = first_non_empty([file::trimmed(&profile.base_url), preset_base_url(name)])
        .ok_or_else(|| Error::UnsupportedProvider {
            name: name.to_string(),
        })?;
    let model =
        first_non_empty([file::trimmed(&profile.model), preset_model(name)]).ok_or_else(|| {
            Error::UnsupportedProvider {
                name: name.to_string(),
            }
        })?;
    Ok((base_url, model))
}

fn builtin_api_key_env(provider: &str) -> Option<&'static str> {
    match provider {
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "bailian" => Some("DASHSCOPE_API_KEY"),
        _ => None,
    }
}

pub(crate) fn normalize_provider(name: &str) -> Result<String, Error> {
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

pub(crate) fn is_preset(provider: &str) -> bool {
    preset_base_url(provider).is_some()
}

fn preset_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "deepseek" => Some(DEEPSEEK_BASE_URL),
        "bailian" => Some(BAILIAN_BASE_URL),
        _ => None,
    }
}

fn preset_model(provider: &str) -> Option<&'static str> {
    match provider {
        "deepseek" => Some(DEEPSEEK_MODEL),
        "bailian" => Some(BAILIAN_MODEL),
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
