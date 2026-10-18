use crate::error::Error;
use crate::i18n::{self, fill};

pub fn os_label() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        other => other,
    }
}

/// 优先级：CLI `--shell` > `CMDGEN_SHELL` > 配置 > `$SHELL` > `sh`。
/// `auto` 和空值会继续向下识别。不会启动新的 Shell 进程。
pub fn resolve_shell(
    cli: Option<&str>,
    config_default: &str,
    cmdgen_shell: Option<&str>,
    shell_env: Option<&str>,
) -> Result<String, Error> {
    for candidate in [cli, cmdgen_shell, Some(config_default)] {
        if let Some(name) = nonempty_shell(candidate)? {
            return Ok(name);
        }
    }
    if let Some(name) = nonempty_shell(shell_env)? {
        return Ok(name);
    }
    Ok("sh".into())
}

fn nonempty_shell(value: Option<&str>) -> Result<Option<String>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    let name = normalize_shell_name(value);
    validate_shell_name(&name)?;
    Ok(Some(name))
}

fn normalize_shell_name(value: &str) -> String {
    let base = value.rsplit(['/', '\\']).next().unwrap_or(value).trim();
    let without_exe = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base);
    without_exe.to_ascii_lowercase()
}

fn validate_shell_name(name: &str) -> Result<(), Error> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '+'));
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidConfig {
            path: "shell".into(),
            reason: fill(i18n::t().err_shell_name, &[("name", name)]),
        })
    }
}
