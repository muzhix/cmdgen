use std::fmt;
use std::path::Path;

use crate::i18n::{self, fill};

/// 用户可见错误。显示文本为中文，且不得包含 API Key。
#[derive(Debug)]
pub enum Error {
    ConfigNotFound {
        path: String,
    },

    InvalidConfig {
        path: String,
        reason: String,
    },

    MissingApiKey {
        message: String,
    },

    CredentialStoreUnavailable {
        reason: String,
    },

    UnsupportedProvider {
        name: String,
    },

    InvalidBaseUrl {
        reason: String,
    },

    NetworkError(String),

    Timeout {
        seconds: u64,
    },

    HttpError {
        status: u16,
        summary: String,
    },

    ApiError(String),

    InvalidResponse(String),

    EmptyCommand,

    ShellIntegrationError(String),

    Io {
        path: String,
        source: std::io::Error,
    },

    Usage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = i18n::t();
        let text = match self {
            Self::ConfigNotFound { path } => fill(m.err_config_not_found, &[("path", path)]),
            Self::InvalidConfig { path, reason } => {
                fill(m.err_invalid_config, &[("path", path), ("reason", reason)])
            }
            Self::MissingApiKey { message } => message.clone(),
            Self::CredentialStoreUnavailable { reason } => {
                fill(m.err_credential_store, &[("reason", reason)])
            }
            Self::UnsupportedProvider { name } => {
                fill(m.err_unsupported_provider, &[("name", name)])
            }
            Self::InvalidBaseUrl { reason } => fill(m.err_invalid_base_url, &[("reason", reason)]),
            Self::NetworkError(message) => fill(m.err_network, &[("message", message)]),
            Self::Timeout { seconds } => fill(m.err_timeout, &[("seconds", &seconds.to_string())]),
            Self::HttpError { status, summary } => fill(
                m.err_http,
                &[("status", &status.to_string()), ("summary", summary)],
            ),
            Self::ApiError(message) => fill(m.err_api, &[("message", message)]),
            Self::InvalidResponse(message) => fill(m.err_invalid_response, &[("message", message)]),
            Self::EmptyCommand => m.err_empty_command.to_string(),
            Self::ShellIntegrationError(message) => fill(m.err_shell, &[("message", message)]),
            Self::Io { path, source } => {
                fill(m.err_io, &[("path", path), ("source", &source.to_string())])
            }
            Self::Usage(message) => message.clone(),
        };
        formatter.write_str(&text)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Error {
    pub(crate) fn io(path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn invalid(path: impl AsRef<Path>, reason: impl Into<String>) -> Self {
        Self::InvalidConfig {
            path: path.as_ref().display().to_string(),
            reason: reason.into(),
        }
    }
}

/// 从文本中移除凭据。短于 4 个字符的值不替换，避免误伤普通单词。
pub fn redact(text: &str, secret: &str) -> String {
    let secret = secret.trim();
    if secret.chars().count() < 4 {
        return text.to_string();
    }
    text.replace(secret, "[REDACTED]")
}

pub(crate) fn missing_api_key(
    provider: &str,
    api_key_env: Option<&str>,
    store_problem: Option<String>,
) -> Error {
    let m = i18n::t();
    let steps = match api_key_env.filter(|name| !name.trim().is_empty()) {
        Some(var) => fill(
            m.missing_key_steps_with_var,
            &[("var", var), ("provider", provider)],
        ),
        None => fill(m.missing_key_steps_without_var, &[("provider", provider)]),
    };
    let mut message = fill(m.missing_key, &[("steps", &steps), ("provider", provider)]);
    if let Some(reason) = store_problem {
        message.push_str(&fill(m.store_unavailable_now, &[("reason", &reason)]));
    }
    Error::MissingApiKey { message }
}
