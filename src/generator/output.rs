use crate::error::Error;
use crate::i18n;

pub fn normalize_command(raw: &str) -> Result<String, Error> {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    let stripped = strip_single_fence(text.trim());
    let command = stripped.trim();
    if command.is_empty() {
        return Err(Error::EmptyCommand);
    }
    if looks_like_explanation(command) {
        return Err(Error::InvalidResponse(i18n::t().err_explanation.into()));
    }
    Ok(command.to_string())
}

fn strip_single_fence(text: &str) -> String {
    let text = text.trim();
    if !text.starts_with("```") {
        return text.to_string();
    }
    let Some((language, rest)) = text[3..].split_once('\n') else {
        return text.to_string();
    };
    let language = language.trim();
    let language_ok = language.is_empty()
        || language
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+'));
    if !language_ok {
        return text.to_string();
    }
    let rest = rest.trim_end();
    let Some(body) = rest.strip_suffix("```") else {
        return text.to_string();
    };
    if !body.is_empty() && !body.ends_with('\n') {
        return text.to_string();
    }
    body.trim().to_string()
}

fn looks_like_explanation(text: &str) -> bool {
    const MARKERS: &[&str] = &[
        "here is the command",
        "here's the command",
        "here is a command",
        "the command is",
        "以下是",
        "命令如下",
        "这是命令",
        "下面是命令",
    ];
    let lower = text.to_lowercase();
    MARKERS.iter().any(|marker| lower.contains(marker))
}
