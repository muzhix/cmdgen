use std::path::Path;

/// 用户可见错误。显示文本为中文，且不得包含 API Key。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("找不到配置文件: {path}")]
    ConfigNotFound { path: String },

    #[error("配置无效 ({path}): {reason}")]
    InvalidConfig { path: String, reason: String },

    #[error("{message}")]
    MissingApiKey { message: String },

    #[error("系统凭据存储不可用: {reason}\n可以改用环境变量 CMDGEN_API_KEY，无需凭据存储。")]
    CredentialStoreUnavailable { reason: String },

    #[error(
        "不支持的 provider: {name}。请设置 base_url 和 model，或改用预设 deepseek / bailian。"
    )]
    UnsupportedProvider { name: String },

    #[error("base_url 无效: {reason}")]
    InvalidBaseUrl { reason: String },

    #[error("网络错误: {0}")]
    NetworkError(String),

    #[error("请求超时（{seconds} 秒）。可增大 generation.timeout_secs，或设置 CMDGEN_TIMEOUT。")]
    Timeout { seconds: u64 },

    #[error("HTTP {status}: {summary}")]
    HttpError { status: u16, summary: String },

    #[error("API 错误: {0}")]
    ApiError(String),

    #[error("模型响应无法解析: {0}")]
    InvalidResponse(String),

    #[error("模型返回了空命令")]
    EmptyCommand,

    #[error("Shell 集成错误: {0}")]
    ShellIntegrationError(String),

    #[error("文件操作失败 ({path}): {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{0}")]
    Usage(String),
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
    let steps = match api_key_env.filter(|name| !name.trim().is_empty()) {
        Some(var) => format!(
            "1. 环境变量 CMDGEN_API_KEY\n  \
             2. 环境变量 {var}\n  \
             3. 运行 `cmdgen config set-key`。优先保存到系统凭据存储（服务名 cmdgen，账号 {provider}）；凭据存储不可用时写入本地凭据文件 credentials.toml"
        ),
        None => format!(
            "1. 环境变量 CMDGEN_API_KEY\n  \
             2. 运行 `cmdgen config set-key`。优先保存到系统凭据存储（服务名 cmdgen，账号 {provider}）；凭据存储不可用时写入本地凭据文件 credentials.toml"
        ),
    };
    let mut message = format!(
        "未找到 API Key。请任选一种方式配置（不会写入 config.toml）：\n\
         \n  \
         {steps}\n\
         \n当前 provider: {provider}"
    );
    if let Some(reason) = store_problem {
        message.push_str("\n系统凭据存储当前不可用: ");
        message.push_str(&reason);
    }
    Error::MissingApiKey { message }
}
