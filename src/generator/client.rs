use std::io::Read;
use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{redact, Error};

const MAX_BODY_BYTES: u64 = 1_048_576;

pub struct RequestOptions<'a> {
    pub model: &'a str,
    pub temperature: f64,
    pub max_tokens: u32,
    pub system: &'a str,
    pub user: &'a str,
    pub disable_thinking: bool,
}

pub struct CompletionRequest<'a> {
    pub endpoint: &'a str,
    pub api_key: &'a str,
    pub body: &'a Value,
    pub timeout_secs: u64,
}

pub fn completions_endpoint(base_url: &str) -> Result<String, Error> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return Err(Error::InvalidBaseUrl {
            reason: "地址不能为空，也不能包含空白。".into(),
        });
    }
    if trimmed.contains('?') || trimmed.contains('#') {
        return Err(Error::InvalidBaseUrl {
            reason: "base_url 只应是 API 前缀，不能包含查询参数或片段。".into(),
        });
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("/chat/completions") {
        return Err(Error::InvalidBaseUrl {
            reason: "base_url 不应包含 /chat/completions，cmdgen 会自行拼接。".into(),
        });
    }
    let https = lower.starts_with("https://");
    let loopback_http = lower.starts_with("http://") && is_loopback_http(trimmed);
    if !https && !loopback_http {
        return Err(Error::InvalidBaseUrl {
            reason: "非本机地址必须使用 HTTPS。仅 127.0.0.1 和 localhost 允许 HTTP。".into(),
        });
    }
    if https && trimmed.len() <= "https://".len() {
        return Err(Error::InvalidBaseUrl {
            reason: "缺少主机名。".into(),
        });
    }
    Ok(format!("{trimmed}/chat/completions"))
}

pub fn build_request_body(options: &RequestOptions<'_>) -> Value {
    let mut body = json!({
        "model": options.model,
        "temperature": options.temperature,
        "max_tokens": options.max_tokens,
        "stream": false,
        "messages": [
            {"role": "system", "content": options.system},
            {"role": "user", "content": options.user},
        ],
    });
    if options.disable_thinking {
        body["thinking"] = json!({"type": "disabled"});
    }
    body
}

pub fn complete(request: &CompletionRequest<'_>) -> Result<String, Error> {
    let client = http_client(request.timeout_secs)?;
    let response = client
        .post(request.endpoint)
        .header(reqwest::header::USER_AGENT, user_agent())
        .bearer_auth(request.api_key)
        .json(request.body)
        .send()
        .map_err(|err| map_transport_error(&err, request.api_key, request.timeout_secs))?;
    let status = response.status().as_u16();
    let body = read_limited(response, request.api_key)?;
    interpret_http(status, &body, request.api_key)
}

pub fn probe_reachable(base_url: &str, timeout_secs: u64) -> Result<u16, Error> {
    let url = base_url.trim().trim_end_matches('/');
    let _endpoint = completions_endpoint(url)?;
    let client = http_client(timeout_secs)?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, user_agent())
        .send()
        .map_err(|err| map_transport_error(&err, "", timeout_secs))?;
    Ok(response.status().as_u16())
}

pub fn interpret_http(status: u16, body: &str, secret: &str) -> Result<String, Error> {
    let value: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) if !(200..300).contains(&status) => {
            return Err(Error::HttpError {
                status,
                summary: redact(&truncate(body), secret),
            });
        }
        Err(_) => {
            return Err(Error::InvalidResponse("响应不是有效的 JSON。".into()));
        }
    };

    if !(200..300).contains(&status) {
        return Err(Error::HttpError {
            status,
            summary: redact(&error_summary(&value, body), secret),
        });
    }

    if let Some(message) = value.get("error").and_then(error_message) {
        return Err(Error::ApiError(redact(&message, secret)));
    }

    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"));

    match content {
        Some(Value::String(text)) if text.trim().is_empty() => {
            if has_reasoning(&value) {
                Err(Error::InvalidResponse(
                    "模型只返回了推理内容，没有命令。请重试。".into(),
                ))
            } else {
                Err(Error::EmptyCommand)
            }
        }
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Null) | None => {
            if has_reasoning(&value) {
                Err(Error::InvalidResponse(
                    "模型只返回了推理内容，没有命令。请重试。".into(),
                ))
            } else {
                Err(Error::InvalidResponse(
                    "响应中缺少 choices[0].message.content。".into(),
                ))
            }
        }
        Some(_) => Err(Error::InvalidResponse(
            "choices[0].message.content 不是字符串。".into(),
        )),
    }
}

fn has_reasoning(value: &Value) -> bool {
    value
        .pointer("/choices/0/message/reasoning_content")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn error_message(error: &Value) -> Option<String> {
    if let Some(message) = error.get("message").and_then(Value::as_str) {
        return Some(message.trim().to_string());
    }
    if let Some(message) = error.as_str() {
        return Some(message.trim().to_string());
    }
    None
}

fn error_summary(value: &Value, body: &str) -> String {
    value
        .get("error")
        .and_then(error_message)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| truncate(body))
}

fn truncate(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut result = String::new();
    for ch in compact.chars().take(300) {
        result.push(ch);
    }
    if compact.chars().count() > 300 {
        result.push('…');
    }
    if result.is_empty() {
        "空的错误响应".into()
    } else {
        result
    }
}

fn http_client(timeout_secs: u64) -> Result<reqwest::blocking::Client, Error> {
    let timeout = Duration::from_secs(timeout_secs);
    let connect = Duration::from_secs((timeout_secs / 2).clamp(1, 10));
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .connect_timeout(connect)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| Error::NetworkError(err.to_string()))
}

fn read_limited(mut response: reqwest::blocking::Response, secret: &str) -> Result<String, Error> {
    let mut buffer = Vec::new();
    let mut limited = (&mut response).take(MAX_BODY_BYTES + 1);
    limited
        .read_to_end(&mut buffer)
        .map_err(|err| Error::NetworkError(redact(&err.to_string(), secret)))?;
    if buffer.len() as u64 > MAX_BODY_BYTES {
        return Err(Error::InvalidResponse("响应超过 1MB，已拒绝解析。".into()));
    }
    String::from_utf8(buffer).map_err(|_| Error::InvalidResponse("响应不是 UTF-8。".into()))
}

fn map_transport_error(err: &reqwest::Error, secret: &str, timeout_secs: u64) -> Error {
    if err.is_connect() {
        let reason = if err.is_timeout() {
            "连接 API 服务器超时"
        } else {
            "无法连接到 API 服务器"
        };
        return Error::NetworkError(reason.into());
    }
    if err.is_timeout() {
        return Error::Timeout {
            seconds: timeout_secs,
        };
    }
    let text = redact(&err.to_string(), secret);
    if text.to_ascii_lowercase().contains("certificate") {
        return Error::NetworkError(
            "TLS 证书校验失败。cmdgen 不会关闭证书验证，请检查系统证书或代理。".into(),
        );
    }
    Error::NetworkError(text)
}

fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.get("http://".len()..) else {
        return false;
    };
    let host = rest.split(['/', ':']).next().unwrap_or("");
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

fn user_agent() -> String {
    format!("cmdgen/{}", env!("CARGO_PKG_VERSION"))
}
